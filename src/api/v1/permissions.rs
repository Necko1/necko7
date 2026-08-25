use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::channel_permissions::{ChannelRole, NewChannelPermission};
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct PermissionResponse {
    /// Twitch channel ID this permission is for
    pub channel_id: String,
    /// Twitch user ID who has the permission
    pub user_id: String,
    /// Role granted to the user (OWNER or EDITOR)
    pub role: ChannelRole,
    /// Twitch user ID of the person who granted this permission
    pub granted_by: String,
    /// Twitch login name of the user who has the permission
    pub user_login: String,
}

#[utoipa::path(
    get,
    path = "/broadcasters/{channel_id}/permissions",
    tag = "Permissions",
    summary = "List channel permissions",
    description = "Returns all permissions for a specific channel. Only the channel owner can access this endpoint.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    responses(
        (status = 200, description = "List of permissions", body = Vec<PermissionResponse>,
            example = json!([
                {
                    "channel_id": "123456789",
                    "user_id": "987654321",
                    "role": "EDITOR",
                    "granted_by": "123456789",
                    "user_login": "some_editor"
                }
            ])
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — only the channel owner can manage permissions"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
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

#[derive(Deserialize, ToSchema)]
pub struct GrantPermissionBody {
    /// Twitch login name of the user to grant access to
    pub login: String,
}

#[utoipa::path(
    post,
    path = "/broadcasters/{channel_id}/permissions",
    tag = "Permissions",
    summary = "Grant permission to a user",
    description = "Grants EDITOR permission to a user for the specified channel. The user must already exist in the system. Only the channel owner can grant permissions.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    request_body = GrantPermissionBody,
    responses(
        (status = 200, description = "Permission granted successfully", body = PermissionResponse,
            example = json!({
                "channel_id": "123456789",
                "user_id": "987654321",
                "role": "EDITOR",
                "granted_by": "123456789",
                "user_login": "some_editor"
            })
        ),
        (status = 400, description = "Invalid request body"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — only the channel owner can grant permissions"),
        (status = 404, description = "Broadcaster settings not found, or user not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
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

#[utoipa::path(
    delete,
    path = "/broadcasters/{channel_id}/permissions/{user_id}",
    tag = "Permissions",
    summary = "Revoke permission from a user",
    description = "Revokes a user's permission for the specified channel. The user cannot revoke their own owner permission.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("user_id" = String, Path, description = "Twitch user ID of the user whose permission to revoke"),
    ),
    responses(
        (status = 200, description = "Permission revoked successfully",
            example = json!({ "deleted": true })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — only the channel owner can revoke permissions, or attempting to revoke own owner permission"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
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
