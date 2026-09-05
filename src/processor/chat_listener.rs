use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};
use crate::db::chat_messages::NewChatMessage;
use crate::state::AppState;

const DEFAULT_EVENTSUB_WS_URL: &str = "wss://eventsub.wss.twitch.tv/ws?keepalive_timeout_seconds=30";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventSubWsFrame {
    metadata: FrameMetadata,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FrameMetadata {
    message_id: String,
    message_type: String,
    #[serde(default)]
    message_timestamp: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionPayload {
    session: SessionInfo,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionInfo {
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    keepalive_timeout_seconds: Option<u64>,
    #[serde(default)]
    reconnect_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NotificationPayload {
    event: ChatEvent,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ChatEvent {
    broadcaster_user_id: String,
    broadcaster_user_login: String,
    chatter_user_id: String,
    chatter_user_login: String,
    message_id: String,
    message: ChatMessageContent,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ChatMessageContent {
    text: String,
}

/// Start the background chat listener task.
pub async fn run_chat_listener(state: Arc<AppState>) {
    info!("Twitch EventSub Chat WebSocket listener service starting...");

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    let mut target_url = DEFAULT_EVENTSUB_WS_URL.to_string();

    while !state.shutdown_token.is_cancelled() {
        // Ensure bot is initialized before trying to connect
        if !state.app_initialized.load(Ordering::Relaxed) {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                _ = state.shutdown_token.cancelled() => break,
            }
        }

        info!(url = %target_url, "Connecting to Twitch EventSub WebSocket...");

        match connect_and_listen(&state, &target_url).await {
            Ok(Some(reconnect_url)) => {
                info!(reconnect_url = %reconnect_url, "Switching to Twitch reconnect URL without backoff");
                target_url = reconnect_url;
                backoff = Duration::from_secs(1);
            }
            Ok(None) => {
                info!("WebSocket connection closed cleanly. Reconnecting to default URL...");
                target_url = DEFAULT_EVENTSUB_WS_URL.to_string();
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                warn!(error = %e, backoff_secs = backoff.as_secs(), "WebSocket connection lost or failed. Backing off...");
                target_url = DEFAULT_EVENTSUB_WS_URL.to_string();

                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {
                        backoff = (backoff * 2).min(max_backoff);
                    }
                    _ = state.shutdown_token.cancelled() => break,
                }
            }
        }
    }

    info!("Twitch EventSub Chat WebSocket listener service stopped.");
}

/// Connect to a given EventSub WebSocket URL and process incoming frames until closed or reconnect requested.
/// Returns Ok(Some(reconnect_url)) if server requested seamless reconnect.
async fn connect_and_listen(state: &Arc<AppState>, url: &str) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let (ws_stream, response) = connect_async(url).await?;
    debug!(status = %response.status(), "WebSocket handshake successful");

    let (mut write, mut read) = ws_stream.split();
    let mut keepalive_timeout = Duration::from_secs(35); // default fallback

    loop {
        tokio::select! {
            _ = state.shutdown_token.cancelled() => {
                info!("Shutdown signal received, sending WebSocket Close frame...");
                let _ = write.send(Message::Close(None)).await;
                *state.chat_session_id.write() = None;
                return Ok(None);
            }

            msg_res = tokio::time::timeout(keepalive_timeout, read.next()) => {
                let msg = match msg_res {
                    Ok(Some(Ok(m))) => m,
                    Ok(Some(Err(e))) => {
                        *state.chat_session_id.write() = None;
                        return Err(format!("WebSocket read error: {}", e).into());
                    }
                    Ok(None) => {
                        *state.chat_session_id.write() = None;
                        return Ok(None); // Stream ended
                    }
                    Err(_) => {
                        warn!("EventSub keepalive timeout exceeded ({}s with grace). Forcing reconnect...", keepalive_timeout.as_secs());
                        *state.chat_session_id.write() = None;
                        let _ = write.send(Message::Close(None)).await;
                        return Err("Keepalive watchdog timeout".into());
                    }
                };

                match msg {
                    Message::Text(text) => {
                        let frame: EventSubWsFrame = match serde_json::from_str(&text) {
                            Ok(f) => f,
                            Err(e) => {
                                warn!(error = %e, text = %text, "Failed to parse EventSub WebSocket frame");
                                continue;
                            }
                        };

                        match frame.metadata.message_type.as_str() {
                            "session_welcome" => {
                                if let Ok(payload) = serde_json::from_value::<SessionPayload>(frame.payload) {
                                    let session = payload.session;
                                    info!(session_id = %session.id, "Received EventSub session_welcome");
                                    *state.chat_session_id.write() = Some(session.id.clone());

                                    if let Some(ka) = session.keepalive_timeout_seconds {
                                        keepalive_timeout = Duration::from_secs(ka + 5);
                                    }

                                    // Subscribe all active broadcasters to channel.chat.message
                                    subscribe_all_active_broadcasters(state, &session.id).await;
                                }
                            }

                            "session_keepalive" => {
                                debug!("EventSub session_keepalive received");
                                // Keepalive timer is reset automatically by the loop iteration
                            }

                            "session_reconnect" => {
                                if let Ok(payload) = serde_json::from_value::<SessionPayload>(frame.payload) {
                                    if let Some(reconnect_url) = payload.session.reconnect_url {
                                        info!(reconnect_url = %reconnect_url, "Received session_reconnect request from Twitch. Executing seamless handover...");
                                        // Connect to the new URL and verify session_welcome BEFORE closing old connection!
                                        match perform_seamless_reconnect(state, &reconnect_url).await {
                                            Ok((new_session_id, new_stream)) => {
                                                info!(new_session_id = %new_session_id, "Seamless reconnect successful. Closing old connection...");
                                                let _ = write.send(Message::Close(None)).await;
                                                // Continue processing on the new socket by splitting and delegating
                                                let (new_write, new_read) = new_stream.split();
                                                write = new_write;
                                                read = new_read;
                                                continue;
                                            }
                                            Err(e) => {
                                                warn!(error = %e, "Failed seamless reconnect handover; falling back to clean reconnect");
                                                *state.chat_session_id.write() = None;
                                                return Ok(Some(reconnect_url));
                                            }
                                        }
                                    }
                                }
                            }

                            "notification" => {
                                if frame.metadata.subscription_type.as_deref() == Some("channel.chat.message") {
                                    handle_chat_message_notification(state, &frame.metadata.message_timestamp, frame.payload).await;
                                }
                            }

                            "revocation" => {
                                warn!(metadata = ?frame.metadata, "Received EventSub subscription revocation");
                            }

                            other => {
                                debug!(message_type = %other, "Received other EventSub message");
                            }
                        }
                    }

                    Message::Ping(payload) => {
                        debug!("Received WebSocket Ping; responding with Pong");
                        let _ = write.send(Message::Pong(payload)).await;
                    }

                    Message::Pong(_) => {
                        debug!("Received WebSocket Pong");
                    }

                    Message::Close(frame) => {
                        warn!(close_frame = ?frame, "Received WebSocket Close frame from Twitch");
                        *state.chat_session_id.write() = None;
                        return Ok(None);
                    }

                    Message::Binary(_) => {}
                    _ => {}
                }
            }
        }
    }
}

/// Perform seamless handover to reconnect_url: connects and waits for session_welcome
async fn perform_seamless_reconnect(
    state: &Arc<AppState>,
    reconnect_url: &str,
) -> Result<(String, tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws_stream, _) = connect_async(reconnect_url).await?;

    // Wait up to 10 seconds for session_welcome on the new connection
    let welcome_timeout = Duration::from_secs(10);
    let welcome_res: Result<String, Box<dyn std::error::Error + Send + Sync>> = tokio::time::timeout(welcome_timeout, async {
        while let Some(msg_res) = ws_stream.next().await {
            let msg = msg_res?;
            match msg {
                Message::Text(text) => {
                    let frame: EventSubWsFrame = serde_json::from_str(&text)?;
                    if frame.metadata.message_type == "session_welcome" {
                        if let Ok(payload) = serde_json::from_value::<SessionPayload>(frame.payload) {
                            return Ok(payload.session.id);
                        }
                    }
                }
                Message::Ping(p) => {
                    let _ = ws_stream.send(Message::Pong(p)).await;
                }
                _ => {}
            }
        }
        let err: Box<dyn std::error::Error + Send + Sync> = "New WebSocket stream ended without welcome".into();
        Err(err)
    }).await?;
    let welcome_session_id = welcome_res?;

    *state.chat_session_id.write() = Some(welcome_session_id.clone());
    Ok((welcome_session_id, ws_stream))
}

/// Subscribe all active broadcasters in DB to the given WebSocket session
async fn subscribe_all_active_broadcasters(state: &Arc<AppState>, session_id: &str) {
    let broadcasters = match state.db.get_all_broadcasters().await {
        Ok(b) => b,
        Err(e) => {
            error!(error = %e, "Failed to get broadcasters from DB for chat WebSocket subscription");
            return;
        }
    };

    let bot_id = {
        let guard = state.bot_info.read();
        match guard.as_ref() {
            Some(b) => b.user_id.clone(),
            None => {
                warn!("Bot is not initialized; skipping chat subscription");
                return;
            }
        }
    };

    for broadcaster in broadcasters {
        let is_active = match state.db.get_broadcaster_setting(&broadcaster.channel_id).await {
            Ok(Some(s)) => s.is_active,
            _ => true,
        };

        if !is_active {
            continue;
        }

        let bc_id = broadcaster.channel_id.clone();
        let b_id = bot_id.clone();
        let s_id = session_id.to_string();

        let sub_res = state.with_bot_user_token(|token| {
            let bc_id = bc_id.clone();
            let b_id = b_id.clone();
            let s_id = s_id.clone();
            async move {
                state.helix_client.create_chat_message_websocket_subscription(
                    &bc_id,
                    &b_id,
                    &s_id,
                    &token,
                ).await
            }
        }).await;

        match sub_res {
            Ok(_) => {
                info!(
                    broadcaster_id = %broadcaster.channel_id,
                    broadcaster_login = %broadcaster.channel_login,
                    "Subscribed to chat messages via EventSub WebSocket"
                );
            }
            Err(e) => {
                warn!(
                    error = %e,
                    broadcaster_id = %broadcaster.channel_id,
                    broadcaster_login = %broadcaster.channel_login,
                    "Failed to subscribe to chat messages on EventSub WebSocket"
                );
            }
        }
    }
}

/// Handle a single channel.chat.message notification frame
async fn handle_chat_message_notification(
    state: &Arc<AppState>,
    message_timestamp: &str,
    payload: serde_json::Value,
) {
    let notification: NotificationPayload = match serde_json::from_value(payload) {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "Failed to parse NotificationPayload for chat message");
            return;
        }
    };

    let event = notification.event;
    let raw_text = event.message.text;
    let trimmed = raw_text.trim();

    // 1. Ignore bot commands starting with '!' or '?'
    if trimmed.starts_with('!') || trimmed.starts_with('?') {
        debug!(text = %trimmed, "Ignoring command message");
        return;
    }

    // 2. Ignore messages sent by the bot account itself
    let bot_user_id = {
        let guard = state.bot_info.read();
        guard.as_ref().map(|b| b.user_id.clone())
    };

    if let Some(ref bot_id) = bot_user_id {
        if &event.chatter_user_id == bot_id {
            debug!("Ignoring message sent by bot account");
            return;
        }
    }

    // 3. Calculate character count (Unicode chars)
    let char_count = raw_text.chars().count() as i32;

    // 4. Parse sent_at timestamp from metadata
    let sent_at = DateTime::parse_from_rfc3339(message_timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let new_msg = NewChatMessage {
        message_id: event.message_id,
        broadcaster_id: event.broadcaster_user_id,
        chatter_user_id: event.chatter_user_id,
        chatter_user_login: event.chatter_user_login,
        message_text: raw_text,
        char_count,
        sent_at,
    };

    if let Err(e) = state.db.insert_chat_message(&new_msg).await {
        error!(error = %e, "Failed to insert chat message into database");
    } else {
        debug!(
            broadcaster = %new_msg.broadcaster_id,
            user = %new_msg.chatter_user_login,
            chars = char_count,
            "Recorded chat message"
        );
    }
}
