use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::api::extractor::path::PathArg;
use crate::api::extractor::query::QueryArg;
use crate::db::redemptions::{Redemption, RedemptionStatus};
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RedemptionPath {
    pub redemption_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct RedemptionResponse {
    /// Twitch redemption UUID
    pub twitch_redemption_id: Uuid,
    /// Twitch reward UUID this redemption belongs to
    pub twitch_reward_id: Uuid,
    /// Twitch user ID who made the redemption
    pub user_id: String,
    /// Twitch login name of the user
    pub user_login: String,
    /// Steam trade link provided by the user
    pub user_trade_link: String,
    /// Twitch channel points spent
    pub twitch_points_cost: i64,
    /// Market price paid in cents (if any)
    pub market_paid_price: Option<i64>,
    /// Currency code (e.g. "RUB", "USD")
    pub currency: String,
    /// Current redemption status
    pub status: RedemptionStatus,
    /// Market item name redeemed/purchased (if known)
    pub market_item_name: Option<String>,
    /// Number of retry attempts
    pub retry_count: i32,
    /// Failure cause code (if failed)
    pub fail_cause: Option<String>,
    /// Human-readable failure description (if failed)
    pub fail_description: Option<String>,
    /// Redemption creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Redemption last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Redemption> for RedemptionResponse {
    fn from(r: Redemption) -> Self {
        Self {
            twitch_redemption_id: r.twitch_redemption_id,
            twitch_reward_id: r.twitch_reward_id,
            user_id: r.user_id,
            user_login: r.user_login,
            user_trade_link: r.user_trade_link,
            twitch_points_cost: r.twitch_points_cost,
            market_paid_price: r.market_paid_price,
            currency: r.currency,
            status: r.status,
            market_item_name: r.market_item_name,
            retry_count: r.retry_count,
            fail_cause: r.fail_cause,
            fail_description: r.fail_description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct PaginatedRedemptionsResponse {
    /// List of redemptions
    pub items: Vec<RedemptionResponse>,
    /// Total number of redemptions matching the filters
    pub total: i64,
    /// Number of records skipped
    pub offset: i64,
    /// Maximum number of records returned
    pub limit: i64,
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ListRedemptionsQuery {
    /// Filter by redemption status (PENDING, ORDER_CREATED, MANUAL_HOLD, FAILED_REFUND, FAILED_PENALTY, COMPLETED)
    pub status: Option<String>,
    /// Filter by reward UUID
    pub reward_id: Option<Uuid>,
    /// Filter by Twitch user ID
    pub user_id: Option<String>,
    /// Number of records to skip (default: 0)
    pub offset: Option<i64>,
    /// Maximum number of records to return (default: 50, max: 100)
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/redemptions",
    tag = "Redemptions",
    summary = "List redemptions",
    description = "Returns redemptions for a specific channel with optional filtering by status, reward, and user ID. Results are paginated.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ListRedemptionsQuery,
    ),
    responses(
        (status = 200, description = "List of redemptions", body = PaginatedRedemptionsResponse,
            example = json!({
                "items": [
                    {
                        "twitch_redemption_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                        "twitch_reward_id": "550e8400-e29b-41d4-a716-446655440000",
                        "user_id": "987654321",
                        "user_login": "some_viewer",
                        "user_trade_link": "https://steamcommunity.com/tradeoffer/new/?partner=123456&token=abcdef",
                        "twitch_points_cost": 5000,
                        "market_paid_price": 3500,
                        "currency": "RUB",
                        "status": "COMPLETED",
                        "fail_cause": null,
                        "fail_description": null,
                        "created_at": "2026-01-15T12:00:00Z",
                        "updated_at": "2026-01-15T12:05:00Z"
                    }
                ],
                "total": 1,
                "offset": 0,
                "limit": 50
            })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn list_redemptions(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<ListRedemptionsQuery>,
) -> Result<Json<PaginatedRedemptionsResponse>, ApiError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50).min(100);

    let redemptions = state.db.get_redemptions_by_broadcaster(
        &auth.channel_id,
        query.status.as_deref(),
        query.reward_id,
        query.user_id.as_deref(),
        offset,
        limit,
    ).await?;

    let total = state.db.count_redemptions_by_broadcaster(
        &auth.channel_id,
        query.status.as_deref(),
        query.reward_id,
        query.user_id.as_deref(),
    ).await?;

    Ok(Json(PaginatedRedemptionsResponse {
        items: redemptions.into_iter().map(RedemptionResponse::from).collect(),
        total,
        offset,
        limit,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/redemptions/{redemption_id}/retry",
    tag = "Redemptions",
    summary = "Retry a failed or manual hold redemption",
    description = "Retries purchasing the item on CSGO Market for a failed or manual hold redemption. Increments retry count, resets failure details, sets status to ORDER_CREATED, and spawns an order watcher on success.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("redemption_id" = Uuid, Path, description = "Twitch redemption UUID"),
    ),
    responses(
        (status = 200, description = "Retry attempt completed, order created", body = RedemptionResponse),
        (status = 400, description = "Market rejected the retry order"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — redemption does not belong to this channel"),
        (status = 404, description = "Redemption or associated reward not found"),
        (status = 422, description = "Cannot retry — redemption is not in a failed or manual hold state"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn retry_redemption(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RedemptionPath>,
) -> Result<Json<RedemptionResponse>, ApiError> {
    let redemption_id = path.redemption_id;
    let redemption = state.db.get_redemption(redemption_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Redemption not found".to_string(),
        })?;

    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Associated reward not found".to_string(),
        })?;

    if reward.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Redemption does not belong to this channel".to_string(),
        });
    }

    if !state.db.inventory_exists(redemption_id).await? {
        return Err(ApiError::UnprocessableEntity {
            message: "Historical redemption has no proven fixed inventory snapshot".into(),
            param: "redemption_id".into(),
        });
    }
    let result = crate::processor::inventory_fulfillment::purchase(&state, redemption_id, false, false).await
        .map_err(|message| ApiError::Internal { message })?;
    if result != "ORDER_CREATED" {
        return Err(ApiError::UnprocessableEntity {
            message: format!("Market order was not created: {result}"), param: "redemption_id".into(),
        });
    }
    let updated = state.db.get_redemption(redemption_id).await?.ok_or_else(|| ApiError::NotFound {
        message: "Redemption not found".into(),
    })?;
    Ok(Json(RedemptionResponse::from(updated)))
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/redemptions/{redemption_id}/refund",
    tag = "Redemptions",
    summary = "Refund a redemption",
    description = "Manually refunds a redemption. The user's Twitch channel points are restored and the redemption status is set to FAILED_REFUND.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("redemption_id" = Uuid, Path, description = "Twitch redemption UUID"),
    ),
    responses(
        (status = 200, description = "Redemption refunded successfully", body = serde_json::Value,
            example = json!({ "status": "refunded" })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — redemption does not belong to this channel"),
        (status = 404, description = "Redemption or associated reward not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn refund_redemption(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RedemptionPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let redemption_id = path.redemption_id;
    let redemption = state.db.get_redemption(redemption_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Redemption not found".to_string(),
        })?;

    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Associated reward not found".to_string(),
        })?;

    if reward.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Redemption does not belong to this channel".to_string(),
        });
    }

    if state.db.inventory_exists(redemption_id).await? {
        let result = crate::processor::inventory_fulfillment::refund(&state, redemption_id, false).await
            .map_err(|message| ApiError::Internal { message })?;
        if result != "REFUNDED" {
            return Err(ApiError::UnprocessableEntity { message: format!("Refund is not safe: {result}"), param: "redemption_id".into() });
        }
        return Ok(Json(serde_json::json!({ "status": "refunded" })));
    }

    match redemption.status {
        RedemptionStatus::Completed => {
            return Err(ApiError::UnprocessableEntity {
                message: "Cannot refund an already completed redemption".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        RedemptionStatus::FailedRefund => {
            return Err(ApiError::UnprocessableEntity {
                message: "Redemption has already been refunded".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        RedemptionStatus::FailedPenalty => {
            return Err(ApiError::UnprocessableEntity {
                message: "Cannot refund a redemption that was already penalized".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        _ => {}
    }

    let broadcaster_id = auth.channel_id.clone();
    let bc_ref = broadcaster_id.clone();
    let state_clone = Arc::clone(&state);
    state.with_broadcaster_token(&bc_ref, move |token| {
        let broadcaster_id = broadcaster_id.clone();
        let reward_id = redemption.twitch_reward_id.to_string();
        let redemption_id = redemption.twitch_redemption_id.to_string();
        let state_clone = Arc::clone(&state_clone);
        async move {
            state_clone.helix_client.update_redemption_status(
                &broadcaster_id,
                &reward_id,
                &redemption_id,
                true,
                &token,
            ).await
        }
    }).await?;

    state.db.update_redemption_status(
        redemption_id,
        RedemptionStatus::FailedRefund,
        Some("manual_refund"),
        Some("Manually refunded by channel owner/editor"),
    ).await?;

    state.channel_logger.log_redemption_manual_action(
        &auth.channel_id,
        &redemption_id.to_string(),
        &redemption.user_login,
        redemption.market_item_name.as_deref().or(reward.market_item_name.as_deref()),
        "REFUND",
        &auth.user_id,
        &auth.user_login,
    );

    state.channel_logger.log_redemption_status_changed(
        &auth.channel_id,
        &redemption_id.to_string(),
        &redemption.user_login,
        redemption.market_item_name.as_deref().or(reward.market_item_name.as_deref()),
        redemption.status.as_str(),
        "FAILED_REFUND",
        Some("manual_refund"),
        Some("Manually refunded by channel owner/editor"),
    );

    tracing::info!(
        redemption_id = %redemption_id,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        "Redemption manually refunded by editor"
    );

    Ok(Json(serde_json::json!({ "status": "refunded" })))
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/redemptions/{redemption_id}/penalty",
    tag = "Redemptions",
    summary = "Penalize a redemption",
    description = "Manually penalizes a redemption. The user's Twitch channel points are not restored and the redemption status is set to FAILED_PENALTY.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("redemption_id" = Uuid, Path, description = "Twitch redemption UUID"),
    ),
    responses(
        (status = 200, description = "Redemption penalized successfully", body = serde_json::Value,
            example = json!({ "status": "penalty" })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — redemption does not belong to this channel"),
        (status = 404, description = "Redemption or associated reward not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn penalty_redemption(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RedemptionPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let redemption_id = path.redemption_id;
    let redemption = state.db.get_redemption(redemption_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Redemption not found".to_string(),
        })?;

    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Associated reward not found".to_string(),
        })?;

    if reward.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Redemption does not belong to this channel".to_string(),
        });
    }

    if state.db.inventory_exists(redemption_id).await? {
        return Err(ApiError::UnprocessableEntity { message: "Inventory fulfillment remains pending until delivery or a safe explicit refund".into(), param: "redemption_id".into() });
    }

    match redemption.status {
        RedemptionStatus::Completed => {
            return Err(ApiError::UnprocessableEntity {
                message: "Cannot penalize an already completed redemption".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        RedemptionStatus::FailedRefund => {
            return Err(ApiError::UnprocessableEntity {
                message: "Cannot penalize a redemption that was already refunded".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        RedemptionStatus::FailedPenalty => {
            return Err(ApiError::UnprocessableEntity {
                message: "Redemption is already penalized".to_string(),
                param: "redemption_id".to_string(),
            });
        }
        _ => {}
    }

    let broadcaster_id = auth.channel_id.clone();
    let bc_ref = broadcaster_id.clone();
    let state_clone = Arc::clone(&state);
    state.with_broadcaster_token(&bc_ref, move |token| {
        let broadcaster_id = broadcaster_id.clone();
        let reward_id = redemption.twitch_reward_id.to_string();
        let redemption_id = redemption.twitch_redemption_id.to_string();
        let state_clone = Arc::clone(&state_clone);
        async move {
            state_clone.helix_client.update_redemption_status(
                &broadcaster_id,
                &reward_id,
                &redemption_id,
                false,
                &token,
            ).await
        }
    }).await?;

    state.db.update_redemption_status(
        redemption_id,
        RedemptionStatus::FailedPenalty,
        Some("manual_penalty"),
        Some("Manually penalized by channel owner/editor"),
    ).await?;

    state.channel_logger.log_redemption_manual_action(
        &auth.channel_id,
        &redemption_id.to_string(),
        &redemption.user_login,
        redemption.market_item_name.as_deref().or(reward.market_item_name.as_deref()),
        "PENALTY",
        &auth.user_id,
        &auth.user_login,
    );

    state.channel_logger.log_redemption_status_changed(
        &auth.channel_id,
        &redemption_id.to_string(),
        &redemption.user_login,
        redemption.market_item_name.as_deref().or(reward.market_item_name.as_deref()),
        redemption.status.as_str(),
        "FAILED_PENALTY",
        Some("manual_penalty"),
        Some("Manually penalized by channel owner/editor"),
    );

    tracing::info!(
        redemption_id = %redemption_id,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        "Redemption manually penalized by editor"
    );

    Ok(Json(serde_json::json!({ "status": "penalty" })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_redemption_response_contains_user_trade_link() {
        let redemption = Redemption {
            twitch_redemption_id: Uuid::new_v4(),
            twitch_reward_id: Uuid::new_v4(),
            user_id: "12345".to_string(),
            user_login: "streamer_fan".to_string(),
            user_trade_link: "https://steamcommunity.com/tradeoffer/new/?partner=123&token=abc".to_string(),
            twitch_points_cost: 5000,
            market_paid_price: Some(150),
            currency: "USD".to_string(),
            status: RedemptionStatus::Completed,
            fail_cause: None,
            fail_description: None,
            retry_count: 0,
            market_item_name: Some("AK-47 | Safari Mesh (Field-Tested)".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response: RedemptionResponse = redemption.into();
        assert_eq!(response.user_trade_link, "https://steamcommunity.com/tradeoffer/new/?partner=123&token=abc");

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"user_trade_link\":\"https://steamcommunity.com/tradeoffer/new/?partner=123&token=abc\""));
    }

    #[test]
    fn test_redemption_response_failure_details_serialization() {
        let redemption = Redemption {
            twitch_redemption_id: Uuid::new_v4(),
            twitch_reward_id: Uuid::new_v4(),
            user_id: "12345".to_string(),
            user_login: "viewer".to_string(),
            user_trade_link: "https://steamcommunity.com/tradeoffer/new/?partner=1&token=x".to_string(),
            twitch_points_cost: 1000,
            market_paid_price: None,
            currency: "RUB".to_string(),
            status: RedemptionStatus::FailedPenalty,
            fail_cause: Some("market_retry_failed".to_string()),
            fail_description: Some("Market rejected purchase retry: Item out of stock".to_string()),
            retry_count: 1,
            market_item_name: Some("AWP | Asiimov".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response: RedemptionResponse = redemption.into();
        assert_eq!(response.fail_cause.as_deref(), Some("market_retry_failed"));
        assert_eq!(response.fail_description.as_deref(), Some("Market rejected purchase retry: Item out of stock"));
        assert_eq!(response.status, RedemptionStatus::FailedPenalty);

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"fail_cause\":\"market_retry_failed\""));
        assert!(json.contains("\"fail_description\":\"Market rejected purchase retry: Item out of stock\""));
        assert!(json.contains("\"status\":\"FailedPenalty\""));
    }
}
