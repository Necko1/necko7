use std::sync::Arc;
use axum::extract::{State, Path};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::rewards::Reward;
use crate::state::AppState;
use crate::helix::api::custom_rewards::model::CreateCustomReward;

#[derive(Serialize)]
pub struct RewardResponse {
    pub twitch_id: Uuid,
    pub is_paused: bool,
    pub is_deleted: bool,
    pub streamer_id: String,
    pub market_item_name: String,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
    pub market_autobuy: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl From<Reward> for RewardResponse {
    fn from(r: Reward) -> Self {
        Self {
            twitch_id: r.twitch_id,
            is_paused: r.is_paused,
            is_deleted: r.is_deleted,
            streamer_id: r.streamer_id,
            market_item_name: r.market_item_name,
            twitch_title: r.twitch_title,
            twitch_description: r.twitch_description,
            current_market_price: r.current_market_price,
            permissible_market_price_deviation: r.permissible_market_price_deviation,
            twitch_price_markup_percentage: r.twitch_price_markup_percentage,
            global_cooldown_seconds: r.global_cooldown_seconds,
            max_redemptions_per_stream: r.max_redemptions_per_stream,
            max_redemptions_per_user_per_stream: r.max_redemptions_per_user_per_stream,
            market_autobuy: r.market_autobuy,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct ListRewardsQuery {
    pub is_paused: Option<bool>,
    pub is_deleted: Option<bool>,
}

pub async fn list_rewards(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<ListRewardsQuery>,
) -> Result<Json<Vec<RewardResponse>>, ApiError> {
    let rewards = state.db.get_rewards_by_streamer_filtered(
        &auth.channel_id,
        query.is_paused,
        query.is_deleted,
    ).await?;

    Ok(Json(rewards.into_iter().map(RewardResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateRewardBody {
    pub twitch_id: Uuid,
    pub market_item_name: String,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
    pub market_autobuy: bool,
    pub is_paused: bool,
}

pub async fn create_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRewardBody>,
) -> Result<Json<RewardResponse>, ApiError> {
    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    if !setting.market_api_key.is_empty() {
        let reward_info = CreateCustomReward {
            title: body.twitch_title.clone(),
            cost: 1,
            description: Some(body.twitch_description.clone()),
            background_color: None,
            max_per_stream: Some(body.max_redemptions_per_stream as u32),
            max_per_user_per_stream: Some(body.max_redemptions_per_user_per_stream as u32),
            global_cooldown_seconds: Some(body.global_cooldown_seconds as u32),
        };

        let broadcaster_id = auth.channel_id.clone();
        let bc_ref = broadcaster_id.clone();
        let state_clone = Arc::clone(&state);
        state.with_broadcaster_token(&bc_ref, move |token| {
            let reward_info = reward_info.clone();
            let broadcaster_id = broadcaster_id.clone();
            let state_clone = Arc::clone(&state_clone);
            async move {
                state_clone.helix_client.create_custom_reward(
                    &broadcaster_id,
                    reward_info,
                    &token,
                ).await
            }
        }).await?;
    }

    let new_reward = crate::db::rewards::NewReward {
        twitch_id: body.twitch_id,
        is_paused: body.is_paused,
        streamer_id: auth.channel_id.clone(),
        market_item_name: body.market_item_name,
        twitch_title: body.twitch_title,
        twitch_description: body.twitch_description,
        current_market_price: body.current_market_price,
        permissible_market_price_deviation: body.permissible_market_price_deviation,
        twitch_price_markup_percentage: body.twitch_price_markup_percentage,
        global_cooldown_seconds: body.global_cooldown_seconds,
        max_redemptions_per_stream: body.max_redemptions_per_stream,
        max_redemptions_per_user_per_stream: body.max_redemptions_per_user_per_stream,
        market_autobuy: body.market_autobuy,
    };

    let reward = state.db.create_reward(&new_reward).await?;

    Ok(Json(RewardResponse::from(reward)))
}

#[derive(Deserialize)]
pub struct UpdateRewardBody {
    pub twitch_title: Option<String>,
    pub twitch_description: Option<String>,
    pub current_market_price: Option<i32>,
    pub permissible_market_price_deviation: Option<i32>,
    pub twitch_price_markup_percentage: Option<i16>,
    pub global_cooldown_seconds: Option<i32>,
    pub max_redemptions_per_stream: Option<i16>,
    pub max_redemptions_per_user_per_stream: Option<i16>,
    pub market_autobuy: Option<bool>,
    pub is_paused: Option<bool>,
}

pub async fn update_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Path(reward_id): Path<Uuid>,
    Json(body): Json<UpdateRewardBody>,
) -> Result<Json<RewardResponse>, ApiError> {
    let existing = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Reward not found".to_string(),
        })?;

    if existing.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Reward does not belong to this channel".to_string(),
        });
    }

    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    if !setting.market_api_key.is_empty() {
        let update_info = crate::helix::api::custom_rewards::model::UpdateCustomReward {
            title: body.twitch_title.clone(),
            cost: None,
            description: body.twitch_description.clone(),
            background_color: None,
            max_per_stream: body.max_redemptions_per_stream.map(|v| v as u32),
            max_per_user_per_stream: body.max_redemptions_per_user_per_stream.map(|v| v as u32),
            global_cooldown_seconds: body.global_cooldown_seconds.map(|v| v as u32),
            is_paused: body.is_paused,
        };

        let broadcaster_id = auth.channel_id.clone();
        let bc_ref = broadcaster_id.clone();
        let state_clone = Arc::clone(&state);
        state.with_broadcaster_token(&bc_ref, move |token| {
            let update_info = update_info.clone();
            let broadcaster_id = broadcaster_id.clone();
            let reward_id_str = reward_id.to_string();
            let state_clone = Arc::clone(&state_clone);
            async move {
                state_clone.helix_client.update_custom_reward(
                    &broadcaster_id,
                    &reward_id_str,
                    update_info,
                    &token,
                ).await
            }
        }).await?;
    }

    let patch = crate::db::rewards::UpdateReward {
        is_paused: body.is_paused,
        is_deleted: None,
        market_item_name: None,
        twitch_title: body.twitch_title,
        twitch_description: body.twitch_description,
        current_market_price: body.current_market_price,
        permissible_market_price_deviation: body.permissible_market_price_deviation,
        twitch_price_markup_percentage: body.twitch_price_markup_percentage,
        global_cooldown_seconds: body.global_cooldown_seconds,
        max_redemptions_per_stream: body.max_redemptions_per_stream,
        max_redemptions_per_user_per_stream: body.max_redemptions_per_user_per_stream,
        market_autobuy: body.market_autobuy,
    };

    state.db.update_reward(reward_id, &patch).await?;

    let updated = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::Internal {
            message: "Failed to fetch updated reward".to_string(),
        })?;

