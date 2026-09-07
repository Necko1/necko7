use std::sync::Arc;
use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct PublicBroadcasterInfo {
    /// Twitch channel ID
    pub channel_id: String,
    /// Twitch channel login
    pub channel_login: String,
    /// Twitch channel display name
    pub display_name: Option<String>,
    /// Twitch channel avatar URL
    pub profile_image_url: Option<String>,
    /// Whether public rewards viewing is enabled on this channel
    pub public_rewards_enabled: bool,
}

#[derive(Serialize, ToSchema)]
pub struct PublicRewardResponse {
    /// Twitch reward UUID
    pub twitch_id: Uuid,
    /// Twitch reward title
    pub twitch_title: String,
    /// Twitch reward description (if permitted by broadcaster)
    pub twitch_description: Option<String>,
    /// Reward type (FIXED, POOL, FILTER)
    pub reward_type: crate::db::rewards::RewardType,
    /// Pricing mode (AUTO, MANUAL)
    pub pricing_mode: crate::db::rewards::PricingMode,
    /// Whether reward is currently paused
    pub is_paused: bool,
    /// Reason why reward is paused (if permitted by broadcaster)
    pub pause_reason: Option<String>,
    /// Cost in Twitch channel points (if permitted by broadcaster)
    pub cost_points: Option<i32>,
    /// Global cooldown in seconds (if permitted by broadcaster)
    pub global_cooldown_seconds: Option<i32>,
    /// Max redemptions per stream (if permitted by broadcaster)
    pub max_redemptions_per_stream: Option<i16>,
    /// Max redemptions per user per stream (if permitted by broadcaster)
    pub max_redemptions_per_user_per_stream: Option<i16>,
    /// Item market price in major currency (e.g. RUB/USD) (if permitted by broadcaster)
    pub market_price: Option<f64>,
    /// Permissible market price deviation percentage (if permitted by broadcaster)
    pub permissible_market_price_deviation: Option<i32>,
    /// Currency code
    pub currency: Option<String>,
    /// Market item name (for FIXED rewards)
    pub market_item_name: Option<String>,
    /// Items in the pool with chances and prices (for POOL rewards)
    pub pool_items: Option<Vec<PublicPoolItem>>,
    /// Filter rules (for FILTER rewards)
    pub filter_details: Option<PublicFilterDetails>,
    /// Chat activity requirements (if permitted by broadcaster)
    pub chat_requirements: Option<PublicChatRequirements>,
    /// Purchase limits config (if permitted by broadcaster)
    pub purchase_limits: Option<crate::db::rewards::RewardPurchaseLimitsConfig>,
}

