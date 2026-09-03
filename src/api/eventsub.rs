use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::{error, info, warn};
use crate::processor::model::EventSubNotification;
use crate::processor::process_redemption;
use crate::state::AppState;

pub const MESSAGE_TYPE_VERIFICATION: &str = "webhook_callback_verification";
pub const MESSAGE_TYPE_NOTIFICATION: &str = "notification";
pub const MESSAGE_TYPE_REVOCATION: &str = "revocation";

#[derive(Deserialize)]
pub struct TwitchChallenge {
    pub challenge: String,
}

pub async fn handle_eventsub(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    fn get_header<'a>(headers: &'a HeaderMap, key: &str) -> &'a str {
        headers
            .get(key)
            .and_then(|val| val.to_str().ok())
            .unwrap_or("")
    }

    let msg_id = get_header(&headers, "twitch-eventsub-message-id");
    let msg_timestamp = get_header(&headers, "twitch-eventsub-message-timestamp");
    let msg_signature = get_header(&headers, "twitch-eventsub-message-signature");
    let msg_type = get_header(&headers, "twitch-eventsub-message-type");

    if msg_id.is_empty()
        || msg_timestamp.is_empty()
        || msg_signature.is_empty()
        || msg_type.is_empty()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if !verify_signature(&state.webhook_secret, msg_id, msg_timestamp, &body, msg_signature)
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    match msg_type {
        MESSAGE_TYPE_VERIFICATION => {
            if let Ok(payload) = serde_json::from_slice::<TwitchChallenge>(&body) {
                return (StatusCode::OK, payload.challenge).into_response();
            }
            StatusCode::BAD_REQUEST.into_response()
        }
        MESSAGE_TYPE_NOTIFICATION => {
            if let Ok(_notification) = serde_json::from_slice::<serde_json::Value>(&body) {
                let event_type = get_header(&headers, "twitch-eventsub-subscription-type");
                info!("Received event: {}", event_type);

                if !event_type.eq("channel.channel_points_custom_reward_redemption.add") {
                    return StatusCode::NO_CONTENT.into_response();
                }

                if let Ok(notification) = serde_json::from_slice::<EventSubNotification>(&body) {
                    let state_clone = state.clone();

                    tokio::spawn(async move {
                        process_redemption(state_clone, notification).await;
                    });
                }
            }
            StatusCode::NO_CONTENT.into_response()
        }
        MESSAGE_TYPE_REVOCATION => {
            warn!("Twitch revoked event subs.");
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&body) {
                let status = val.pointer("/subscription/status").and_then(|v| v.as_str()).unwrap_or("");
                let broadcaster_id = val.pointer("/subscription/condition/broadcaster_user_id").and_then(|v| v.as_str());
                let sub_type = val.pointer("/subscription/type").and_then(|v| v.as_str()).unwrap_or("");

                warn!(status = status, broadcaster_id = ?broadcaster_id, sub_type = sub_type, "EventSub subscription revoked");

                if let Some(bc_id) = broadcaster_id {
                    if status != "authorization_revoked" && status != "user_removed" {
                        let state_clone = state.clone();
                        let bc_id = bc_id.to_string();
                        tokio::spawn(async move {
                            info!("Attempting to re-subscribe revoked EventSub for broadcaster {}", bc_id);
                            if let Err(e) = state_clone.create_eventsub_subscription(&bc_id).await {
                                error!("Failed to re-subscribe after revocation for broadcaster {}: {}", bc_id, e);
                            } else {
                                info!("Successfully re-subscribed EventSub for broadcaster {}", bc_id);
                            }
                        });
                    }
                }
            }
            StatusCode::NO_CONTENT.into_response()
        }
        _ => {
            warn!("Unknown message type: {}", msg_type);
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

fn verify_signature(
    secret: &str,
    msg_id: &str,
    msg_timestamp: &str,
    body: &[u8],
    signature_header: &str,
) -> bool {
    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };

    let Ok(expected_mac_bytes) = hex::decode(hex_sig) else {
        return false;
    };

    let mut mac: Hmac<Sha256> =
        Hmac::new_from_slice(secret.as_bytes()).expect("HMAC key init failed");

    mac.update(msg_id.as_bytes());
    mac.update(msg_timestamp.as_bytes());
    mac.update(body);

    mac.verify_slice(&expected_mac_bytes).is_ok()
}
