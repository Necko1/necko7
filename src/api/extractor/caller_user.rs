use std::sync::Arc;
use axum::extract::FromRequestParts;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use uuid::Uuid;
use crate::api::error::ApiError;
use crate::state::AppState;

pub struct CallerUser {
    pub user_id: String,
}

impl FromRequestParts<Arc<AppState>> for CallerUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let cookie_header = parts
            .headers
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

        let session_id = match session_id {
            Some(s) => s,
            None => return Err(ApiError::Unauthorized {
                message: "Missing session".to_string(),
            }),
        };

        let uuid = match Uuid::parse_str(session_id) {
            Ok(u) => u,
            Err(_) => return Err(ApiError::Unauthorized {
                message: "Invalid session".to_string(),
            }),
        };

        match state.db.get_valid_session(uuid).await {
            Ok(Some(session)) => Ok(CallerUser {
                user_id: session.user_id,
            }),
            _ => Err(ApiError::Unauthorized {
                message: "Invalid or expired session".to_string(),
            }),
        }
    }
}