    Ok(Json(RewardResponse::from(updated)))
}

pub async fn delete_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Path(reward_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let existing = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Reward not found".to_string(),
        })?;

    if existing.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Reward does not belong to this channel".to_string(),
        });
    }

    let broadcaster_id = auth.channel_id.clone();
    let bc_ref = broadcaster_id.clone();
    let state_clone = Arc::clone(&state);
    state.with_broadcaster_token(&bc_ref, move |token| {
        let broadcaster_id = broadcaster_id.clone();
        let reward_id_str = reward_id.to_string();
        let state_clone = Arc::clone(&state_clone);
        async move {
            state_clone.helix_client.delete_custom_reward(
                &broadcaster_id,
                &reward_id_str,
                &token,
            ).await
        }
    }).await?;

    state.db.set_reward_deleted(reward_id).await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn update_reward_price(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Path(reward_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let existing = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Reward not found".to_string(),
        })?;

    if existing.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Reward does not belong to this channel".to_string(),
        });
    }

    state.db.update_reward_market_price(reward_id, existing.current_market_price).await?;

    Ok(Json(serde_json::json!({ "updated": true })))
}

#[derive(Deserialize)]
pub struct BatchRewardBody {
    pub action: String,
    pub reward_ids: Vec<Uuid>,
}

pub async fn batch_rewards(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchRewardBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut affected = 0u32;

    for reward_id in &body.reward_ids {
        let existing = state.db.get_reward_by_twitch_id(*reward_id).await?;
        let existing = match existing {
            Some(r) if r.streamer_id == auth.channel_id => r,
            _ => continue,
        };

        match body.action.as_str() {
            "pause" => {
                if !existing.is_paused {
                    state.db.set_reward_paused(*reward_id, true).await?;
                    affected += 1;
                }
            }
            "unpause" => {
                if existing.is_paused {
                    state.db.set_reward_paused(*reward_id, false).await?;
                    affected += 1;
                }
            }
            "delete" => {
                state.db.set_reward_deleted(*reward_id).await?;
                affected += 1;
            }
            _ => return Err(ApiError::BadRequest {
                message: format!("Unknown batch action: {}", body.action),
                param: "action".to_string(),
            }),
        }
    }

    Ok(Json(serde_json::json!({ "affected": affected })))
}
