use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::db::broadcaster_settings::BroadcasterSetting;
use crate::db::rewards::Reward;
use crate::helix::api::custom_rewards::model::UpdateCustomReward;
use crate::state::AppState;
use crate::steam::market;

pub struct PriceUpdater {
    state: Arc<AppState>,
    broadcaster_id: String,
}

impl PriceUpdater {
    pub fn new(state: Arc<AppState>, broadcaster_id: String) -> Self {
        Self {
            state,
            broadcaster_id,
        }
    }

    pub async fn run(self, token: CancellationToken) {
        info!(broadcaster_id = %self.broadcaster_id, "Starting price updater task for broadcaster");

        loop {
            if token.is_cancelled() {
                debug!(broadcaster_id = %self.broadcaster_id, "Price updater received cancellation; stopping");
                break;
            }

            let setting = match self.state.db.get_broadcaster_setting(&self.broadcaster_id).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    debug!(broadcaster_id = %self.broadcaster_id, "Broadcaster setting not found, waiting before retry");
                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(60)) => continue,
                    }
                }
                Err(e) => {
                    error!(error = %e, broadcaster_id = %self.broadcaster_id, "DB error fetching broadcaster setting");
                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(60)) => continue,
                    }
                }
            };

            let period_secs = (setting.update_prices_period as u64).max(60);

            if setting.is_active && !setting.market_api_key.trim().is_empty() {
                if let Err(e) = self.update_prices(&setting, &token).await {
                    warn!(error = %e, broadcaster_id = %self.broadcaster_id, "Error during price update run");
                }
            }

            tokio::select! {
                _ = token.cancelled() => {
                    debug!(broadcaster_id = %self.broadcaster_id, "Price updater received cancellation; stopping");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(period_secs)) => {}
            }
        }
    }

    async fn update_prices(
        &self,
        setting: &BroadcasterSetting,
        token: &CancellationToken,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let rewards = self.state.db.get_rewards_by_streamer_filtered(&self.broadcaster_id, None, Some(false), None).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        if rewards.is_empty() {
            return Ok(());
        }

        debug!(broadcaster_id = %self.broadcaster_id, count = rewards.len(), "Checking prices for active rewards");

        for reward in rewards {
            if token.is_cancelled() {
                debug!(broadcaster_id = %self.broadcaster_id, "Price updater cancelled during reward batch; aborting batch");
                break;
            }

            if let Err(e) = update_reward_price_inner(&self.state, setting, &reward).await {
                warn!(
                    error = %e,
                    reward = %reward.twitch_title,
                    reward_id = %reward.twitch_id,
                    broadcaster_id = %self.broadcaster_id,
                    "Failed to update price for reward"
                );
            }
        }

        Ok(())
    }
}

pub async fn update_single_reward_price(
    state: &Arc<AppState>,
    broadcaster_id: &str,
    reward_id: Uuid,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let setting = state.db.get_broadcaster_setting(broadcaster_id).await?
        .ok_or_else(|| format!("Broadcaster setting not found for channel {}", broadcaster_id))?;

    if setting.market_api_key.trim().is_empty() {
        return Err("Market API key is not configured".into());
    }

    let reward = state.db.get_reward_by_twitch_id(reward_id).await?
        .ok_or_else(|| format!("Reward {} not found", reward_id))?;

    update_reward_price_inner(state, &setting, &reward).await
}

pub async fn update_reward_price_inner(
    state: &Arc<AppState>,
    setting: &BroadcasterSetting,
    reward: &Reward,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match reward.reward_type {
        crate::db::rewards::RewardType::Fixed => {
            update_fixed_reward_price(state, setting, reward).await
        }
        crate::db::rewards::RewardType::Pool => {
            update_pool_reward_price(state, setting, reward).await
        }
        crate::db::rewards::RewardType::Filter => {
            update_filter_reward_price(state, setting, reward).await
        }
    }
}

