use std::sync::Arc;
use std::sync::atomic::Ordering;
use axum::extract::{Query, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{Duration, Utc};
use serde::Deserialize;
use tracing::{error, info, warn};
use urlencoding::encode;
use uuid::Uuid;
use crate::api::cookie::{build_cookie_string, build_csrf_cookie};
use crate::state::{AppState, BotInfo};
use crate::db::app_settings::KEY_BOT_AUTH;
use crate::db::broadcaster_settings::NewBroadcasterSetting;
use crate::db::broadcasters::NewBroadcaster;
use crate::db::channel_permissions::{ChannelRole, NewChannelPermission};
use crate::db::sessions::NewSession;
use crate::db::users::NewUser;

pub const STREAMER_AUTH_SCOPES: &str = "channel:read:redemptions channel:manage:redemptions channel:bot";
pub const BOT_AUTH_SCOPES: &str = "user:write:chat user:bot";

#[derive(Deserialize)]
pub struct AuthQuery {
    pub code: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
    pub state: Option<String>,
}

pub async fn bot_login_redirect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.app_initialized.load(Ordering::Relaxed) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let csrf_state = format!("bot:{}", Uuid::new_v4());

    let cookie = build_csrf_cookie(&state, &csrf_state);
    let auth_url = get_auth_url(state, BOT_AUTH_SCOPES, &csrf_state);

    ([(SET_COOKIE, cookie)], Redirect::to(&auth_url)).into_response()
}

pub async fn streamer_login_redirect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let csrf_state = format!("streamer:{}", Uuid::new_v4());

    let cookie = build_csrf_cookie(&state, &csrf_state);
    let auth_url = get_auth_url(state, &STREAMER_AUTH_SCOPES, &csrf_state);

    ([(SET_COOKIE, cookie)], Redirect::to(&auth_url)).into_response()
}

pub async fn user_login_redirect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let csrf_state = format!("user:{}", Uuid::new_v4());

    let cookie = build_csrf_cookie(&state, &csrf_state);
    let auth_url = get_auth_url(state, "", &csrf_state);

    ([(SET_COOKIE, cookie)], Redirect::to(&auth_url)).into_response()
}

fn get_auth_url(state: Arc<AppState>, scopes: &str, csrf_state: &str) -> String {
    let redirect_uri = format!("{}/auth/callback", state.app_url);

    format!(
        "https://id.twitch.tv/oauth2/authorize?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&force_verify=true",
        state.client_id,
        encode(&redirect_uri),
        encode(scopes),
        encode(csrf_state)
    )
}

