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
            None => {
                tracing::debug!("CallerUser rejected: missing session_id cookie");
                return Err(ApiError::Unauthorized {
                    message: "Missing session".to_string(),
                });
            }
        };

        let uuid = match Uuid::parse_str(session_id) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(error = %e, session_str = session_id, "CallerUser rejected: invalid session UUID format");
                return Err(ApiError::Unauthorized {
                    message: "Invalid session".to_string(),
                });
            }
        };

        match state.db.get_valid_session(uuid).await {
            Ok(Some(session)) => Ok(CallerUser {
                user_id: session.user_id,
            }),
            Ok(None) => {
                tracing::debug!(session_uuid = %uuid, "CallerUser rejected: session not found or expired in DB");
                Err(ApiError::Unauthorized {
                    message: "Invalid or expired session".to_string(),
                })
            }
            Err(e) => {
                tracing::error!(error = %e, session_uuid = %uuid, "CallerUser DB error checking session");
                Err(ApiError::Internal {
                    message: "Database error during session validation".to_string(),
                })
            }
        }
    }
}
