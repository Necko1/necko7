use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::api::extractor::query::QueryArg;
use crate::db::channel_logs::{ChannelLog, ChannelLogsSummary, ListChannelLogsQuery};
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct ChannelLogResponse {
    /// Unique log entry ID
    pub id: i64,
    /// Twitch broadcaster user ID
    pub broadcaster_id: String,
    /// Log severity level (DEBUG, INFO, WARN, ERROR)
    pub level: String,
    /// Log category (REDEMPTION, REWARD, MARKET, BOT, AUTH, SYSTEM)
    pub category: String,
    /// Machine-readable event identifier
    pub event_type: String,
    /// Human-readable log message
    pub message: String,
    /// Structured contextual details (e.g. redemption ID, price, user login)
    pub details: Option<serde_json::Value>,
    /// Actionable recommendation on how to resolve the issue
    pub solution_hint: Option<String>,
    /// Timestamp when the log event was recorded
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<ChannelLog> for ChannelLogResponse {
    fn from(log: ChannelLog) -> Self {
        Self {
            id: log.id,
            broadcaster_id: log.broadcaster_id,
            level: log.level,
            category: log.category,
            event_type: log.event_type,
            message: log.message,
            details: log.details,
            solution_hint: log.solution_hint,
            created_at: log.created_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct PaginatedChannelLogsResponse {
    /// List of channel log entries
    pub items: Vec<ChannelLogResponse>,
    /// Total number of log entries matching the filters
    pub total: i64,
    /// Number of records skipped
    pub offset: i64,
    /// Maximum number of records returned
    pub limit: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ChannelLogsSummaryResponse {
    /// Count of errors within the last 24 hours
    pub errors_last_24h: i64,
    /// Count of warnings within the last 24 hours
    pub warnings_last_24h: i64,
    /// Count of info events within the last 24 hours
    pub info_last_24h: i64,
    /// Total log events within the last 24 hours
    pub total_last_24h: i64,
}

impl From<ChannelLogsSummary> for ChannelLogsSummaryResponse {
    fn from(s: ChannelLogsSummary) -> Self {
        Self {
            errors_last_24h: s.errors_last_24h,
            warnings_last_24h: s.warnings_last_24h,
            info_last_24h: s.info_last_24h,
            total_last_24h: s.total_last_24h,
        }
    }
}

/// Retrieve paginated channel logs with filtering by level, category, text search, and date range.
#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/logs",
    params(
        ("channel_id" = String, Path, description = "Twitch broadcaster user ID"),
        ListChannelLogsQuery
    ),
    responses(
        (status = 200, description = "Paginated list of channel logs", body = PaginatedChannelLogsResponse),
        (status = 401, description = "Unauthorized - missing or invalid session cookie"),
        (status = 403, description = "Forbidden - caller does not have owner/editor role on this channel"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("session_id" = [])
    ),
    tag = "Channel Logs"
)]
pub async fn list_channel_logs(
    auth: AuthorizedChannel,
    QueryArg(query): QueryArg<ListChannelLogsQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PaginatedChannelLogsResponse>, ApiError> {
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);

    let (logs, total) = state
        .db
        .list_channel_logs(&auth.channel_id, &query)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, channel_id = %auth.channel_id, "Failed to list channel logs from DB");
            ApiError::Internal {
                message: "Failed to fetch channel logs".to_string(),
            }
        })?;

    let items = logs.into_iter().map(ChannelLogResponse::from).collect();

    Ok(Json(PaginatedChannelLogsResponse {
        items,
        total,
        offset,
        limit,
    }))
}

/// Retrieve aggregated error and warning statistics for the broadcaster channel for the last 24 hours.
#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/logs/summary",
    params(
        ("channel_id" = String, Path, description = "Twitch broadcaster user ID")
    ),
    responses(
        (status = 200, description = "Summary of errors and warnings in the last 24 hours", body = ChannelLogsSummaryResponse),
        (status = 401, description = "Unauthorized - missing or invalid session cookie"),
        (status = 403, description = "Forbidden - caller does not have owner/editor role on this channel"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("session_id" = [])
    ),
    tag = "Channel Logs"
)]
pub async fn get_channel_logs_summary(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ChannelLogsSummaryResponse>, ApiError> {
    let summary = state
        .db
        .get_channel_logs_summary(&auth.channel_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, channel_id = %auth.channel_id, "Failed to fetch channel logs summary from DB");
            ApiError::Internal {
                message: "Failed to fetch channel logs summary".to_string(),
            }
        })?;

    Ok(Json(ChannelLogsSummaryResponse::from(summary)))
}
