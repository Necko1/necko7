use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;
use utoipa::ToSchema;
use crate::api::extractor::json::JsonArg;
use crate::db::inventory::ViewerSettings;
use crate::steam::trade_link::TradeLink;
use crate::api::error::ApiError;
use crate::api::extractor::caller_user::CallerUser;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct UserResponse {
    /// Twitch user ID
    pub twitch_id: String,
    /// Twitch username/login
    pub login: String,
    /// Profile picture URL
    pub avatar_url: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/users/me",
    tag = "Users",
    summary = "Get current authenticated user",
    description = "Returns the profile information of the currently authenticated user based on session cookie.",
    responses(
        (status = 200, description = "Current user profile", body = UserResponse,
            example = json!({
                "twitch_id": "123456789",
                "login": "some_user",
                "avatar_url": "https://static-cdn.jtvnw.net/jtv_user_pictures/example.png"
            })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 404, description = "User not found in database"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_current_user(
    CallerUser { user_id }: CallerUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = state.db.get_user_by_twitch_id(&user_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "User not found".to_string(),
        })?;

    Ok(Json(UserResponse {
        twitch_id: user.twitch_id,
        login: user.login,
        avatar_url: user.avatar_url,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateViewerSettings {
    pub auto_buy_enabled: bool,
    pub trade_link: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/users/me/settings", tag = "Users", responses((status = 200, body = ViewerSettings)), security(("session_id" = [])))]
pub async fn get_viewer_settings(
    CallerUser { user_id }: CallerUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ViewerSettings>, ApiError> {
    Ok(Json(state.db.get_viewer_settings(&user_id).await?))
}

#[utoipa::path(put, path = "/api/v1/users/me/settings", tag = "Users", request_body = UpdateViewerSettings, responses((status = 200, body = ViewerSettings)), security(("session_id" = [])))]
pub async fn update_viewer_settings(
    CallerUser { user_id }: CallerUser,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<UpdateViewerSettings>,
) -> Result<Json<ViewerSettings>, ApiError> {
    let link = body.trade_link.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if link.is_some_and(|s| TradeLink::parse_single_url(s).is_none()) {
        return Err(ApiError::BadRequest { message: "Enter a valid Steam trade offer URL".into(), param: "trade_link".into() });
    }
    Ok(Json(state.db.save_viewer_settings(&user_id, body.auto_buy_enabled, link).await?))
}
