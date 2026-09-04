use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::api::extractor::json::JsonArg;
use crate::api::extractor::query::QueryArg;
use crate::api::extractor::path::PathArg;
use crate::db::rewards::Reward;
use crate::state::AppState;
use crate::helix::api::custom_rewards::model::CreateCustomReward;
use crate::steam::market;

#[derive(Deserialize)]
pub struct RewardPath {
    pub reward_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct RewardResponse {
    /// Twitch reward UUID
    pub twitch_id: Uuid,
    /// Whether the reward is currently paused
    pub is_paused: bool,
    /// Reason why the reward is paused ("MANUAL", "NO_MONEY"). Null if reward is active.
    pub pause_reason: Option<crate::db::rewards::PauseReason>,
    /// Whether the reward has been soft-deleted
    pub is_deleted: bool,
    /// Twitch channel ID of the streamer
    pub streamer_id: String,
    /// Market item name for automated purchases
    pub market_item_name: String,
    /// Twitch reward title
    pub twitch_title: String,
    /// Twitch reward description
    pub twitch_description: String,
    /// Current market price in cents
    pub current_market_price: i32,
    /// Permissible market price deviation percentage
    pub permissible_market_price_deviation: i32,
    /// Twitch price markup percentage
    pub twitch_price_markup_percentage: i16,
    /// Global cooldown in seconds between redemptions
    pub global_cooldown_seconds: i32,
    /// Maximum redemptions per stream
    pub max_redemptions_per_stream: i16,
    /// Maximum redemptions per user per stream
    pub max_redemptions_per_user_per_stream: i16,
    /// Whether to automatically buy from the market
    pub market_autobuy: bool,
    /// Currency code (e.g. "RUB", "USD")
    pub currency: String,
    /// Reward creation timestamp
    pub created_at: chrono::DateTime<Utc>,
    /// Reward last update timestamp
    pub updated_at: chrono::DateTime<Utc>,
}

impl From<Reward> for RewardResponse {
    fn from(r: Reward) -> Self {
        Self {
            twitch_id: r.twitch_id,
            is_paused: r.is_paused,
            pause_reason: r.pause_reason,
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
            currency: r.currency,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ListRewardsQuery {
    /// Filter by paused status
    pub is_paused: Option<bool>,
    /// Filter by deleted status
    pub is_deleted: Option<bool>,
    /// Filter by pause reason ("MANUAL", "NO_MONEY")
    pub pause_reason: Option<crate::db::rewards::PauseReason>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/rewards",
    tag = "Rewards",
    summary = "List rewards",
    description = "Returns all rewards for a specific channel, with optional filtering by paused, deleted status, and pause reason.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ListRewardsQuery,
    ),
    responses(
        (status = 200, description = "List of rewards", body = Vec<RewardResponse>,
            example = json!([
                {
                    "twitch_id": "550e8400-e29b-41d4-a716-446655440000",
                    "is_paused": false,
                    "pause_reason": null,
                    "is_deleted": false,
                    "streamer_id": "123456789",
                    "market_item_name": "AWP | Asiimov (Field-Tested)",
                    "twitch_title": "Get AWP Asiimov",
                    "twitch_description": "Redeem for an AWP Asiimov!",
                    "current_market_price": 3500,
                    "permissible_market_price_deviation": 10,
                    "twitch_price_markup_percentage": 150,
                    "global_cooldown_seconds": 60,
                    "max_redemptions_per_stream": 5,
                    "max_redemptions_per_user_per_stream": 1,
                    "market_autobuy": true,
                    "created_at": "2026-01-15T10:30:00Z",
                    "updated_at": "2026-01-20T14:00:00Z"
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
pub async fn list_rewards(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<ListRewardsQuery>,
) -> Result<Json<Vec<RewardResponse>>, ApiError> {
    let rewards = state.db.get_rewards_by_streamer_filtered(
        &auth.channel_id,
        query.is_paused,
        query.is_deleted,
        query.pause_reason,
    ).await?;

    Ok(Json(rewards.into_iter().map(RewardResponse::from).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateRewardBody {
    /// Market item name for automated purchases
    pub market_item_name: String,
    /// Twitch reward title
    pub twitch_title: String,
    /// Twitch reward description
    pub twitch_description: String,
    /// Permissible market price deviation percentage
    pub permissible_market_price_deviation: i32,
    /// Twitch price markup percentage
    pub twitch_price_markup_percentage: i16,
    /// Global cooldown in seconds between redemptions
    pub global_cooldown_seconds: i32,
    /// Maximum redemptions per stream
    pub max_redemptions_per_stream: i16,
    /// Maximum redemptions per user per stream
    pub max_redemptions_per_user_per_stream: i16,
    /// Whether to automatically buy from the market
    pub market_autobuy: bool,
    /// Whether to create the reward as paused
    pub is_paused: bool,
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/rewards",
    tag = "Rewards",
    summary = "Create a reward",
    description = "Creates a new reward for the specified channel.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    request_body = CreateRewardBody,
    responses(
        (status = 201, description = "Reward created successfully", body = RewardResponse,
            example = json!({
                "twitch_id": "550e8400-e29b-41d4-a716-446655440000",
                "is_paused": false,
                "is_deleted": false,
                "streamer_id": "123456789",
                "market_item_name": "AWP | Asiimov (Field-Tested)",
                "twitch_title": "Get AWP Asiimov",
                "twitch_description": "Redeem for an AWP Asiimov!",
                "current_market_price": 3500,
                "permissible_market_price_deviation": 10,
                "twitch_price_markup_percentage": 150,
                "global_cooldown_seconds": 60,
                "max_redemptions_per_stream": 5,
                "max_redemptions_per_user_per_stream": 1,
                "market_autobuy": true,
                "created_at": "2026-01-15T10:30:00Z",
                "updated_at": "2026-01-15T10:30:00Z"
            })
        ),
        (status = 400, description = "Invalid request body (missing or wrong parameter)"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings or Item on the market not found"),
        (status = 422, description = "Validation error (field type mismatch)"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn create_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<CreateRewardBody>,
) -> Result<Json<RewardResponse>, ApiError> {
    if body.twitch_title.trim().is_empty() {
        return Err(ApiError::BadRequest {
            message: "Twitch reward title cannot be empty".into(),
            param: "twitch_title".into(),
        });
    }

    if body.twitch_title.chars().count() > 45 {
        return Err(ApiError::BadRequest {
            message: "The parameter \"title\" was malformed: the value must be less than or equal to 45".into(),
            param: "twitch_title".into(),
        });
    }

    if body.twitch_description.chars().count() > 500 {
        return Err(ApiError::BadRequest {
            message: "Twitch reward description must be 500 characters or less".into(),
            param: "twitch_description".into(),
        });
    }

    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    if setting.market_api_key.trim().is_empty() {
        return Err(ApiError::BadRequest {
            message: "Market API key is not configured for this channel".into(),
            param: "market_api_key".into(),
        });
    }

    let max_per_stream = if body.max_redemptions_per_stream > 0 {
        Some(body.max_redemptions_per_stream as u32)
    } else { None };

    let max_per_user_per_stream = if body.max_redemptions_per_user_per_stream > 0 {
        Some(body.max_redemptions_per_user_per_stream as u32)
    } else { None };

    let global_cooldown_seconds = if body.global_cooldown_seconds > 0 {
        Some(body.global_cooldown_seconds as u32)
    } else { None };

    let items = state.market_client.search_item(&setting.market_api_key, &body.market_item_name).await?;
    if let Some(error) = items.error {
        let msg = format!("Failed to search item through market client: {}", error);
        error!(msg);
        return Err(ApiError::Internal { message: msg })
    } else if !items.success {
        let msg = "Failed to search item through market client";
        error!(msg);
        return Err(ApiError::Internal { message: msg.into() })
    }

    let items_data = items.data.unwrap();

    let cheapest_item = items_data.iter().min_by_key(|item| item.price)
        .ok_or(ApiError::NotFound { message: "Can't find specified item on the market".into() })?;
    let currency = items.currency.clone().unwrap_or_else(|| "RUB".to_string());
    let price_decimal = market::minor_to_major(cheapest_item.price, &currency);

    let markup_factor = 1.0 + (body.twitch_price_markup_percentage as f64 / 100.0).max(0.0);

    let raw_cost = price_decimal
        * markup_factor
        * setting.base_price_multiplier as f64;

    let twitch_points_cost = (raw_cost.ceil() as u32).max(1);

    let mut is_paused = body.is_paused;
    let mut pause_reason = if body.is_paused {
        Some(crate::db::rewards::PauseReason::Manual)
    } else {
        None
    };

    if !is_paused && setting.pause_reward_if_no_money {
        let max_price = (cheapest_item.price as i64)
            + ((cheapest_item.price as i64 * body.permissible_market_price_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, &currency);

        match state.get_cached_or_fetch_balance(&auth.channel_id).await {
            Ok(balance) => {
                if balance.money < cost {
                    tracing::info!(
                        channel_id = %auth.channel_id,
                        balance = balance.money,
                        cost = cost,
                        "Pausing newly created reward because broadcaster balance ({:.2}) is less than reward market cost ({:.2}) and pause_reward_if_no_money is enabled",
                        balance.money,
                        cost
                    );
                    is_paused = true;
                    pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    channel_id = %auth.channel_id,
                    "Failed to retrieve market balance during reward creation"
                );
            }
        }
    }

    let reward_info = CreateCustomReward {
        title: body.twitch_title.clone(),
        cost: twitch_points_cost,
        description: Some(body.twitch_description.clone()),
        background_color: None,
        max_per_stream,
        max_per_user_per_stream,
        global_cooldown_seconds,
    };

    let broadcaster_id = auth.channel_id.clone();
    let bc_ref = broadcaster_id.clone();
    let state_clone = Arc::clone(&state);
    let twitch_reward_info = state.with_broadcaster_token(&bc_ref, move |token| {
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

    let twitch_reward_id = twitch_reward_info.id.parse::<Uuid>()
        .map_err(|_| ApiError::Internal { message: "Failed to parse twitch reward id as Uuid".into() })?;

    if is_paused {
        let broadcaster_id = auth.channel_id.clone();
        let bc_ref = broadcaster_id.clone();
        let state_clone = Arc::clone(&state);
        let reward_id_str = twitch_reward_info.id.clone();
        let update_info = crate::helix::api::custom_rewards::model::UpdateCustomReward {
            is_paused: Some(true),
            ..Default::default()
        };
        if let Err(e) = state.with_broadcaster_token(&bc_ref, move |token| {
            let update_info = update_info.clone();
            let broadcaster_id = broadcaster_id.clone();
            let reward_id_str = reward_id_str.clone();
            let state_clone = Arc::clone(&state_clone);
            async move {
                state_clone.helix_client.update_custom_reward(
                    &broadcaster_id,
                    &reward_id_str,
                    update_info,
                    &token,
                ).await
            }
        }).await {
            tracing::warn!(
                error = %e,
                reward_id = %twitch_reward_info.id,
                "Failed to pause newly created reward on Twitch"
            );
        }
    }

    let new_reward = crate::db::rewards::NewReward {
        twitch_id: twitch_reward_id,
        is_paused,
        pause_reason,
        streamer_id: auth.channel_id.clone(),
        market_item_name: body.market_item_name,
        twitch_title: body.twitch_title,
        twitch_description: body.twitch_description,
        current_market_price: cheapest_item.price as i32,
        permissible_market_price_deviation: body.permissible_market_price_deviation,
        twitch_price_markup_percentage: body.twitch_price_markup_percentage,
        global_cooldown_seconds: body.global_cooldown_seconds,
        max_redemptions_per_stream: body.max_redemptions_per_stream,
        max_redemptions_per_user_per_stream: body.max_redemptions_per_user_per_stream,
        market_autobuy: body.market_autobuy,
        currency,
    };

    let reward = state.db.create_reward(&new_reward).await?;

    tracing::info!(
        reward_id = %reward.twitch_id,
        reward_title = %reward.twitch_title,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        market_item = %reward.market_item_name,
        market_price = reward.current_market_price,
        "Custom reward created successfully"
    );

    Ok(Json(RewardResponse::from(reward)))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateRewardBody {
    /// New Twitch reward title
    pub twitch_title: Option<String>,
    /// New Twitch reward description
    pub twitch_description: Option<String>,
    /// New current market price in cents
    pub current_market_price: Option<i32>,
    /// New permissible market price deviation percentage
    pub permissible_market_price_deviation: Option<i32>,
    /// New Twitch price markup percentage
    pub twitch_price_markup_percentage: Option<i16>,
    /// New global cooldown in seconds
    pub global_cooldown_seconds: Option<i32>,
    /// New maximum redemptions per stream
    pub max_redemptions_per_stream: Option<i16>,
    /// New maximum redemptions per user per stream
    pub max_redemptions_per_user_per_stream: Option<i16>,
    /// New market autobuy flag
    pub market_autobuy: Option<bool>,
    /// New paused status
    pub is_paused: Option<bool>,
    /// New pause reason ("MANUAL", "NO_MONEY")
    pub pause_reason: Option<crate::db::rewards::PauseReason>,
}

#[utoipa::path(
    put,
    path = "/api/v1/broadcasters/{channel_id}/rewards/{reward_id}",
    tag = "Rewards",
    summary = "Update a reward",
    description = "Updates an existing reward. Only provided fields are updated (PATCH semantics).",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("reward_id" = Uuid, Path, description = "Twitch reward UUID"),
    ),
    request_body = UpdateRewardBody,
    responses(
        (status = 200, description = "Reward updated successfully", body = RewardResponse,
            example = json!({
                "twitch_id": "550e8400-e29b-41d4-a716-446655440000",
                "is_paused": false,
                "pause_reason": null,
                "is_deleted": false,
                "streamer_id": "123456789",
                "market_item_name": "AWP | Asiimov (Field-Tested)",
                "twitch_title": "Get AWP Asiimov (Updated)",
                "twitch_description": "Redeem for an AWP Asiimov!",
                "current_market_price": 4000,
                "permissible_market_price_deviation": 10,
                "twitch_price_markup_percentage": 150,
                "global_cooldown_seconds": 60,
                "max_redemptions_per_stream": 5,
                "max_redemptions_per_user_per_stream": 1,
                "market_autobuy": true,
                "created_at": "2026-01-15T10:30:00Z",
                "updated_at": "2026-01-20T14:00:00Z"
            })
        ),
        (status = 400, description = "Invalid request body"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — reward does not belong to this channel"),
        (status = 404, description = "Reward not found"),
        (status = 422, description = "Validation error (field type mismatch)"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn update_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RewardPath>,
    JsonArg(body): JsonArg<UpdateRewardBody>,
) -> Result<Json<RewardResponse>, ApiError> {
    let reward_id = path.reward_id;
    let existing = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: "Reward not found".to_string(),
        })?;

    if existing.streamer_id != auth.channel_id {
        return Err(ApiError::Forbidden {
            message: "Reward does not belong to this channel".to_string(),
        });
    }

    if let Some(ref title) = body.twitch_title {
        if title.trim().is_empty() {
            return Err(ApiError::BadRequest {
                message: "Twitch reward title cannot be empty".into(),
                param: "twitch_title".into(),
            });
        }
        if title.chars().count() > 45 {
            return Err(ApiError::BadRequest {
                message: "The parameter \"title\" was malformed: the value must be less than or equal to 45".into(),
                param: "twitch_title".into(),
            });
        }
    }

    if let Some(ref desc) = body.twitch_description {
        if desc.chars().count() > 500 {
            return Err(ApiError::BadRequest {
                message: "Twitch reward description must be 500 characters or less".into(),
                param: "twitch_description".into(),
            });
        }
    }

    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    let new_cost = if body.twitch_price_markup_percentage.is_some() || body.current_market_price.is_some() {
        let effective_markup = body.twitch_price_markup_percentage.unwrap_or(existing.twitch_price_markup_percentage);
        let effective_price = body.current_market_price.unwrap_or(existing.current_market_price);
        let price_decimal = market::minor_to_major(effective_price as i64, &existing.currency);
        let markup_factor = 1.0 + (effective_markup as f64 / 100.0).max(0.0);
        let raw_cost = price_decimal * markup_factor * setting.base_price_multiplier as f64;
        Some((raw_cost.ceil() as u32).max(1))
    } else {
        None
    };

    let has_twitch_updates = body.twitch_title.is_some()
        || new_cost.is_some()
        || body.twitch_description.is_some()
        || body.max_redemptions_per_stream.is_some()
        || body.max_redemptions_per_user_per_stream.is_some()
        || body.global_cooldown_seconds.is_some()
        || body.is_paused.is_some();

    if has_twitch_updates {
        let update_info = crate::helix::api::custom_rewards::model::UpdateCustomReward {
            title: body.twitch_title.clone(),
            cost: new_cost,
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

    let (target_paused, patch_pause_reason) = match body.is_paused {
        Some(false) => (Some(false), None),
        Some(true) => {
            let reason = if let Some(r) = body.pause_reason {
                Some(r)
            } else if !existing.is_paused {
                Some(crate::db::rewards::PauseReason::Manual)
            } else {
                existing.pause_reason
            };
            (Some(true), reason)
        }
        None => {
            if let Some(r) = body.pause_reason {
                (None, Some(r))
            } else {
                (None, None)
            }
        }
    };

    let patch = crate::db::rewards::UpdateReward {
        is_paused: target_paused,
        pause_reason: patch_pause_reason,
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
        currency: None,
    };

    state.db.update_reward(reward_id, &patch).await?;

    tracing::info!(
        reward_id = %reward_id,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        "Custom reward updated successfully"
    );

    let updated = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::Internal {
            message: "Failed to fetch updated reward".to_string(),
        })?;

    Ok(Json(RewardResponse::from(updated)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/broadcasters/{channel_id}/rewards/{reward_id}",
    tag = "Rewards",
    summary = "Delete a reward",
    description = "Soft-deletes a reward and removes it from Twitch. The reward will be marked as deleted in the database and removed from the Twitch channel point rewards.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("reward_id" = Uuid, Path, description = "Twitch reward UUID"),
    ),
    responses(
        (status = 200, description = "Reward deleted successfully",
            example = json!({ "deleted": true })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — reward does not belong to this channel"),
        (status = 404, description = "Reward not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn delete_reward(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RewardPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let reward_id = path.reward_id;
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
    let twitch_res = state.with_broadcaster_token(&bc_ref, move |token| {
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
    }).await;

    if let Err(e) = twitch_res {
        tracing::warn!(
            reward_id = %reward_id,
            error = %e,
            "Failed to delete reward on Twitch (might already be deleted); proceeding with DB soft-delete"
        );
    }

    state.db.set_reward_deleted(reward_id).await?;

    tracing::info!(
        reward_id = %reward_id,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        "Custom reward deleted from Twitch and DB"
    );

    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/rewards/{reward_id}/update-price",
    tag = "Rewards",
    summary = "Update reward price",
    description = "Triggers a price update for a specific reward. The market price is recalculated and the Twitch reward cost is updated accordingly.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        ("reward_id" = Uuid, Path, description = "Twitch reward UUID"),
    ),
    responses(
        (status = 200, description = "Price updated successfully",
            example = json!({ "updated": true })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — reward does not belong to this channel"),
        (status = 404, description = "Reward not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn update_reward_price(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<RewardPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let reward_id = path.reward_id;
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

    if setting.market_api_key.trim().is_empty() {
        return Err(ApiError::BadRequest {
            message: "Market API key is not configured for this channel".into(),
            param: "market_api_key".into(),
        });
    }

    let items = state.market_client.search_item(&setting.market_api_key, &existing.market_item_name).await?;
    if let Some(error) = items.error {
        let msg = format!("Failed to search item through market client: {}", error);
        error!(msg);
        return Err(ApiError::Internal { message: msg })
    } else if !items.success {
        let msg = "Failed to search item through market client";
        error!(msg);
        return Err(ApiError::Internal { message: msg.into() })
    }

    let items_data = items.data.unwrap();

    let cheapest_item = items_data.iter().min_by_key(|item| item.price)
        .ok_or(ApiError::NotFound { message: "Can't find specified item on the market".into() })?;
    let currency = items.currency.clone().unwrap_or_else(|| existing.currency.clone());
    let price_decimal = market::minor_to_major(cheapest_item.price, &currency);

    let markup_factor = 1.0 + (existing.twitch_price_markup_percentage as f64 / 100.0).max(0.0);

    let raw_cost = price_decimal
        * markup_factor
        * setting.base_price_multiplier as f64;

    let twitch_points_cost = (raw_cost.ceil() as u32).max(1);

    let bc_ref = auth.channel_id.clone();
    let state_clone = Arc::clone(&state);
    let reward_id_str = reward_id.to_string();

    state.with_broadcaster_token(&auth.channel_id, move |token| {
        let b_id = bc_ref.clone();
        let rids = reward_id_str.clone();
        let state_c = Arc::clone(&state_clone);
        async move {
            state_c.helix_client.update_custom_reward(
                &b_id,
                &rids,
                crate::helix::api::custom_rewards::model::UpdateCustomReward {
                    cost: Some(twitch_points_cost),
                    ..Default::default()
                },
                &token,
            ).await
        }
    }).await?;

    state.db.update_reward(reward_id, &crate::db::rewards::UpdateReward {
        current_market_price: Some(cheapest_item.price as i32),
        currency: items.currency,
        ..Default::default()
    }).await?;

    tracing::info!(
        reward_id = %reward_id,
        channel_id = %auth.channel_id,
        new_market_price = cheapest_item.price,
        new_cost = twitch_points_cost,
        "Reward price manually recalculated"
    );

    Ok(Json(serde_json::json!({ "updated": true })))
}

#[derive(Deserialize, ToSchema)]
pub struct BatchRewardBody {
    /// Batch action to perform: "pause", "unpause", or "delete"
    pub action: String,
    /// List of reward UUIDs to apply the action to
    pub reward_ids: Vec<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/rewards/batch",
    tag = "Rewards",
    summary = "Batch operations on rewards",
    description = "Performs a batch operation (pause, unpause, or delete) on multiple rewards at once. Only rewards belonging to the specified channel are affected.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    request_body = BatchRewardBody,
    responses(
        (status = 200, description = "Batch operation completed", body = serde_json::Value,
            example = json!({ "affected": 3 })
        ),
        (status = 400, description = "Invalid request body or unknown action"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn batch_rewards(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<BatchRewardBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut affected = 0u32;
    let broadcaster_id = auth.channel_id.clone();

    for reward_id in &body.reward_ids {
        let existing = match state.db.get_reward_by_twitch_id(*reward_id).await? {
            Some(r) if r.streamer_id == auth.channel_id => r,
            _ => continue,
        };

        let bc_ref = broadcaster_id.clone();
        let reward_id_str = reward_id.to_string();
        let state_clone = Arc::clone(&state);
        let action = body.action.as_str();

        match action {
            "pause" | "unpause" => {
                let target_pause = action == "pause";
                let needs_update = existing.is_paused != target_pause
                    || (target_pause && existing.pause_reason != Some(crate::db::rewards::PauseReason::Manual));

                if needs_update {
                    if existing.is_paused != target_pause {
                        state.with_broadcaster_token(&broadcaster_id, move |token| {
                            let state_c = state_clone.clone();
                            let b_id = bc_ref.clone();
                            let r_id = reward_id_str.clone();
                            async move {
                                state_c.helix_client.update_custom_reward(
                                    &b_id,
                                    &r_id,
                                    crate::helix::api::custom_rewards::model::UpdateCustomReward {
                                        is_paused: Some(target_pause),
                                        ..Default::default()
                                    },
                                    &token,
                                ).await
                            }
                        }).await?;
                    }

                    let reason = if target_pause {
                        Some(crate::db::rewards::PauseReason::Manual)
                    } else {
                        None
                    };

                    state.db.set_reward_paused(*reward_id, target_pause, reason).await?;
                    affected += 1;
                }
            }
            "delete" => {
                state.with_broadcaster_token(&broadcaster_id, move |token| {
                    let state_c = state_clone.clone();
                    let b_id = bc_ref.clone();
                    let r_id = reward_id_str.clone();
                    async move {
                        state_c.helix_client.delete_custom_reward(&b_id, &r_id, &token).await
                    }
                }).await?;

                state.db.set_reward_deleted(*reward_id).await?;
                affected += 1;
            }
            _ => return Err(ApiError::BadRequest {
                message: format!("Unknown batch action: {}", body.action),
                param: "action".to_string(),
            }),
        }
    }

    tracing::info!(
        action = %body.action,
        requested_count = body.reward_ids.len(),
        affected = affected,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        "Batch rewards action completed"
    );

    Ok(Json(serde_json::json!({ "affected": affected })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reward_balance_check_insufficient() {
        let current_market_price = 3500i64; // in cents
        let permissible_deviation = 10i32;
        let currency = "RUB";

        let max_price = current_market_price + ((current_market_price * permissible_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, currency);

        assert_eq!(cost, 38.5);

        let balance = 20.0;
        let pause_reward_if_no_money = true;
        let mut is_paused = false;
        let mut pause_reason = None;

        if !is_paused && pause_reward_if_no_money && balance < cost {
            is_paused = true;
            pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);
        }

        assert!(is_paused);
        assert_eq!(pause_reason, Some(crate::db::rewards::PauseReason::NoMoney));
    }

    #[test]
    fn test_reward_balance_check_sufficient() {
        let current_market_price = 3500i64;
        let permissible_deviation = 10i32;
        let currency = "RUB";

        let max_price = current_market_price + ((current_market_price * permissible_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, currency);

        let balance = 100.0;
        let pause_reward_if_no_money = true;
        let mut is_paused = false;
        let mut pause_reason = None;

        if !is_paused && pause_reward_if_no_money && balance < cost {
            is_paused = true;
            pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);
        }

        assert!(!is_paused);
        assert!(pause_reason.is_none());
    }

    #[test]
    fn test_reward_balance_check_disabled_setting() {
        let current_market_price = 3500i64;
        let permissible_deviation = 10i32;
        let currency = "RUB";

        let max_price = current_market_price + ((current_market_price * permissible_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, currency);

        let balance = 5.0; // insufficient
        let pause_reward_if_no_money = false; // setting disabled
        let mut is_paused = false;
        let mut pause_reason = None;

        if !is_paused && pause_reward_if_no_money && balance < cost {
            is_paused = true;
            pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);
        }

        assert!(!is_paused);
        assert!(pause_reason.is_none());
    }

    #[test]
    fn test_sync_skips_unpausing_manually_paused_reward() {
        let current_market_price = 3500i64;
        let permissible_deviation = 10i32;
        let currency = "RUB";
        let max_price = current_market_price + ((current_market_price * permissible_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, currency);

        let balance = 100.0; // Sufficient balance!
        let has_enough_money = balance >= cost;

        let reward_is_paused = true;
        let reward_pause_reason = Some(crate::db::rewards::PauseReason::Manual);

        let mut target_paused = reward_is_paused;
        let mut target_pause_reason = reward_pause_reason;
        let mut skipped = false;

        if has_enough_money {
            if reward_is_paused {
                if matches!(reward_pause_reason, Some(crate::db::rewards::PauseReason::NoMoney)) {
                    target_paused = false;
                    target_pause_reason = None;
                } else {
                    // Manually paused: skip unpausing
                    skipped = true;
                }
            }
        }

        assert!(skipped);
        assert!(target_paused);
        assert_eq!(target_pause_reason, Some(crate::db::rewards::PauseReason::Manual));
    }

    #[test]
    fn test_sync_unpauses_no_money_paused_reward() {
        let current_market_price = 3500i64;
        let permissible_deviation = 10i32;
        let currency = "RUB";
        let max_price = current_market_price + ((current_market_price * permissible_deviation as i64) / 100);
        let cost = market::minor_to_major(max_price, currency);

        let balance = 100.0; // Sufficient balance!
        let has_enough_money = balance >= cost;

        let reward_is_paused = true;
        let reward_pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);

        let mut target_paused = reward_is_paused;
        let mut target_pause_reason = reward_pause_reason;

        if has_enough_money {
            if reward_is_paused {
                if matches!(reward_pause_reason, Some(crate::db::rewards::PauseReason::NoMoney)) {
                    target_paused = false;
                    target_pause_reason = None;
                }
            }
        }

        assert!(!target_paused);
        assert!(target_pause_reason.is_none());
    }

    #[test]
    fn test_update_reward_manual_pause_reason_transitions() {
        // Transition unpaused -> paused without explicit reason defaults to MANUAL
        let existing_paused = false;
        let existing_reason: Option<crate::db::rewards::PauseReason> = None;
        let body_paused = Some(true);
        let body_reason: Option<crate::db::rewards::PauseReason> = None;

        let (_, patch_reason) = match body_paused {
            Some(false) => (Some(false), None),
            Some(true) => {
                let r = if let Some(r) = body_reason {
                    Some(r)
                } else if !existing_paused {
                    Some(crate::db::rewards::PauseReason::Manual)
                } else {
                    existing_reason
                };
                (Some(true), r)
            }
            None => (None, None),
        };

        assert_eq!(patch_reason, Some(crate::db::rewards::PauseReason::Manual));

        // Unpausing clears pause_reason
        let body_paused = Some(false);
        let (_, patch_reason): (Option<bool>, Option<crate::db::rewards::PauseReason>) = match body_paused {
            Some(false) => (Some(false), None),
            _ => unreachable!(),
        };
        assert!(patch_reason.is_none());
    }
}


