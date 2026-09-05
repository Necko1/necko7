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
    /// Reward type: FIXED, POOL, or FILTER
    pub reward_type: crate::db::rewards::RewardType,
    /// Pricing mode: AUTO or MANUAL
    pub pricing_mode: crate::db::rewards::PricingMode,
    /// Strategy for pricing calculations (AVERAGE, MEDIAN, MAX)
    pub price_strategy: Option<crate::db::rewards::PriceStrategy>,
    /// Market item name for automated purchases (for FIXED rewards)
    pub market_item_name: Option<String>,
    /// Filter configuration (for FILTER rewards)
    pub filter_config: Option<crate::db::rewards::FilterConfig>,
    /// Pool items configuration (for POOL rewards)
    pub pool_items: Option<Vec<crate::db::rewards::PoolItemConfig>>,
    /// Fixed Twitch channel points cost when pricing_mode is MANUAL
    pub manual_twitch_points: Option<i32>,
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
    /// Optional minimum allowed market price in cents
    pub min_market_price: Option<i32>,
    /// Optional maximum allowed market price in cents
    pub max_market_price: Option<i32>,
    /// Optional minimum chat messages required in the time window
    pub chat_min_messages: Option<i32>,
    /// Optional minimum chat characters required in the time window
    pub chat_min_characters: Option<i32>,
    /// Time window in hours to evaluate chat requirements
    pub chat_time_window_hours: Option<i32>,
    /// Logical operator between messages and characters criteria ("AND", "OR")
    pub chat_logical_operator: Option<crate::db::rewards::ChatLogicalOperator>,
    /// Whether channel points are refunded if chat requirements are not met
    pub refund_if_chat_req_failed: bool,
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
            reward_type: r.reward_type,
            pricing_mode: r.pricing_mode,
            price_strategy: r.price_strategy,
            market_item_name: r.market_item_name,
            filter_config: r.filter_config.map(|j| j.0),
            pool_items: r.pool_items.map(|j| j.0),
            manual_twitch_points: r.manual_twitch_points,
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
            min_market_price: r.min_market_price,
            max_market_price: r.max_market_price,
            chat_min_messages: r.chat_min_messages,
            chat_min_characters: r.chat_min_characters,
            chat_time_window_hours: r.chat_time_window_hours,
            chat_logical_operator: r.chat_logical_operator,
            refund_if_chat_req_failed: r.refund_if_chat_req_failed,
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
    /// Reward type: FIXED (default), POOL, or FILTER
    #[serde(default)]
    pub reward_type: crate::db::rewards::RewardType,
    /// Pricing mode: AUTO (default) or MANUAL
    #[serde(default)]
    pub pricing_mode: crate::db::rewards::PricingMode,
    /// Price calculation strategy for AUTO pricing with POOL or FILTER (AVERAGE, MEDIAN, MAX)
    pub price_strategy: Option<crate::db::rewards::PriceStrategy>,
    /// Market item name for FIXED rewards
    pub market_item_name: Option<String>,
    /// Filter configuration for FILTER rewards
    pub filter_config: Option<crate::db::rewards::FilterConfig>,
    /// Pool items configuration for POOL rewards
    pub pool_items: Option<Vec<crate::db::rewards::PoolItemConfig>>,
    /// Fixed Twitch channel points cost when pricing_mode is MANUAL
    pub manual_twitch_points: Option<u32>,
    /// Twitch reward title
    pub twitch_title: String,
    /// Twitch reward description
    pub twitch_description: String,
    /// Permissible market price deviation percentage (used for FIXED / FILTER)
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
    /// Optional minimum allowed market price in cents (auto-pauses reward if below)
    pub min_market_price: Option<i32>,
    /// Optional maximum allowed market price in cents (auto-pauses reward if above)
    pub max_market_price: Option<i32>,
    /// Optional minimum chat messages required in the time window
    pub chat_min_messages: Option<i32>,
    /// Optional minimum chat characters required in the time window
    pub chat_min_characters: Option<i32>,
    /// Time window in hours for chat requirement (default: 24)
    pub chat_time_window_hours: Option<i32>,
    /// Logical operator for chat conditions (AND / OR)
    pub chat_logical_operator: Option<crate::db::rewards::ChatLogicalOperator>,
    /// Whether channel points are refunded if viewer does not meet chat requirements (default: true)
    #[serde(default = "default_true")]
    pub refund_if_chat_req_failed: bool,
}

