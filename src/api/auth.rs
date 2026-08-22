use std::sync::Arc;
use std::sync::atomic::Ordering;
use axum::extract::{Query, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tracing::{error, info, warn};
use urlencoding::encode;
use uuid::Uuid;
use crate::state::{AppState, BotInfo};
use crate::db::app_settings::KEY_BOT_AUTH;
use crate::db::broadcasters::NewBroadcaster;

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

    let auth_url = get_auth_url(state, BOT_AUTH_SCOPES, &csrf_state);

    let cookie = format!(
        "oauth_state={}; HttpOnly; SameSite=Lax; Path=/auth; Max-Age=300",
        csrf_state
    );

    ([(SET_COOKIE, cookie)], Redirect::to(&auth_url)).into_response()
}

pub async fn streamer_login_redirect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let csrf_state = format!("streamer:{}", Uuid::new_v4());
    let auth_url = get_auth_url(state, &STREAMER_AUTH_SCOPES, &csrf_state);

    let cookie = format!(
        "oauth_state={}; HttpOnly; SameSite=Lax; Path=/auth; Max-Age=300",
        csrf_state
    );

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

    let clear_cookie = (SET_COOKIE, "oauth_state=; HttpOnly; SameSite=Lax; Path=/auth; Max-Age=0");

    match stored_state {
        Some(expected) if expected == query_state => {
            // success
        }
        _ => {
            error!("CSRF attack detected or session expired. Query state: {}, Cookie state: {:?}", query_state, stored_state);
            return (
                StatusCode::FORBIDDEN,
                [clear_cookie],
                "CSRF verification failed or session expired",
            ).into_response();
        }
    }

    let redirect_uri = format!("{}/auth/callback", state.app_url);

    let token_resp = match state.helix_client.exchange_code_for_user_token(&code, &redirect_uri).await
    {
        Ok(data) => data,
        Err(err) => {
            error!("Error while exchange code for user token: {:?}", err);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [clear_cookie],
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
            error!("Failed to get user info: {:?}", err);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [clear_cookie],
                "Failed to get user info"
            ).into_response();
        }
    };

    let (channel_id, channel_login) = (info.id, info.login);

    if query_state.starts_with("bot:") {
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

            info!("Bot account {} (ID: {}) successfully authorized!", channel_login, channel_id);
        } else {
            warn!("Failed to save authorized bot account {} (ID: {})", channel_login, channel_id)
        }
    } else if query_state.starts_with("streamer:") {
        let new_broadcaster = NewBroadcaster {
            channel_id: channel_id.clone(),
            channel_login: channel_login.clone(),
            user_access_token: user_token,
            refresh_token,
        };

        state.db.upsert_broadcaster(&new_broadcaster).await.unwrap();

        info!("Streamer {} (ID: {}) successfully authorized!", channel_login, channel_id);
    } else {
        warn!("Unknown state prefix: {}", query_state);
    }

    if query_state.starts_with("streamer:") {
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(err) =
                state_clone.create_eventsub_subscription(&channel_id).await
            {
                error!("Failed to create EventSub subscription: {:?}", err);
                return;
            }
        });
    }

    (
        StatusCode::OK,
        [clear_cookie],
        "Success"
    ).into_response()
}