async fn update_fixed_reward_price(
    state: &Arc<AppState>,
    setting: &BroadcasterSetting,
    reward: &Reward,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let item_name = match reward.market_item_name.as_deref() {
        Some(name) if !name.trim().is_empty() => name,
        _ => return Err("Fixed reward has no market_item_name".into()),
    };

    let items_res = match state.market_client.search_item(&setting.market_api_key, item_name).await {
        Ok(res) => res,
        Err(e) => {
            return Err(format!("Failed to search item {}: {}", item_name, e).into());
        }
    };

    if let Some(err) = items_res.error {
        return Err(format!("Market error searching item: {}", err).into());
    }

    if !items_res.success || items_res.data.is_none() {
        return Err("Failed to search item on market or no data returned".into());
    }

    let items_data = items_res.data.unwrap();
    let cheapest = items_data.iter().min_by_key(|i| i.price)
        .ok_or_else(|| "No market listings available for item")?;

    let currency = items_res.currency.as_deref().unwrap_or(&reward.currency);
    let price_decimal = market::minor_to_major(cheapest.price, currency);
    let markup_factor = 1.0 + (reward.twitch_price_markup_percentage as f64 / 100.0).max(0.0);
    let raw_cost = price_decimal * markup_factor * setting.base_price_multiplier as f64;
    let new_twitch_points_cost = (raw_cost.ceil() as u32).max(1);

    apply_twitch_and_db_cost_update(state, reward, new_twitch_points_cost, cheapest.price as i32, items_res.currency.clone(), None).await?;

    info!(
        reward_id = %reward.twitch_id,
        reward = %reward.twitch_title,
        old_market_price = reward.current_market_price,
        new_market_price = cheapest.price,
        new_cost = new_twitch_points_cost,
        "Fixed reward price updated successfully"
    );

    Ok(())
}

async fn update_pool_reward_price(
    state: &Arc<AppState>,
    setting: &BroadcasterSetting,
    reward: &Reward,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut pool_items = match reward.pool_items.as_ref().map(|j| j.0.clone()) {
        Some(items) if !items.is_empty() => items,
        _ => return Err("Pool reward has empty pool_items".into()),
    };

    let all_prices = state.get_cached_or_fetch_prices(&reward.currency).await
        .map_err(|e| format!("Failed to fetch market prices for pool reward: {}", e))?;

    let price_map: std::collections::HashMap<&str, f64> = all_prices
        .iter()
        .map(|i| (i.market_hash_name.as_str(), i.price))
        .collect();

    let mut price_weight_pairs: Vec<(f64, f64)> = Vec::with_capacity(pool_items.len());
    let mut prices_vec: Vec<f64> = Vec::with_capacity(pool_items.len());

    for item in &mut pool_items {
        if let Some(&price_major) = price_map.get(item.market_hash_name.as_str()) {
            item.current_market_price = market::major_to_minor(price_major, &reward.currency) as i32;
        }
        let p_major = market::minor_to_major(item.current_market_price as i64, &reward.currency);
        prices_vec.push(p_major);
        price_weight_pairs.push((p_major, item.weight));
    }

    let strategy = reward.price_strategy.unwrap_or(crate::db::rewards::PriceStrategy::Average);
    let effective_price_major = match strategy {
        crate::db::rewards::PriceStrategy::Average => {
            crate::steam::market::prices::calculate_weighted_average(&price_weight_pairs)
                .unwrap_or(0.0)
        }
        crate::db::rewards::PriceStrategy::Median => {
            crate::steam::market::prices::calculate_median(&mut prices_vec)
                .unwrap_or(0.0)
        }
        crate::db::rewards::PriceStrategy::Max => {
            crate::steam::market::prices::calculate_max(&prices_vec)
                .unwrap_or(0.0)
        }
    };

    if effective_price_major <= 0.0 {
        return Err("Effective price for pool items is zero or negative".into());
    }

    let markup_factor = 1.0 + (reward.twitch_price_markup_percentage as f64 / 100.0).max(0.0);
    let raw_cost = effective_price_major * markup_factor * setting.base_price_multiplier as f64;
    let new_twitch_points_cost = (raw_cost.ceil() as u32).max(1);
    let new_market_price = market::major_to_minor(effective_price_major, &reward.currency) as i32;

    apply_twitch_and_db_cost_update(
        state,
        reward,
        new_twitch_points_cost,
        new_market_price,
        None,
        Some(sqlx::types::Json(pool_items)),
    ).await?;

    info!(
        reward_id = %reward.twitch_id,
        reward = %reward.twitch_title,
        strategy = ?strategy,
        new_cost = new_twitch_points_cost,
        "Pool reward price updated successfully"
    );

    Ok(())
}

