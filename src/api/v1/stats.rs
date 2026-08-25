use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::state::AppState;

#[derive(Serialize)]
pub struct StatsResponse {
    pub total_redemptions: i64,
    pub completed: i64,
    pub failed: i64,
    pub total_spent: i64,
    pub total_points_earned: i64,
}

#[derive(Deserialize)]
pub struct StatsQuery {
    pub period: String,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

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