#[derive(Serialize, ToSchema)]
pub struct PublicPoolItem {
    pub market_hash_name: String,
    /// Drop chance percentage (0.00% - 100.00%)
    pub chance_percentage: Option<f64>,
    /// Market price of this pool item in major currency
    pub current_market_price: Option<f64>,
    /// Permissible market price deviation percentage (if permitted by broadcaster)
    pub permissible_market_price_deviation: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct PublicFilterDetails {
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub name_prefix: Option<String>,
    pub name_suffix: Option<String>,
    pub name_contains: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PublicChatRequirements {
    pub min_messages: Option<i32>,
    pub min_characters: Option<i32>,
    pub time_window_hours: Option<i32>,
    pub logical_operator: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/public/broadcasters/{identifier}",
    tag = "Public",
    summary = "Get public broadcaster info",
    description = "Returns public info about a broadcaster by channel ID or channel login name. No authentication required.",
    params(
        ("identifier" = String, Path, description = "Twitch channel ID or channel login"),
    ),
    responses(
        (status = 200, description = "Broadcaster info", body = PublicBroadcasterInfo),
        (status = 404, description = "Channel not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_public_broadcaster_info(
    Path(identifier): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicBroadcasterInfo>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&identifier).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Channel '{}' not found", identifier),
        })?;

    let setting = state.db.get_or_create_broadcaster_setting(&broadcaster.channel_id).await?;
    let twitch_user = state.get_twitch_user_cached(&broadcaster.channel_id).await;

    let (display_name, profile_image_url) = if let Some(ref u) = twitch_user {
        (Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
    } else {
        (None, None)
    };

    Ok(Json(PublicBroadcasterInfo {
        channel_id: broadcaster.channel_id,
        channel_login: broadcaster.channel_login,
        display_name,
        profile_image_url,
        public_rewards_enabled: setting.public_rewards_config().enabled,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/public/broadcasters/{identifier}/rewards",
    tag = "Public",
    summary = "Get public reward catalog",
    description = "Returns the public rewards catalog of a broadcaster, filtered by streamer's visibility settings. Accessible without authentication.",
    params(
        ("identifier" = String, Path, description = "Twitch channel ID or channel login"),
    ),
    responses(
        (status = 200, description = "List of public rewards", body = Vec<PublicRewardResponse>),
        (status = 403, description = "Public rewards catalog is disabled by the broadcaster"),
        (status = 404, description = "Channel not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_public_rewards(
    Path(identifier): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PublicRewardResponse>>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&identifier).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Channel '{}' not found", identifier),
        })?;

    let setting = state.db.get_or_create_broadcaster_setting(&broadcaster.channel_id).await?;
    let cfg = setting.public_rewards_config();

    if !cfg.enabled {
        return Err(ApiError::Forbidden {
            message: "Public rewards catalog is disabled on this channel".to_string(),
        });
    }

    let rewards = state.db.get_public_rewards_by_streamer_id(&broadcaster.channel_id).await?;
    let result = rewards
        .into_iter()
        .filter(|r| cfg.show_paused_rewards || !r.is_paused)
        .map(|r| build_public_reward_response(&r, &cfg, setting.base_price_multiplier))
        .collect();

    Ok(Json(result))
}

#[utoipa::path(
    get,
    path = "/api/v1/public/broadcasters/{identifier}/rewards/{reward_id}",
    tag = "Public",
    summary = "Get single public reward details",
    description = "Returns detailed view of a specific public reward. Accessible without authentication.",
    params(
        ("identifier" = String, Path, description = "Twitch channel ID or channel login"),
        ("reward_id" = Uuid, Path, description = "Twitch reward UUID"),
    ),
    responses(
        (status = 200, description = "Public reward details", body = PublicRewardResponse),
        (status = 403, description = "Public rewards catalog is disabled or reward is not public"),
        (status = 404, description = "Channel or reward not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_public_reward_by_id(
    Path((identifier, reward_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicRewardResponse>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&identifier).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Channel '{}' not found", identifier),
        })?;

    let setting = state.db.get_or_create_broadcaster_setting(&broadcaster.channel_id).await?;
    let cfg = setting.public_rewards_config();

    if !cfg.enabled {
        return Err(ApiError::Forbidden {
            message: "Public rewards catalog is disabled on this channel".to_string(),
        });
    }

    let reward = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Reward '{}' not found", reward_id),
        })?;

    if reward.streamer_id != broadcaster.channel_id || reward.is_deleted || !reward.is_public {
        return Err(ApiError::NotFound {
            message: format!("Reward '{}' not found on this channel", reward_id),
        });
    }

    if reward.is_paused && !cfg.show_paused_rewards {
        return Err(ApiError::Forbidden {
            message: "This reward is currently paused".to_string(),
        });
    }

    Ok(Json(build_public_reward_response(&reward, &cfg, setting.base_price_multiplier)))
}

fn build_public_reward_response(
    r: &crate::db::rewards::Reward,
    cfg: &crate::db::broadcaster_settings::PublicRewardsConfig,
    base_multiplier: i16,
) -> PublicRewardResponse {
    let cost_points = if cfg.show_cost_points {
        match r.pricing_mode {
            crate::db::rewards::PricingMode::Manual => r.manual_twitch_points,
            crate::db::rewards::PricingMode::Auto => {
                let markup = 1.0 + (r.twitch_price_markup_percentage as f64 / 100.0);
                let mult = base_multiplier as f64;
                let price_major = r.current_market_price as f64 / 100.0;
                Some((price_major * mult * markup).round() as i32)
            }
        }
    } else {
        None
    };

    let market_price = if cfg.show_market_price {
        Some(r.current_market_price as f64 / 100.0)
    } else {
        None
    };

    let currency = if cfg.show_market_price {
        Some(r.currency.clone())
    } else {
        None
    };

    let pause_reason = if r.is_paused && cfg.show_pause_reason {
        r.pause_reason.map(|p| p.as_str().to_string())
    } else {
        None
    };

    let pool_items = if cfg.show_pool_items {
        r.pool_items.as_ref().map(|items| {
            let total_weight: f64 = items.0.iter().map(|item| item.weight).sum();
            items.0.iter().map(|item| {
                let chance = if cfg.show_pool_chances && total_weight > 0.0 {
                    let pct = (item.weight / total_weight) * 100.0;
                    Some((pct * 100.0).round() / 100.0)
                } else {
                    None
                };

                let item_price = if cfg.show_pool_item_prices {
                    Some(item.current_market_price as f64 / 100.0)
                } else {
                    None
                };

                let pool_deviation = if cfg.show_price_deviation {
                    Some(item.permissible_market_price_deviation)
                } else {
                    None
                };

                PublicPoolItem {
                    market_hash_name: item.market_hash_name.clone(),
                    chance_percentage: chance,
                    current_market_price: item_price,
                    permissible_market_price_deviation: pool_deviation,
                }
            }).collect()
        })
    } else {
        None
    };

    let filter_details = if cfg.show_filter_details {
        r.filter_config.as_ref().map(|f| PublicFilterDetails {
            min_price: Some(f.0.min_price),
            max_price: Some(f.0.max_price),
            name_prefix: f.0.name_prefix.clone(),
            name_suffix: f.0.name_suffix.clone(),
            name_contains: f.0.name_contains.clone(),
        })
    } else {
        None
    };

    let chat_requirements = if cfg.show_chat_requirements && (r.chat_min_messages.is_some() || r.chat_min_characters.is_some()) {
        Some(PublicChatRequirements {
            min_messages: r.chat_min_messages,
            min_characters: r.chat_min_characters,
            time_window_hours: r.chat_time_window_hours,
            logical_operator: r.chat_logical_operator.map(|op| format!("{:?}", op)),
        })
    } else {
        None
    };

    let purchase_limits = if cfg.show_purchase_limits {
        r.purchase_limits.as_ref().map(|p| p.0.clone())
    } else {
        None
    };

    let (cooldown, max_stream, max_user_stream) = if cfg.show_cooldown_and_limits {
        (
            Some(r.global_cooldown_seconds),
            Some(r.max_redemptions_per_stream),
            Some(r.max_redemptions_per_user_per_stream),
        )
    } else {
        (None, None, None)
    };

    let deviation = if cfg.show_price_deviation {
        Some(r.permissible_market_price_deviation)
    } else {
        None
    };

    PublicRewardResponse {
        twitch_id: r.twitch_id,
        twitch_title: r.twitch_title.clone(),
        twitch_description: if cfg.show_description { Some(r.twitch_description.clone()) } else { None },
        reward_type: r.reward_type,
        pricing_mode: r.pricing_mode,
        is_paused: r.is_paused,
        pause_reason,
        cost_points,
        global_cooldown_seconds: cooldown,
        max_redemptions_per_stream: max_stream,
        max_redemptions_per_user_per_stream: max_user_stream,
        market_price,
        permissible_market_price_deviation: deviation,
        currency,
        market_item_name: r.market_item_name.clone(),
        pool_items,
        filter_details,
        chat_requirements,
        purchase_limits,
    }
}