pub async fn auth_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<AuthQuery>,
) -> Response {
    if let Some(err) = params.error {
        let description = params.error_description.unwrap_or("".to_string());
        error!("OAuth error from Twitch: {}; description: {}", err, description);
        return (StatusCode::BAD_REQUEST, format!("OAuth error ({}): {}", err, description)).into_response();
    }

    let code = match params.code {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, "Auth code not found").into_response(),
    };

    let query_state = match params.state {
        Some(s) => s,
        None => return (StatusCode::BAD_REQUEST, "State parameter missing").into_response(),
    };

    let cookie_header = headers
        .get(COOKIE)
        .and_then(|val| val.to_str().ok())
        .unwrap_or("");

    let stored_state = cookie_header
        .split(';')
        .find_map(|c| {
            let mut parts = c.trim().splitn(2, '=');
            if parts.next()? == "oauth_state" {
                parts.next()
            } else {
                None
            }
        });

    let clear_cookie_str = build_cookie_string("oauth_state", "", 0, &state.app_url);

        match stored_state {
        Some(expected) if expected == query_state => {
            // success
        }
        _ => {
            warn!(
                query_state = %query_state,
                stored_state = ?stored_state,
                "OAuth CSRF verification failed: state mismatch or missing cookie"
            );
            return (
                StatusCode::FORBIDDEN,
                [(SET_COOKIE, clear_cookie_str.as_str())],
                "CSRF verification failed or session expired",
            ).into_response();
        }
    }

    let redirect_uri = format!("{}/auth/callback", state.app_url);

    let token_resp = match state.helix_client.exchange_code_for_user_token(&code, &redirect_uri).await
    {
        Ok(data) => data,
        Err(err) => {
            error!(error = %err, "Failed to exchange authorization code for user token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(SET_COOKIE, clear_cookie_str.as_str())],
                "Failed to exchange code for user token",
            ).into_response();
        }
    };

    let user_token = token_resp.access_token;
    let refresh_token = token_resp.refresh_token;

    let info = match state.helix_client.get_user_info_by_token(&user_token).await
    {
        Ok(info) => info,
        Err(err) => {
            error!(error = %err, "Failed to get user info from Twitch Helix after token exchange");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(SET_COOKIE, clear_cookie_str.as_str())],
                "Failed to get user info"
            ).into_response();
        }
    };

    let (channel_id, channel_login) = (info.id, info.login);

    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&clear_cookie_str).unwrap());

    if query_state.starts_with("bot:") {
        if state.app_initialized.load(Ordering::Relaxed) {
            warn!(channel_login = %channel_login, channel_id = %channel_id, "Bot authorization attempted on already-initialized application");
            return (
                StatusCode::EXPECTATION_FAILED,
                response_headers,
                "Bot already initialized"
            ).into_response()
        }

        let bot_info = BotInfo {
            user_login: channel_login.clone(),
            user_id: channel_id.clone(),
            access_token: user_token,
            refresh_token,
        };

        if let Ok(bot_info_str) = serde_json::to_string(&bot_info) {
            state.db.set_setting(KEY_BOT_AUTH, &bot_info_str).await.unwrap();

            {
                let mut write_lock = state.bot_info.write();
                *write_lock = Some(bot_info);
            }

            state.app_initialized.store(true, Ordering::Relaxed);

            info!(bot_login = %channel_login, bot_id = %channel_id, "Bot account successfully authorized and saved to DB");
        } else {
            error!(bot_login = %channel_login, bot_id = %channel_id, "Failed to serialize bot account info to JSON");
        }
    } else if query_state.starts_with("streamer:") {
        let new_broadcaster = NewBroadcaster {
            channel_id: channel_id.clone(),
            channel_login: channel_login.clone(),
            user_access_token: user_token,
            refresh_token,
        };

        if let Err(e) = state.db.upsert_broadcaster(&new_broadcaster).await {
            error!(error = %e, channel_id = %channel_id, "DB Error saving broadcaster");
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }

        let new_setting = NewBroadcasterSetting {
            channel_id: channel_id.clone(),
            is_active: true,
            market_api_key: "".to_string(),
            base_price_multiplier: 200,
            update_prices_period: 3600,
            refund_on_buyer_fail: false,
            refund_if_no_money: true,
            pause_reward_if_no_money: false,
            market_chance_to_transfer: 0,
        };
        
        if let Err(e) = state.db.upsert_broadcaster_setting(&new_setting).await {
            error!(error = %e, channel_id = %channel_id, "DB Error saving broadcaster setting");
            return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
        }

        let new_permission = NewChannelPermission {
            channel_id: channel_id.clone(),
            user_id: channel_id.clone(),
            role: ChannelRole::Owner,
            granted_by: channel_id.clone(),
        };

        if let Err(e) = state.db.upsert_permission(&new_permission).await {
            error!(error = %e, channel_id = %channel_id, "Failed to upsert OWNER permission in DB");
        }

        let state_clone = state.clone();
        let cid_clone = channel_id.clone();
        tokio::spawn(async move {
            if let Err(err) = state_clone.create_eventsub_subscription(&cid_clone).await {
                error!(error = %err, channel_id = %cid_clone, "Failed to create EventSub subscription for new streamer");
            }
        });

        crate::processor::start_broadcaster_tasks(state.clone(), channel_id.clone());

        info!(channel_login = %channel_login, channel_id = %channel_id, "Streamer successfully connected the bot");
    } else if query_state.starts_with("user:") {
        let new_user = NewUser {
            twitch_id: channel_id.clone(),
            login: channel_login.clone(),
            avatar_url: Some(info.profile_image_url),
        };

        let session_id = Uuid::new_v4();
        let new_session = NewSession {
            session_id,
            user_id: channel_id.clone(),
            expires_at: Utc::now() + Duration::weeks(1),
        };

        if let Err(e) = state.db.upsert_user(&new_user).await {
            error!(error = %e, user_id = %channel_id, "DB Error saving user in auth callback");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                response_headers,
                "DB Error saving user"
            ).into_response();
        }
        if let Err(e) = state.db.create_session(&new_session).await {
            error!(error = %e, user_id = %channel_id, "DB Error saving session in auth callback");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                response_headers,
                "DB Error saving session"
            ).into_response();
        }

        let session_cookie = build_cookie_string(
            "session_id",
            &session_id.to_string(),
            7 * 24 * 60 * 60,
            &state.app_url,
        );

        response_headers.append(
            SET_COOKIE,
            HeaderValue::from_str(&session_cookie).unwrap()
        );

        info!(user_id = %channel_id, user_login = %channel_login, session_id = %session_id, "User logged in to dashboard successfully");
    } else {
        warn!(state = %query_state, "Unknown OAuth state prefix received in callback");
    }

    let frontend_dashboard_url = format!("{}/dashboard", state.frontend_url);

    (
        response_headers,
        Redirect::to(&frontend_dashboard_url)
    ).into_response()
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let cookie_header = headers
        .get(COOKIE)
        .and_then(|val| val.to_str().ok())
        .unwrap_or("");

    let session_id = cookie_header
        .split(';')
        .find_map(|c| {
            let mut parts = c.trim().splitn(2, '=');
            if parts.next()? == "session_id" {
                parts.next()
            } else {
                None
            }
        });

    if let Some(sid_str) = session_id {
        if let Ok(uuid) = Uuid::parse_str(sid_str) {
            info!(session_id = %uuid, "Logging out user: deleting session from DB");
            if let Err(e) = state.db.delete_session(uuid).await {
                warn!(error = %e, session_id = %uuid, "Failed to delete session from DB during logout");
            }
        }
    }

    let clear_cookie = build_cookie_string("session_id", "", 0, &state.app_url);
    let mut response_headers = HeaderMap::new();
    if let Ok(hv) = HeaderValue::from_str(&clear_cookie) {
        response_headers.insert(SET_COOKIE, hv);
    }

    (
        response_headers,
        axum::Json(serde_json::json!({ "success": true }))
    ).into_response()
}
