use std::sync::Arc;
use axum::extract::{State, Path};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::redemptions::{Redemption, RedemptionStatus};
use crate::state::AppState;

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
    /// Twitch channel points spent
    pub twitch_points_cost: i64,
    /// Market price paid in cents (if any)
    pub market_paid_price: Option<i64>,
    /// Current redemption status
    pub status: RedemptionStatus,
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
            twitch_points_cost: r.twitch_points_cost,
            market_paid_price: r.market_paid_price,
            status: r.status,
            fail_cause: r.fail_cause,
            fail_description: r.fail_description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ListRedemptionsQuery {
    /// Filter by redemption status (PENDING, ORDER_CREATED, FAILED_REFUND, FAILED_PENALTY, COMPLETED)
    pub status: Option<String>,
    /// Filter by reward UUID
    pub reward_id: Option<Uuid>,
    /// Number of records to skip (default: 0)
    pub offset: Option<i64>,
    /// Maximum number of records to return (default: 50, max: 100)
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/broadcasters/{channel_id}/redemptions",
    tag = "Redemptions",
    summary = "List redemptions",
    description = "Returns redemptions for a specific channel with optional filtering by status and reward. Results are paginated.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ListRedemptionsQuery,
    ),
    responses(
        (status = 200, description = "List of redemptions", body = Vec<RedemptionResponse>,
            example = json!([
                {
                    "twitch_redemption_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                    "twitch_reward_id": "550e8400-e29b-41d4-a716-446655440000",
                    "user_id": "987654321",
                    "user_login": "some_viewer",
                    "twitch_points_cost": 5000,
                    "market_paid_price": 3500,
                    "status": "COMPLETED",
                    "fail_cause": null,
                    "fail_description": null,
                    "created_at": "2026-01-15T12:00:00Z",
                    "updated_at": "2026-01-15T12:05:00Z"
                }
            ])
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
    axum::extract::Query(query): axum::extract::Query<ListRedemptionsQuery>,
) -> Result<Json<Vec<RedemptionResponse>>, ApiError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50).min(100);

    let redemptions = state.db.get_redemptions_by_broadcaster(
        &auth.channel_id,
        query.status.as_deref(),
        query.reward_id,
        offset,
        limit,
    ).await?;

    Ok(Json(redemptions.into_iter().map(RedemptionResponse::from).collect()))
}

#[utoipa::path(
    post,
    path = "/broadcasters/{channel_id}/redemptions/{redemption_id}/retry",
    tag = "Redemptions",
    summary = "Retry a failed redemption",
    description = "Retries a failed redemption by attempting to buy the item from the market again. Only works for redemptions with status FAILED_PENALTY.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("redemption_id" = Uuid, Path, description = "Twitch redemption UUID"),
    ),
    responses(
        (status = 200, description = "Retry attempt completed", body = serde_json::Value,
            example = json!({ "status": "order_created" })
        ),
        (status = 200, description = "Market error during retry", body = serde_json::Value,
            example = json!({ "status": "market_error", "error": "Item not available" })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — redemption does not belong to this channel"),
        (status = 404, description = "Redemption or associated reward not found"),
        (status = 422, description = "Cannot retry — redemption is not in a failed state"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn retry_redemption(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Path(redemption_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
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

    match redemption.status {
        RedemptionStatus::FailedPenalty => {}
        _ => return Err(ApiError::UnprocessableEntity {
            message: "Can only retry failed redemptions".to_string(),
            param: "redemption_id".to_string(),
        }),
    }

    let setting = state.db.get_broadcaster_setting(&auth.channel_id).await?
        .ok_or_else(|| ApiError::Internal {
            message: "Broadcaster settings not found".to_string(),
        })?;

    let trade_link = crate::steam::trade_link::TradeLink::parse(&redemption.user_trade_link)
        .ok_or_else(|| ApiError::Internal {
            message: "Invalid trade link stored for this redemption".to_string(),
        })?;

    let max_price = reward.current_market_price
        + (reward.current_market_price * reward.permissible_market_price_deviation / 100);

    let market_result = state.market_client.buy_for(
        &setting.market_api_key,
        &reward.market_item_name,
        max_price,
        setting.market_chance_to_transfer,
        trade_link,
        &redemption.twitch_redemption_id,
    ).await;

    match market_result {
        Ok(res) if res.success => {
            state.db.update_redemption_status(
                redemption_id,
                RedemptionStatus::OrderCreated,
                None,
                None,
            ).await?;

            Ok(Json(serde_json::json!({ "status": "order_created" })))
        }
        Ok(res) => {
            let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());

            state.db.update_redemption_status(
                redemption_id,
                RedemptionStatus::FailedPenalty, // doesn't change
                Some("market_retry_failed"),
                Some(&error_msg),
            ).await?;

            Ok(Json(serde_json::json!({
                "status": "market_error",
                "error": error_msg
            })))
        }
        Err(e) => Err(ApiError::Internal {
            message: format!("Market API request failed: {}", e),
        }),
    }
}

#[utoipa::path(
    post,
    path = "/broadcasters/{channel_id}/redemptions/{redemption_id}/refund",
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
    Path(redemption_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
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

    Ok(Json(serde_json::json!({ "status": "refunded" })))
}

#[utoipa::path(
    post,
    path = "/broadcasters/{channel_id}/redemptions/{redemption_id}/penalty",
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
    Path(redemption_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
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

    Ok(Json(serde_json::json!({ "status": "penalty" })))
}
