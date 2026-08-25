use std::collections::HashMap;
use std::sync::Arc;
use axum::extract::{FromRequestParts, Path};
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use uuid::Uuid;
use crate::api::error::ApiError;
use crate::db::channel_permissions::ChannelRole;
use crate::state::AppState;

pub struct AuthorizedChannel {
    pub user_id: String,
    pub channel_id: String,
    pub role: ChannelRole,
}

impl AuthorizedChannel {
    pub fn require_owner(&self) -> Result<(), ApiError> {
        if self.role != ChannelRole::Owner {
            return Err(ApiError::Forbidden {
                message: "Owner access required".to_string(),
            });
        }
        Ok(())
    }
}

impl FromRequestParts<Arc<AppState>> for AuthorizedChannel {
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

        let user_id = match state.db.get_valid_session(uuid).await {
            Ok(Some(session)) => session.user_id,
            _ => return Err(ApiError::Unauthorized {
                message: "Invalid or expired session".to_string(),
            }),
        };

        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::BadRequest {
                message: "Missing channel_id in path".to_string(),
                param: "channel_id".to_string(),
            })?;

        let channel_id = params.get("channel_id")
            .ok_or_else(|| ApiError::BadRequest {
                message: "Missing channel_id in path".to_string(),
                param: "channel_id".to_string(),
            })?
            .clone();

        let permission = state.db.get_permission(&channel_id, &user_id).await?
            .ok_or_else(|| ApiError::Forbidden {
                message: format!("No access to channel {}", channel_id),
            })?;

        Ok(AuthorizedChannel {
            user_id,
            channel_id,
            role: permission.role,
        })
    }
}
