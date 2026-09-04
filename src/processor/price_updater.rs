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
        let rewards = self.state.db.get_rewards_by_streamer_filtered(&self.broadcaster_id, None, Some(false)).await
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
    let items_res = match state.market_client.search_item(&setting.market_api_key, &reward.market_item_name).await {
        Ok(res) => res,
        Err(e) => {
            return Err(format!("Failed to search item {}: {}", reward.market_item_name, e).into());
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

    let update_patch = crate::db::rewards::UpdateReward {
        current_market_price: Some(cheapest.price as i32),
        currency: items_res.currency.clone(),
        ..Default::default()
    };
    state.db.update_reward(reward.twitch_id, &update_patch).await?;

    info!(
        reward_id = %reward.twitch_id,
        reward = %reward.twitch_title,
        old_market_price = reward.current_market_price,
        new_market_price = cheapest.price,
        new_cost = new_twitch_points_cost,
        "Reward price updated successfully"
    );

    Ok(())
}

