use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    /// Total number of redemptions in the period
    pub total_redemptions: i64,
    /// Number of completed redemptions
    pub completed: i64,
    /// Number of failed redemptions
    pub failed: i64,
    /// Total amount spent in cents (from completed redemptions)
    pub total_spent: i64,
    /// Total Twitch channel points earned from completed redemptions
    pub total_points_earned: i64,
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct StatsQuery {
    /// Time period: "year", "month", "week", or "custom"
    pub period: String,
    /// Start date for custom period (ISO 8601 format). Required when period is "custom".
    pub from: Option<DateTime<Utc>>,
    /// End date for custom period (ISO 8601 format). Defaults to now.
    pub to: Option<DateTime<Utc>>,
}

#[utoipa::path(
    get,
    path = "/broadcasters/{channel_id}/stats",
    tag = "Stats",
    summary = "Get redemption statistics",
    description = "Returns redemption statistics for a specific channel and time period. Supports predefined periods (year, month, week) and custom date ranges.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        StatsQuery,
    ),
    responses(
        (status = 200, description = "Redemption statistics", body = StatsResponse,
            example = json!({
                "total_redemptions": 150,
                "completed": 120,
                "failed": 30,
                "total_spent": 420000,
                "total_points_earned": 750000
            })
        ),
        (status = 400, description = "Invalid period parameter or missing 'from' for custom period"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_stats(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<StatsQuery>,
) -> Result<Json<StatsResponse>, ApiError> {
    let now = Utc::now();

    let (from, to) = match query.period.as_str() {
        "year" => (now - Duration::days(365), now),
        "month" => (now - Duration::days(30), now),
        "week" => (now - Duration::days(7), now),
        "custom" => {
            let from = query.from.ok_or_else(|| ApiError::BadRequest {
                message: "from parameter required for custom period".to_string(),
                param: "from".to_string(),
            })?;
            let to = query.to.unwrap_or(now);
            (from, to)
        }
        _ => return Err(ApiError::BadRequest {
            message: "Invalid period. Use: year, month, week, custom".to_string(),
            param: "period".to_string(),
        }),
    };

    let stats = state.db.get_redemption_stats(&auth.channel_id, from, to).await?;

    Ok(Json(StatsResponse {
        total_redemptions: stats.total_redemptions,
        completed: stats.completed,
        failed: stats.failed,
        total_spent: stats.total_spent,
        total_points_earned: stats.total_points_earned,
    }))
}