fn default_true() -> bool {
    true
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/rewards",
    tag = "Rewards",
    summary = "Create a reward",
    description = "Creates a new reward for the specified channel. Supports FIXED, POOL, and FILTER reward types, as well as AUTO or MANUAL pricing modes.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    request_body = CreateRewardBody,
    responses(
        (status = 201, description = "Reward created successfully", body = RewardResponse,
            example = json!({
                "twitch_id": "550e8400-e29b-41d4-a716-446655440000",
                "is_paused": false,
                "pause_reason": null,
                "is_deleted": false,
                "streamer_id": "123456789",
                "reward_type": "FIXED",
                "pricing_mode": "AUTO",
                "price_strategy": null,
                "market_item_name": "AWP | Asiimov (Field-Tested)",
                "filter_config": null,
                "pool_items": null,
                "twitch_title": "Get AWP Asiimov",
                "twitch_description": "Redeem for an AWP Asiimov!",
                "current_market_price": 3500,
                "permissible_market_price_deviation": 10,
                "twitch_price_markup_percentage": 150,
                "global_cooldown_seconds": 60,
                "max_redemptions_per_stream": 5,
                "max_redemptions_per_user_per_stream": 1,
                "market_autobuy": true,
                "currency": "RUB",
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

    if let (Some(min_p), Some(max_p)) = (body.min_market_price, body.max_market_price) {
        if min_p < 0 || max_p < min_p {
            return Err(ApiError::BadRequest {
                message: "min_market_price must be >= 0 and <= max_market_price".into(),
                param: "min_market_price".into(),
            });
        }
    } else if let Some(min_p) = body.min_market_price {
        if min_p < 0 {
            return Err(ApiError::BadRequest {
                message: "min_market_price must be >= 0".into(),
                param: "min_market_price".into(),
            });
        }
    } else if let Some(max_p) = body.max_market_price {
        if max_p < 0 {
            return Err(ApiError::BadRequest {
                message: "max_market_price must be >= 0".into(),
                param: "max_market_price".into(),
            });
        }
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

    let cached_balance = state.get_cached_or_fetch_balance(&auth.channel_id).await.ok();
    let mut currency = cached_balance.as_ref().map(|b| b.currency.clone()).unwrap_or_else(|| "RUB".to_string());

    let reward_type = body.reward_type;
    let pricing_mode = body.pricing_mode;
    let price_strategy = body.price_strategy.unwrap_or(crate::db::rewards::PriceStrategy::Average);

    let (market_item_name, filter_config, pool_items, initial_market_price, max_cost_for_balance_check) = match reward_type {
        crate::db::rewards::RewardType::Fixed => {
            let item_name = match body.market_item_name.as_deref() {
                Some(name) if !name.trim().is_empty() => name.trim().to_string(),
                _ => return Err(ApiError::BadRequest {
                    message: "market_item_name is required for FIXED reward type".into(),
                    param: "market_item_name".into(),
                }),
            };

            let items = state.market_client.search_item(&setting.market_api_key, &item_name).await?;
            if let Some(error) = items.error {
                let msg = format!("Failed to search item through market client: {}", error);
                error!(msg);
                return Err(ApiError::Internal { message: msg });
            } else if !items.success {
                let msg = "Failed to search item through market client";
                error!(msg);
                return Err(ApiError::Internal { message: msg.into() });
            }

            let items_data = items.data.unwrap_or_default();
            let cheapest_item = items_data.iter().min_by_key(|item| item.price)
                .ok_or(ApiError::NotFound { message: "Can't find specified item on the market".into() })?;

            if let Some(c) = items.currency {
                currency = c;
            }

            let price_major = market::minor_to_major(cheapest_item.price, &currency);
            let max_cost = price_major * (1.0 + (body.permissible_market_price_deviation as f64 / 100.0).max(0.0));

            (Some(item_name), None, None, cheapest_item.price as i32, max_cost)
        }
        crate::db::rewards::RewardType::Pool => {
            let mut pool = match body.pool_items {
                Some(items) if !items.is_empty() => items,
                _ => return Err(ApiError::BadRequest {
                    message: "pool_items cannot be empty for POOL reward type".into(),
                    param: "pool_items".into(),
                }),
            };

            for item in &pool {
                if item.market_hash_name.trim().is_empty() {
                    return Err(ApiError::BadRequest {
                        message: "pool item market_hash_name cannot be empty".into(),
                        param: "pool_items".into(),
                    });
                }
                if item.weight <= 0.0 || !item.weight.is_finite() {
                    return Err(ApiError::BadRequest {
                        message: "pool item weight must be a positive finite number".into(),
                        param: "pool_items".into(),
                    });
                }
            }

            let all_prices = state.get_cached_or_fetch_prices(&currency).await
                .map_err(|e| ApiError::Internal { message: format!("Failed to fetch market prices: {}", e) })?;

            let price_map: std::collections::HashMap<&str, f64> = all_prices
                .iter()
                .map(|i| (i.market_hash_name.as_str(), i.price))
                .collect();

            let mut price_weight_pairs: Vec<(f64, f64)> = Vec::with_capacity(pool.len());
            let mut prices_vec: Vec<f64> = Vec::with_capacity(pool.len());
            let mut max_single_cost = 0.0f64;

            for item in &mut pool {
                if let Some(&p_major) = price_map.get(item.market_hash_name.as_str()) {
                    item.current_market_price = market::major_to_minor(p_major, &currency) as i32;
                }
                let p_major = market::minor_to_major(item.current_market_price as i64, &currency);
                prices_vec.push(p_major);
                price_weight_pairs.push((p_major, item.weight));
                let item_max_cost = p_major * (1.0 + (item.permissible_market_price_deviation as f64 / 100.0).max(0.0));
                if item_max_cost > max_single_cost {
                    max_single_cost = item_max_cost;
                }
            }

            let effective_price_major = match price_strategy {
                crate::db::rewards::PriceStrategy::Average => {
                    crate::steam::market::prices::calculate_weighted_average(&price_weight_pairs).unwrap_or(0.0)
                }
                crate::db::rewards::PriceStrategy::Median => {
                    crate::steam::market::prices::calculate_median(&mut prices_vec).unwrap_or(0.0)
                }
                crate::db::rewards::PriceStrategy::Max => {
                    crate::steam::market::prices::calculate_max(&prices_vec).unwrap_or(0.0)
                }
            };

            if effective_price_major <= 0.0 {
                return Err(ApiError::BadRequest {
                    message: "Effective price for pool items is zero or negative; check item prices".into(),
                    param: "pool_items".into(),
                });
            }

            let market_price = market::major_to_minor(effective_price_major, &currency) as i32;
            (None, None, Some(sqlx::types::Json(pool)), market_price, max_single_cost)
        }
        crate::db::rewards::RewardType::Filter => {
            let filter = match body.filter_config {
                Some(f) => f,
                None => return Err(ApiError::BadRequest {
                    message: "filter_config is required for FILTER reward type".into(),
                    param: "filter_config".into(),
                }),
            };

            if filter.min_price < 0.0 || filter.max_price < filter.min_price {
                return Err(ApiError::BadRequest {
                    message: "Invalid filter price range: min_price must be >= 0 and max_price >= min_price".into(),
                    param: "filter_config".into(),
                });
            }

            let all_prices = state.get_cached_or_fetch_prices(&currency).await
                .map_err(|e| ApiError::Internal { message: format!("Failed to fetch market prices: {}", e) })?;

            let matching = crate::steam::market::prices::filter_prices(&all_prices, &filter);
            if matching.is_empty() {
                return Err(ApiError::BadRequest {
                    message: "No market items match the specified filter criteria".into(),
                    param: "filter_config".into(),
                });
            }

            let mut prices: Vec<f64> = matching.iter().map(|i| i.price).collect();
            let effective_price_major = match price_strategy {
                crate::db::rewards::PriceStrategy::Average => {
                    crate::steam::market::prices::calculate_average(&prices).unwrap_or(filter.max_price)
                }
                crate::db::rewards::PriceStrategy::Median => {
                    crate::steam::market::prices::calculate_median(&mut prices).unwrap_or(filter.max_price)
                }
                crate::db::rewards::PriceStrategy::Max => {
                    crate::steam::market::prices::calculate_max(&prices).unwrap_or(filter.max_price)
                }
            };

            let market_price = market::major_to_minor(effective_price_major, &currency) as i32;
            let max_cost = filter.max_price * (1.0 + (body.permissible_market_price_deviation as f64 / 100.0).max(0.0));
            (None, Some(sqlx::types::Json(filter)), None, market_price, max_cost)
        }
    };

    let twitch_points_cost: u32 = match pricing_mode {
        crate::db::rewards::PricingMode::Manual => {
            let pts = body.manual_twitch_points.ok_or_else(|| ApiError::BadRequest {
                message: "manual_twitch_points is required when pricing_mode is MANUAL".into(),
                param: "manual_twitch_points".into(),
            })?;
            if pts == 0 {
                return Err(ApiError::BadRequest {
                    message: "manual_twitch_points must be at least 1".into(),
                    param: "manual_twitch_points".into(),
                });
            }
            pts
        }
        crate::db::rewards::PricingMode::Auto => {
            let price_decimal = market::minor_to_major(initial_market_price as i64, &currency);
            let markup_factor = 1.0 + (body.twitch_price_markup_percentage as f64 / 100.0).max(0.0);
            let raw_cost = price_decimal * markup_factor * setting.base_price_multiplier as f64;
            (raw_cost.ceil() as u32).max(1)
        }
    };

    let mut is_paused = body.is_paused;
    let mut pause_reason = if body.is_paused {
        Some(crate::db::rewards::PauseReason::Manual)
    } else {
        None
    };

    if !is_paused && setting.pause_reward_if_no_money {
        if let Some(balance) = cached_balance {
            if balance.money < max_cost_for_balance_check {
                tracing::info!(
                    channel_id = %auth.channel_id,
                    balance = balance.money,
                    cost = max_cost_for_balance_check,
                    "Pausing newly created reward because broadcaster balance ({:.2}) is less than reward market cost ({:.2}) and pause_reward_if_no_money is enabled",
                    balance.money,
                    max_cost_for_balance_check
                );
                is_paused = true;
                pause_reason = Some(crate::db::rewards::PauseReason::NoMoney);
            }
        }
    }

    if !is_paused {
        if let Some(min_p) = body.min_market_price {
            if initial_market_price < min_p {
                tracing::info!(
                    channel_id = %auth.channel_id,
                    price = initial_market_price,
                    min = min_p,
                    "Pausing newly created reward because initial market price is below min_market_price"
                );
                is_paused = true;
                pause_reason = Some(crate::db::rewards::PauseReason::PriceLimit);
            }
        }
        if let Some(max_p) = body.max_market_price {
            if initial_market_price > max_p {
                tracing::info!(
                    channel_id = %auth.channel_id,
                    price = initial_market_price,
                    max = max_p,
                    "Pausing newly created reward because initial market price exceeds max_market_price"
                );
                is_paused = true;
                pause_reason = Some(crate::db::rewards::PauseReason::PriceLimit);
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
        reward_type,
        pricing_mode,
        price_strategy: Some(price_strategy),
        market_item_name,
        filter_config,
        pool_items,
        manual_twitch_points: body.manual_twitch_points.map(|v| v as i32),
        twitch_title: body.twitch_title,
        twitch_description: body.twitch_description,
        current_market_price: initial_market_price,
        permissible_market_price_deviation: body.permissible_market_price_deviation,
        twitch_price_markup_percentage: body.twitch_price_markup_percentage,
        global_cooldown_seconds: body.global_cooldown_seconds,
        max_redemptions_per_stream: body.max_redemptions_per_stream,
        max_redemptions_per_user_per_stream: body.max_redemptions_per_user_per_stream,
        market_autobuy: body.market_autobuy,
        currency,
        min_market_price: body.min_market_price,
        max_market_price: body.max_market_price,
        chat_min_messages: body.chat_min_messages,
        chat_min_characters: body.chat_min_characters,
        chat_time_window_hours: body.chat_time_window_hours,
        chat_logical_operator: body.chat_logical_operator,
        refund_if_chat_req_failed: body.refund_if_chat_req_failed,
    };

    let reward = state.db.create_reward(&new_reward).await?;

    tracing::info!(
        reward_id = %reward.twitch_id,
        reward_title = %reward.twitch_title,
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        reward_type = ?reward.reward_type,
        pricing_mode = ?reward.pricing_mode,
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
    /// New reward type (FIXED, POOL, or FILTER)
    pub reward_type: Option<crate::db::rewards::RewardType>,
    /// New pricing mode (AUTO or MANUAL)
    pub pricing_mode: Option<crate::db::rewards::PricingMode>,
    /// New price strategy (AVERAGE, MEDIAN, MAX)
    pub price_strategy: Option<crate::db::rewards::PriceStrategy>,
    /// New market item name (for FIXED rewards)
    pub market_item_name: Option<String>,
    /// New filter configuration (for FILTER rewards)
    pub filter_config: Option<crate::db::rewards::FilterConfig>,
    /// New pool items configuration (for POOL rewards)
    pub pool_items: Option<Vec<crate::db::rewards::PoolItemConfig>>,
    /// Manual Twitch channel points cost (used when pricing_mode is MANUAL)
    pub manual_twitch_points: Option<u32>,
    /// Optional minimum allowed market price in cents (auto-pauses reward if below)
    pub min_market_price: Option<i32>,
    /// Optional maximum allowed market price in cents (auto-pauses reward if above)
    pub max_market_price: Option<i32>,
    /// Optional minimum chat messages required in the time window
    pub chat_min_messages: Option<i32>,
    /// Optional minimum chat characters required in the time window
    pub chat_min_characters: Option<i32>,
    /// Time window in hours for chat requirement
    pub chat_time_window_hours: Option<i32>,
    /// Logical operator for chat conditions (AND / OR)
    pub chat_logical_operator: Option<crate::db::rewards::ChatLogicalOperator>,
    /// Whether channel points are refunded if viewer does not meet chat requirements
    pub refund_if_chat_req_failed: Option<bool>,
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
                "reward_type": "FIXED",
                "pricing_mode": "AUTO",
                "price_strategy": null,
                "market_item_name": "AWP | Asiimov (Field-Tested)",
                "filter_config": null,
                "pool_items": null,
                "twitch_title": "Get AWP Asiimov (Updated)",
                "twitch_description": "Redeem for an AWP Asiimov!",
                "current_market_price": 4000,
                "permissible_market_price_deviation": 10,
                "twitch_price_markup_percentage": 150,
                "global_cooldown_seconds": 60,
                "max_redemptions_per_stream": 5,
                "max_redemptions_per_user_per_stream": 1,
                "market_autobuy": true,
                "currency": "RUB",
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

    if let Some(ref pool) = body.pool_items {
        if pool.is_empty() {
            return Err(ApiError::BadRequest {
                message: "pool_items cannot be empty".into(),
                param: "pool_items".into(),
            });
        }
        for item in pool {
            if item.market_hash_name.trim().is_empty() {
                return Err(ApiError::BadRequest {
                    message: "pool item market_hash_name cannot be empty".into(),
                    param: "pool_items".into(),
                });
            }
            if item.weight <= 0.0 || !item.weight.is_finite() {
                return Err(ApiError::BadRequest {
                    message: "pool item weight must be a positive finite number".into(),
                    param: "pool_items".into(),
                });
            }
        }
    }

    let effective_min = body.min_market_price.or(existing.min_market_price);
    let effective_max = body.max_market_price.or(existing.max_market_price);
    if let (Some(min_p), Some(max_p)) = (effective_min, effective_max) {
        if min_p < 0 || max_p < min_p {
            return Err(ApiError::BadRequest {
                message: "min_market_price must be >= 0 and <= max_market_price".into(),
                param: "min_market_price".into(),
            });
        }
    } else if let Some(min_p) = effective_min {
        if min_p < 0 {
            return Err(ApiError::BadRequest {
                message: "min_market_price must be >= 0".into(),
                param: "min_market_price".into(),
            });
        }
    } else if let Some(max_p) = effective_max {
        if max_p < 0 {
            return Err(ApiError::BadRequest {
                message: "max_market_price must be >= 0".into(),
                param: "max_market_price".into(),
            });
        }
    }

    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;
    let effective_pricing_mode = body.pricing_mode.unwrap_or(existing.pricing_mode);

    let new_cost = if let Some(manual_pts) = body.manual_twitch_points {
        if effective_pricing_mode == crate::db::rewards::PricingMode::Manual {
            Some(manual_pts.max(1))
        } else {
            None
        }
    } else if effective_pricing_mode == crate::db::rewards::PricingMode::Auto && (body.twitch_price_markup_percentage.is_some() || body.current_market_price.is_some()) {
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
        reward_type: body.reward_type,
        pricing_mode: body.pricing_mode,
        price_strategy: body.price_strategy,
        market_item_name: body.market_item_name,
        filter_config: body.filter_config.map(sqlx::types::Json),
        pool_items: body.pool_items.map(sqlx::types::Json),
        manual_twitch_points: body.manual_twitch_points.map(|v| v as i32),
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
        min_market_price: body.min_market_price,
        max_market_price: body.max_market_price,
        chat_min_messages: body.chat_min_messages,
        chat_min_characters: body.chat_min_characters,
        chat_time_window_hours: body.chat_time_window_hours,
        chat_logical_operator: body.chat_logical_operator,
        refund_if_chat_req_failed: body.refund_if_chat_req_failed,
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

    if body.min_market_price.is_some() || body.max_market_price.is_some() || body.current_market_price.is_some() {
        let state_clone = state.clone();
        let updated_clone = updated.clone();
        state.spawn_task(async move {
            let _ = crate::processor::price_updater::check_and_sync_price_limits(
                &state_clone,
                &updated_clone,
                updated_clone.current_market_price,
            ).await;
        });
    }

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
    description = "Triggers a price update for a specific reward. The market price is recalculated and the Twitch reward cost is updated accordingly based on reward type and strategy.",
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

    crate::processor::price_updater::update_single_reward_price(&state, &auth.channel_id, reward_id).await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?;

    tracing::info!(
        reward_id = %reward_id,
        channel_id = %auth.channel_id,
        "Reward price manually recalculated"
    );

    Ok(Json(serde_json::json!({ "updated": true })))
}

#[derive(Deserialize, ToSchema)]
pub struct PreviewFilterBody {
    /// Filter configuration to test
    pub filter_config: crate::db::rewards::FilterConfig,
    /// Strategy to calculate aggregate market price (AVERAGE, MEDIAN, MAX). Defaults to AVERAGE.
    pub price_strategy: Option<crate::db::rewards::PriceStrategy>,
    /// Optional Twitch markup percentage to estimate Channel Points cost
    pub twitch_price_markup_percentage: Option<i16>,
    /// Optional currency code (e.g. "RUB", "USD"). If not provided, channel currency is used.
    pub currency: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PreviewFilterResponse {
    /// Number of market items matching filter
    pub total_matching_items: usize,
    /// Minimum market price among matching items
    pub min_price: f64,
    /// Maximum market price among matching items
    pub max_price: f64,
    /// Average market price among matching items
    pub average_price: f64,
    /// Median market price among matching items
    pub median_price: f64,
    /// Calculated market price in major currency units according to strategy
    pub calculated_market_price: f64,
    /// Estimated Twitch channel points cost
    pub estimated_twitch_points: u32,
    /// Currency code (e.g. "RUB")
    pub currency: String,
    /// Sample matching items (up to 50)
    pub sample_items: Vec<crate::steam::market::prices::MarketPriceItem>,
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/rewards/preview-filter",
    tag = "Rewards",
    summary = "Preview filter reward items and price",
    description = "Tests a filter configuration against market prices, returning the total count of matching skins, sample items, and estimated Twitch channel points cost.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    request_body = PreviewFilterBody,
    responses(
        (status = 200, description = "Preview calculated successfully", body = PreviewFilterResponse),
        (status = 400, description = "Invalid request or filter parameters"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn preview_filter(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<PreviewFilterBody>,
) -> Result<Json<PreviewFilterResponse>, ApiError> {
    if body.filter_config.min_price < 0.0 || body.filter_config.max_price < body.filter_config.min_price {
        return Err(ApiError::BadRequest {
            message: "Invalid filter price range: min_price must be >= 0 and max_price >= min_price".into(),
            param: "filter_config".into(),
        });
    }

    let setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;
    let cached_balance = state.get_cached_or_fetch_balance(&auth.channel_id).await.ok();
    let currency = body.currency
        .filter(|c| !c.trim().is_empty())
        .or_else(|| cached_balance.map(|b| b.currency))
        .unwrap_or_else(|| "RUB".to_string());

    let all_prices = state.get_cached_or_fetch_prices(&currency).await
        .map_err(|e| ApiError::Internal { message: format!("Failed to fetch market prices: {}", e) })?;

    let matching = crate::steam::market::prices::filter_prices(&all_prices, &body.filter_config);
    let total_matching_items = matching.len();

    let strategy = body.price_strategy.unwrap_or(crate::db::rewards::PriceStrategy::Average);
    let prices: Vec<f64> = matching.iter().map(|i| i.price).collect();

    let min_price = prices.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0.0);
    let max_price = crate::steam::market::prices::calculate_max(&prices).unwrap_or(0.0);
    let average_price = crate::steam::market::prices::calculate_average(&prices).unwrap_or(0.0);
    let median_price = crate::steam::market::prices::calculate_median(&mut prices.clone()).unwrap_or(0.0);

    let calculated_market_price = match strategy {
        crate::db::rewards::PriceStrategy::Average => average_price,
        crate::db::rewards::PriceStrategy::Median => median_price,
        crate::db::rewards::PriceStrategy::Max => max_price,
    };

    let markup_factor = 1.0 + (body.twitch_price_markup_percentage.unwrap_or(0) as f64 / 100.0).max(0.0);
    let raw_cost = calculated_market_price * markup_factor * setting.base_price_multiplier as f64;
    let estimated_twitch_points = (raw_cost.ceil() as u32).max(1);

    let sample_items: Vec<crate::steam::market::prices::MarketPriceItem> = matching.into_iter().take(50).collect();

    Ok(Json(PreviewFilterResponse {
        total_matching_items,
        min_price,
        max_price,
        average_price,
        median_price,
        calculated_market_price,
        estimated_twitch_points,
        currency,
        sample_items,
    }))
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

    #[test]
    fn test_create_reward_body_defaults() {
        let json_str = r#"{
            "twitch_title": "Test Reward",
            "twitch_description": "Desc",
            "permissible_market_price_deviation": 10,
            "twitch_price_markup_percentage": 100,
            "global_cooldown_seconds": 0,
            "max_redemptions_per_stream": 0,
            "max_redemptions_per_user_per_stream": 0,
            "market_autobuy": true,
            "is_paused": false
        }"#;

        let parsed: CreateRewardBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.reward_type, crate::db::rewards::RewardType::Fixed);
        assert_eq!(parsed.pricing_mode, crate::db::rewards::PricingMode::Auto);
        assert!(parsed.price_strategy.is_none());
        assert!(parsed.filter_config.is_none());
        assert!(parsed.pool_items.is_none());
    }

    #[test]
    fn test_create_reward_body_pool_and_manual() {
        let json_str = r#"{
            "twitch_title": "Pool Reward",
            "twitch_description": "Desc",
            "reward_type": "POOL",
            "pricing_mode": "MANUAL",
            "manual_twitch_points": 5000,
            "pool_items": [
                {
                    "market_hash_name": "AK-47 | Redline (Field-Tested)",
                    "weight": 80.0,
                    "permissible_market_price_deviation": 10
                },
                {
                    "market_hash_name": "AWP | Asiimov (Field-Tested)",
                    "weight": 20.0,
                    "permissible_market_price_deviation": 5
                }
            ],
            "permissible_market_price_deviation": 10,
            "twitch_price_markup_percentage": 50,
            "global_cooldown_seconds": 0,
            "max_redemptions_per_stream": 0,
            "max_redemptions_per_user_per_stream": 0,
            "market_autobuy": true,
            "is_paused": false
        }"#;

        let parsed: CreateRewardBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.reward_type, crate::db::rewards::RewardType::Pool);
        assert_eq!(parsed.pricing_mode, crate::db::rewards::PricingMode::Manual);
        assert_eq!(parsed.manual_twitch_points, Some(5000));
        assert_eq!(parsed.pool_items.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_create_reward_body_filter() {
        let json_str = r#"{
            "twitch_title": "Random Pistol",
            "twitch_description": "Desc",
            "reward_type": "FILTER",
            "pricing_mode": "AUTO",
            "price_strategy": "MEDIAN",
            "filter_config": {
                "min_price": 10.0,
                "max_price": 50.0,
                "name_contains": "Glock-18",
                "min_volume": 100
            },
            "permissible_market_price_deviation": 15,
            "twitch_price_markup_percentage": 20,
            "global_cooldown_seconds": 0,
            "max_redemptions_per_stream": 0,
            "max_redemptions_per_user_per_stream": 0,
            "market_autobuy": true,
            "is_paused": false
        }"#;

        let parsed: CreateRewardBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.reward_type, crate::db::rewards::RewardType::Filter);
        assert_eq!(parsed.price_strategy, Some(crate::db::rewards::PriceStrategy::Median));
        let filter = parsed.filter_config.unwrap();
        assert_eq!(filter.min_price, 10.0);
        assert_eq!(filter.max_price, 50.0);
        assert_eq!(filter.name_contains.as_deref(), Some("Glock-18"));
        assert_eq!(filter.min_volume, Some(100));
    }

    #[test]
    fn test_update_reward_body_deserialization() {
        let json_str = r#"{
            "reward_type": "FILTER",
            "pricing_mode": "MANUAL",
            "manual_twitch_points": 12000,
            "price_strategy": "MAX",
            "filter_config": {
                "min_price": 5.0,
                "max_price": 25.0
            }
        }"#;

        let parsed: UpdateRewardBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.reward_type, Some(crate::db::rewards::RewardType::Filter));
        assert_eq!(parsed.pricing_mode, Some(crate::db::rewards::PricingMode::Manual));
        assert_eq!(parsed.manual_twitch_points, Some(12000));
        assert_eq!(parsed.price_strategy, Some(crate::db::rewards::PriceStrategy::Max));
        assert!(parsed.filter_config.is_some());
    }

    #[test]
    fn test_create_and_update_reward_body_with_price_limits() {
        let json_create = r#"{
            "twitch_title": "Fixed Skin",
            "twitch_description": "Desc",
            "market_item_name": "AK-47 | Redline (Field-Tested)",
            "reward_type": "FIXED",
            "pricing_mode": "MANUAL",
            "manual_twitch_points": 50000,
            "min_market_price": 1000,
            "max_market_price": 5000,
            "permissible_market_price_deviation": 10,
            "twitch_price_markup_percentage": 0,
            "global_cooldown_seconds": 0,
            "max_redemptions_per_stream": 0,
            "max_redemptions_per_user_per_stream": 0,
            "market_autobuy": true,
            "is_paused": false
        }"#;

        let parsed_create: CreateRewardBody = serde_json::from_str(json_create).unwrap();
        assert_eq!(parsed_create.min_market_price, Some(1000));
        assert_eq!(parsed_create.max_market_price, Some(5000));

        let json_update = r#"{
            "min_market_price": 1500,
            "max_market_price": 6000,
            "pause_reason": "PRICE_LIMIT"
        }"#;
        let parsed_update: UpdateRewardBody = serde_json::from_str(json_update).unwrap();
        assert_eq!(parsed_update.min_market_price, Some(1500));
        assert_eq!(parsed_update.max_market_price, Some(6000));
        assert_eq!(parsed_update.pause_reason, Some(crate::db::rewards::PauseReason::PriceLimit));
    }

    #[test]
    fn test_create_and_update_reward_body_with_chat_requirements() {
        let json_create = r#"{
            "twitch_title": "VIP Skin",
            "twitch_description": "Requires active chatters",
            "market_item_name": "AK-47 | Redline (Field-Tested)",
            "reward_type": "FIXED",
            "pricing_mode": "MANUAL",
            "manual_twitch_points": 50000,
            "chat_min_messages": 50,
            "chat_min_characters": 500,
            "chat_time_window_hours": 72,
            "chat_logical_operator": "OR",
            "refund_if_chat_req_failed": false,
            "permissible_market_price_deviation": 10,
            "twitch_price_markup_percentage": 0,
            "global_cooldown_seconds": 0,
            "max_redemptions_per_stream": 0,
            "max_redemptions_per_user_per_stream": 0,
            "market_autobuy": true,
            "is_paused": false
        }"#;

        let parsed_create: CreateRewardBody = serde_json::from_str(json_create).unwrap();
        assert_eq!(parsed_create.chat_min_messages, Some(50));
        assert_eq!(parsed_create.chat_min_characters, Some(500));
        assert_eq!(parsed_create.chat_time_window_hours, Some(72));
        assert_eq!(parsed_create.chat_logical_operator, Some(crate::db::rewards::ChatLogicalOperator::Or));
        assert_eq!(parsed_create.refund_if_chat_req_failed, false);

        let json_update = r#"{
            "chat_min_messages": 100,
            "chat_logical_operator": "AND",
            "refund_if_chat_req_failed": true
        }"#;
        let parsed_update: UpdateRewardBody = serde_json::from_str(json_update).unwrap();
        assert_eq!(parsed_update.chat_min_messages, Some(100));
        assert_eq!(parsed_update.chat_logical_operator, Some(crate::db::rewards::ChatLogicalOperator::And));
        assert_eq!(parsed_update.refund_if_chat_req_failed, Some(true));
    }

    #[test]
    fn test_reward_response_includes_manual_twitch_points() {
        let reward = crate::db::rewards::Reward {
            twitch_id: uuid::Uuid::new_v4(),
            is_paused: false,
            pause_reason: None,
            is_deleted: false,
            streamer_id: "12345".to_string(),
            reward_type: crate::db::rewards::RewardType::Fixed,
            pricing_mode: crate::db::rewards::PricingMode::Manual,
            price_strategy: None,
            market_item_name: Some("AK-47 | Redline (Field-Tested)".to_string()),
            filter_config: None,
            pool_items: None,
            manual_twitch_points: Some(50000),
            twitch_title: "Manual Reward".to_string(),
            twitch_description: "Description".to_string(),
            current_market_price: 1500,
            permissible_market_price_deviation: 10,
            twitch_price_markup_percentage: 0,
            global_cooldown_seconds: 0,
            max_redemptions_per_stream: 0,
            max_redemptions_per_user_per_stream: 0,
            market_autobuy: true,
            currency: "USD".to_string(),
            min_market_price: None,
            max_market_price: None,
            chat_min_messages: None,
            chat_min_characters: None,
            chat_time_window_hours: None,
            chat_logical_operator: None,
            refund_if_chat_req_failed: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let res = RewardResponse::from(reward);
        assert_eq!(res.manual_twitch_points, Some(50000));
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains("\"manual_twitch_points\":50000"));
    }
}


