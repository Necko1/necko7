use std::sync::Arc;
use axum::extract::{State, Path};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::redemptions::{Redemption, RedemptionStatus};
use crate::state::AppState;

#[derive(Serialize)]
pub struct RedemptionResponse {
    pub twitch_redemption_id: Uuid,
    pub twitch_reward_id: Uuid,
    pub user_id: String,
    pub user_login: String,
    pub twitch_points_cost: i64,
    pub market_paid_price: Option<i64>,
    pub status: RedemptionStatus,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
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

#[derive(Deserialize)]
pub struct ListRedemptionsQuery {
    pub status: Option<String>,
    pub reward_id: Option<Uuid>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

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
        RedemptionStatus::FailedRefund | RedemptionStatus::FailedPenalty => {}
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
                RedemptionStatus::FailedRefund,
                Some("market_error"),
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
