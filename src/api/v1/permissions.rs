use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::channel_permissions::{ChannelRole, NewChannelPermission};
use crate::state::AppState;

#[derive(Serialize)]
pub struct PermissionResponse {
    pub channel_id: String,
    pub user_id: String,
    pub role: ChannelRole,
    pub granted_by: String,
    pub user_login: String,
}

pub async fn list_permissions(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PermissionResponse>>, ApiError> {
    auth.require_owner()?;

    let permissions = state.db.get_permissions_by_channel(&auth.channel_id).await?;

    let mut result = Vec::new();
    for perm in permissions {
        let user_login = state.db.get_user_by_twitch_id(&perm.user_id).await?
            .map(|u| u.login)
            .unwrap_or_default();

        result.push(PermissionResponse {
            channel_id: perm.channel_id,
            user_id: perm.user_id,
            role: perm.role,
            granted_by: perm.granted_by,
            user_login,
        });
    }

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct GrantPermissionBody {
    pub login: String,
}

pub async fn grant_permission(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Json(body): Json<GrantPermissionBody>,
) -> Result<Json<PermissionResponse>, ApiError> {
    auth.require_owner()?;

    let user = state.db.get_user_by_login(&body.login).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("User with login '{}' not found", body.login),
        })?;

    let new_perm = NewChannelPermission {
        channel_id: auth.channel_id.clone(),
        user_id: user.twitch_id.clone(),
        role: ChannelRole::Editor,
        granted_by: auth.user_id.clone(),
    };

    let perm = state.db.upsert_permission(&new_perm).await?;

    Ok(Json(PermissionResponse {
        channel_id: perm.channel_id,
        user_id: perm.user_id,
        role: perm.role,
        granted_by: perm.granted_by,
        user_login: user.login,
    }))
}

pub async fn revoke_permission(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    auth.require_owner()?;

    if user_id == auth.user_id {
        return Err(ApiError::Forbidden {
            message: "Cannot revoke your own owner permission".to_string(),
        });
    }

    state.db.delete_permission(&auth.channel_id, &user_id).await?;

    Ok(Json(serde_json::json!({ "deleted": true }))
    )
}
