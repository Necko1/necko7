use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::api::extractor::path::PathArg;
use crate::api::extractor::query::QueryArg;
use crate::api::v1::redemptions::{PaginatedRedemptionsResponse, RedemptionResponse};
use crate::db::chat_messages::{ChatMessage, LeaderboardUserItem, UserChatSummary};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UserPathParam {
    pub user_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct PaginatedLeaderboardResponse {
    /// List of top chatters
    pub items: Vec<LeaderboardUserItem>,
    /// Total number of unique chatters matching the query
    pub total: i64,
    /// Number of records skipped
    pub offset: i64,
    /// Maximum number of records returned
    pub limit: i64,
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct LeaderboardQuery {
    /// Time window in hours (e.g. 6, 24, 48, 168 for week, 720 for month). If not specified, returns all time.
    pub time_window_hours: Option<i32>,
    /// Sort by: "messages", "characters", or "last_active" (default: "messages")
    pub sort_by: Option<String>,
    /// Sort order: "desc" or "asc" (default: "desc")
    pub order: Option<String>,
    /// Search substring for chatter username/login
    pub search: Option<String>,
    /// Number of records to skip (default: 0)
    pub offset: Option<i64>,
    /// Maximum number of records to return (default: 50, max: 100)
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/leaderboard",
    tag = "Chat Analytics",
    summary = "Get channel chat leaderboard",
    description = "Returns top chatters for a specific channel sorted by messages, characters, or last activity within an optional time window in hours.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        LeaderboardQuery,
    ),
    responses(
        (status = 200, description = "Chat leaderboard", body = PaginatedLeaderboardResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_chat_leaderboard(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<LeaderboardQuery>,
) -> Result<Json<PaginatedLeaderboardResponse>, ApiError> {
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let sort_by = query.sort_by.as_deref().unwrap_or("messages");
    let order = query.order.as_deref().unwrap_or("desc");

    let since = query.time_window_hours
        .filter(|&h| h > 0)
        .map(|h| Utc::now() - Duration::hours(h as i64));

    let (items, total) = state.db.get_leaderboard(
        &auth.channel_id,
        since,
        sort_by,
        order,
        limit,
        offset,
        query.search.as_deref(),
    ).await?;

    Ok(Json(PaginatedLeaderboardResponse {
        items,
        total,
        offset,
        limit,
    }))
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct UserStatsQuery {
    /// Time window in hours (e.g. 24, 168, 720). If not specified, returns all time.
    pub time_window_hours: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct UserChatStatsResponse {
    /// Twitch user ID
    pub user_id: String,
    /// Twitch user login (username)
    pub user_login: Option<String>,
    /// Twitch user display name
    pub display_name: Option<String>,
    /// Twitch user avatar image URL
    pub profile_image_url: Option<String>,
    /// Total messages sent by user in this channel within the window
    pub message_count: i64,
    /// Total characters sent by user in this channel within the window
    pub char_count: i64,
    /// Time window evaluated in hours (None if all time)
    pub time_window_hours: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/stats",
    tag = "Chat Analytics",
    summary = "Get user chat activity statistics",
    description = "Returns the count of messages and characters sent by a specific user in this channel within an optional time window, along with their Twitch username and avatar URL.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("user_id" = String, Path, description = "Twitch user ID"),
        UserStatsQuery,
    ),
    responses(
        (status = 200, description = "User chat statistics", body = UserChatStatsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_user_chat_stats(
    auth: AuthorizedChannel,
    PathArg(path): PathArg<UserPathParam>,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<UserStatsQuery>,
) -> Result<Json<UserChatStatsResponse>, ApiError> {
    let since = query.time_window_hours
        .filter(|&h| h > 0)
        .map(|h| Utc::now() - Duration::hours(h as i64));

    let (message_count, char_count) = state.db.get_user_chat_stats(
        &auth.channel_id,
        &path.user_id,
        since,
    ).await?;

    let twitch_user = state.get_twitch_user_cached(&path.user_id).await;

    let (user_login, display_name, profile_image_url) = if let Some(u) = twitch_user {
        (Some(u.login.clone()), Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
    } else {
        let fallback_login = match state.db.get_user_summary(&auth.channel_id, &path.user_id, None).await {
            Ok(Some(s)) => Some(s.chatter_user_login),
            _ => None,
        };
        (fallback_login, None, None)
    };

    Ok(Json(UserChatStatsResponse {
        user_id: path.user_id,
        user_login,
        display_name,
        profile_image_url,
        message_count,
        char_count,
        time_window_hours: query.time_window_hours,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/summary",
    tag = "Chat Analytics",
    summary = "Get user chat summary profile",
    description = "Returns lifetime summary for a user in this channel (first seen, last active, all-time totals).",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("user_id" = String, Path, description = "Twitch user ID"),
    ),
    responses(
        (status = 200, description = "User chat summary", body = Option<UserChatSummary>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_user_chat_summary(
    auth: AuthorizedChannel,
    PathArg(path): PathArg<UserPathParam>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Option<UserChatSummary>>, ApiError> {
    let summary = state.db.get_user_summary(
        &auth.channel_id,
        &path.user_id,
        None,
    ).await?;

    Ok(Json(summary))
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct UserMessagesQuery {
    /// Time window in hours. If not specified, returns messages from all time.
    pub time_window_hours: Option<i32>,
    /// Number of records to skip (default: 0)
    pub offset: Option<i64>,
    /// Maximum number of records to return (default: 50, max: 100)
    pub limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct PaginatedUserMessagesResponse {
    /// List of chat messages
    pub items: Vec<ChatMessage>,
    /// Total number of messages matching the filter
    pub total: i64,
    /// Number of records skipped
    pub offset: i64,
    /// Maximum number of records returned
    pub limit: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/messages",
    tag = "Chat Analytics",
    summary = "Get user message history",
    description = "Returns chronological message history sent by a specific user in this channel with timestamp and character counts.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("user_id" = String, Path, description = "Twitch user ID"),
        UserMessagesQuery,
    ),
    responses(
        (status = 200, description = "User message history", body = PaginatedUserMessagesResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_user_chat_messages(
    auth: AuthorizedChannel,
    PathArg(path): PathArg<UserPathParam>,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<UserMessagesQuery>,
) -> Result<Json<PaginatedUserMessagesResponse>, ApiError> {
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let since = query.time_window_hours
        .filter(|&h| h > 0)
        .map(|h| Utc::now() - Duration::hours(h as i64));

    let (items, total) = state.db.get_user_messages(
        &auth.channel_id,
        &path.user_id,
        since,
        limit,
        offset,
    ).await?;

    Ok(Json(PaginatedUserMessagesResponse {
        items,
        total,
        offset,
        limit,
    }))
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct UserRedemptionsQuery {
    /// Number of records to skip (default: 0)
    pub offset: Option<i64>,
    /// Maximum number of records to return (default: 50, max: 100)
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/redemptions",
    tag = "Chat Analytics",
    summary = "Get redemptions made by a specific user",
    description = "Returns all reward redemptions for a user on this channel including Steam trade links, market item names, points, and statuses.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("user_id" = String, Path, description = "Twitch user ID"),
        UserRedemptionsQuery,
    ),
    responses(
        (status = 200, description = "User redemptions list", body = PaginatedRedemptionsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_user_redemptions(
    auth: AuthorizedChannel,
    PathArg(path): PathArg<UserPathParam>,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<UserRedemptionsQuery>,
) -> Result<Json<PaginatedRedemptionsResponse>, ApiError> {
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let redemptions = state.db.get_redemptions_by_broadcaster(
        &auth.channel_id,
        None,
        None,
        Some(&path.user_id),
        offset,
        limit,
    ).await?;

    let total = state.db.count_redemptions_by_broadcaster(
        &auth.channel_id,
        None,
        None,
        Some(&path.user_id),
    ).await?;

    Ok(Json(PaginatedRedemptionsResponse {
        items: redemptions.into_iter().map(RedemptionResponse::from).collect(),
        total,
        offset,
        limit,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_chat_stats_response_serialization() {
        let stats = UserChatStatsResponse {
            user_id: "12345678".to_string(),
            user_login: Some("cool_viewer".to_string()),
            display_name: Some("Cool_Viewer".to_string()),
            profile_image_url: Some("https://static-cdn.jtvnw.net/jtv_user_pictures/avatar.png".to_string()),
            message_count: 42,
            char_count: 512,
            time_window_hours: Some(24),
        };

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"user_login\":\"cool_viewer\""));
        assert!(json.contains("\"display_name\":\"Cool_Viewer\""));
        assert!(json.contains("\"profile_image_url\":\"https://static-cdn.jtvnw.net/jtv_user_pictures/avatar.png\""));
        assert!(json.contains("\"message_count\":42"));
    }
}