async fn update_filter_reward_price(
    state: &Arc<AppState>,
    setting: &BroadcasterSetting,
    reward: &Reward,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = match reward.filter_config.as_ref().map(|j| &j.0) {
        Some(f) => f,
        None => return Err("Filter reward has no filter_config".into()),
    };

    let all_prices = state.get_cached_or_fetch_prices(&reward.currency).await
        .map_err(|e| format!("Failed to fetch market prices for filter reward: {}", e))?;

    let matching = crate::steam::market::prices::filter_prices(&all_prices, filter);
    if matching.is_empty() {
        warn!(reward_id = %reward.twitch_id, "No items in prices.json match filter criteria");
        return Ok(());
    }

    let mut prices: Vec<f64> = matching.iter().map(|i| i.price).collect();
    let strategy = reward.price_strategy.unwrap_or(crate::db::rewards::PriceStrategy::Average);

    let effective_price_major = match strategy {
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

    let markup_factor = 1.0 + (reward.twitch_price_markup_percentage as f64 / 100.0).max(0.0);
    let raw_cost = effective_price_major * markup_factor * setting.base_price_multiplier as f64;
    let new_twitch_points_cost = (raw_cost.ceil() as u32).max(1);
    let new_market_price = market::major_to_minor(effective_price_major, &reward.currency) as i32;

    apply_twitch_and_db_cost_update(state, reward, new_twitch_points_cost, new_market_price, None, None).await?;

    info!(
        reward_id = %reward.twitch_id,
        reward = %reward.twitch_title,
        strategy = ?strategy,
        new_cost = new_twitch_points_cost,
        "Filter reward price updated successfully"
    );

    Ok(())
}

async fn apply_twitch_and_db_cost_update(
    state: &Arc<AppState>,
    reward: &Reward,
    new_twitch_points_cost: u32,
    new_market_price: i32,
    currency: Option<String>,
    pool_items: Option<sqlx::types::Json<Vec<crate::db::rewards::PoolItemConfig>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reward.pricing_mode == crate::db::rewards::PricingMode::Auto {
        let bc_id = reward.streamer_id.clone();
        let bc_ref = bc_id.clone();
        let r_id = reward.twitch_id.to_string();
        let state_clone = state.clone();

        state.with_broadcaster_token(&bc_ref, move |token| {
            let b = bc_id.clone();
            let r = r_id.clone();
            let s = state_clone.clone();
            async move {
                s.helix_client.update_custom_reward(
                    &b,
                    &r,
                    UpdateCustomReward {
                        cost: Some(new_twitch_points_cost),
                        ..Default::default()
                    },
                    &token,
                ).await
            }
        }).await
        .map_err(|e| e.to_string())?;
    } else {
        debug!(
            reward_id = %reward.twitch_id,
            "Reward pricing_mode is Manual; skipped updating Channel Points cost on Twitch Helix"
        );
    }

    let update_patch = crate::db::rewards::UpdateReward {
        current_market_price: Some(new_market_price),
        currency,
        pool_items,
        ..Default::default()
    };
    state.db.update_reward(reward.twitch_id, &update_patch).await?;

    check_and_sync_price_limits(state, reward, new_market_price).await?;

    Ok(())
}

pub async fn check_and_sync_price_limits(
    state: &Arc<AppState>,
    reward: &Reward,
    current_price: i32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let has_min = reward.min_market_price.is_some();
    let has_max = reward.max_market_price.is_some();
    if !has_min && !has_max {
        return Ok(());
    }

    let is_below_min = reward.min_market_price.is_some_and(|min_p| current_price < min_p);
    let is_above_max = reward.max_market_price.is_some_and(|max_p| current_price > max_p);
    let is_out_of_bounds = is_below_min || is_above_max;

    if is_out_of_bounds {
        if !reward.is_paused {
            warn!(
                reward_id = %reward.twitch_id,
                title = %reward.twitch_title,
                price = current_price,
                min = ?reward.min_market_price,
                max = ?reward.max_market_price,
                "Reward market price is outside configured limits; pausing on Twitch and DB with PRICE_LIMIT"
            );

            let bc_id = reward.streamer_id.clone();
            let bc_ref = bc_id.clone();
            let r_id = reward.twitch_id.to_string();
            let state_clone = state.clone();

            state.with_broadcaster_token(&bc_ref, move |token| {
                let b = bc_id.clone();
                let r = r_id.clone();
                let s = state_clone.clone();
                async move {
                    s.helix_client.update_custom_reward(
                        &b,
                        &r,
                        UpdateCustomReward {
                            is_paused: Some(true),
                            ..Default::default()
                        },
                        &token,
                    ).await
                }
            }).await
            .map_err(|e| e.to_string())?;

            state.db.set_reward_paused(
                reward.twitch_id,
                true,
                Some(crate::db::rewards::PauseReason::PriceLimit),
            ).await?;
        }
    } else {
        // Price is within bounds! If it was paused due to PriceLimit, auto-unpause it!
        if reward.is_paused && matches!(reward.pause_reason, Some(crate::db::rewards::PauseReason::PriceLimit)) {
            let setting = state.db.get_broadcaster_setting(&reward.streamer_id).await?;
            let has_enough_money = if let Some(ref s) = setting {
                if s.pause_reward_if_no_money {
                    if let Ok(balance) = state.get_cached_or_fetch_balance(&reward.streamer_id).await {
                        let dev = reward.permissible_market_price_deviation as i64;
                        let max_price_minor = current_price as i64 + (current_price as i64 * dev) / 100;
                        let cost = market::minor_to_major(max_price_minor, &reward.currency);
                        balance.money >= cost
                    } else {
                        true
                    }
                } else {
                    true
                }
            } else {
                true
            };

            if has_enough_money {
                info!(
                    reward_id = %reward.twitch_id,
                    title = %reward.twitch_title,
                    price = current_price,
                    "Reward market price returned within configured limits; auto-unpausing reward"
                );

                let bc_id = reward.streamer_id.clone();
                let bc_ref = bc_id.clone();
                let r_id = reward.twitch_id.to_string();
                let state_clone = state.clone();

                state.with_broadcaster_token(&bc_ref, move |token| {
                    let b = bc_id.clone();
                    let r = r_id.clone();
                    let s = state_clone.clone();
                    async move {
                        s.helix_client.update_custom_reward(
                            &b,
                            &r,
                            UpdateCustomReward {
                                is_paused: Some(false),
                                ..Default::default()
                            },
                            &token,
                        ).await
                    }
                }).await
                .map_err(|e| e.to_string())?;

                state.db.set_reward_paused(
                    reward.twitch_id,
                    false,
                    None,
                ).await?;
            } else {
                warn!(
                    reward_id = %reward.twitch_id,
                    title = %reward.twitch_title,
                    "Reward market price is within limits, but broadcaster has insufficient balance; switching reason to NO_MONEY"
                );
                state.db.set_reward_paused(
                    reward.twitch_id,
                    true,
                    Some(crate::db::rewards::PauseReason::NoMoney),
                ).await?;
            }
        }
    }

    Ok(())
}


