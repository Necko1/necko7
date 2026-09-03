use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use chrono::{DateTime, Duration, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::warn;
use crate::AppResult;
use crate::db::app_settings::{KEY_APP_TOKEN, KEY_BOT_AUTH};
use crate::db::Db;
use crate::db::error::DbResult;
use crate::helix::error::HelixError;
use crate::helix::api::custom_rewards::model::UpdateCustomReward;
use crate::helix::{api, HelixClient};
use crate::steam::market::{self, MarketClient};

#[derive(Serialize, Deserialize)]
pub struct BotInfo {
    pub user_login: String,
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMarketBalance {
    pub money: f64,
    pub money_settlement: f64,
    pub currency: String,
    pub updated_at: DateTime<Utc>,
}

pub struct AppState {
    pub helix_client: HelixClient,
    pub market_client: MarketClient,

    // env
    pub webhook_secret: String,
    pub client_id: String,
    pub client_secret: String,
    pub app_url: String,
    pub frontend_url: String,

    pub bot_info: RwLock<Option<BotInfo>>,
    pub market_balances: RwLock<HashMap<String, CachedMarketBalance>>,
    pub active_broadcaster_tasks: Mutex<HashSet<String>>,

    pub db: Db,

    pub app_initialized: AtomicBool,
}

impl AppState {
    pub async fn from_env(db: Db) -> DbResult<Arc<Self>> {
        let bot_info: Option<BotInfo> = db
            .get_setting(KEY_BOT_AUTH)
            .await?
            .and_then(|json| serde_json::from_str(&json).ok());

        let app_initialized = bot_info.is_some();

        let client_id = env::var("TWITCH_CLIENT_ID")
            .expect("TWITCH_CLIENT_ID not found in the environment");
        let client_secret = env::var("TWITCH_CLIENT_SECRET")
            .expect("TWITCH_CLIENT_SECRET not found in the environment");

        Ok(Arc::new(Self {
            helix_client: HelixClient::new(client_id.clone(), client_secret.clone()),
            market_client: MarketClient::new(),
            webhook_secret: env::var("TWITCH_EVENTSUB_SECRET")
                .expect("TWITCH_EVENTSUB_SECRET not found in the environment"),
            client_id,
            client_secret,
            app_url: env::var("APP_URL")
                .expect("APP_URL not found in the environment"),
            frontend_url: env::var("FRONTEND_URL")
                .expect("FRONTEND_URL not found in the environment"),
            bot_info: RwLock::new(bot_info),
            market_balances: RwLock::new(HashMap::new()),
            active_broadcaster_tasks: Mutex::new(HashSet::new()),
            db,
            app_initialized: AtomicBool::new(app_initialized),
        }))
    }
}

impl AppState {
    pub async fn with_app_token<F, Fut, T>(&self, mut action: F) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let mut token = self.get_or_refresh_app_token().await?;

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                // Если 401 и это первая попытка — обновляем токен и пробуем снова
                Err(HelixError::Unauthorized(ref msg)) if attempt == 0 => {
                    tracing::info!(reason = %msg, "App access token received 401 Unauthorized; refreshing token...");
                    token = self.update_app_access_token().await?;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn with_broadcaster_token<F, Fut, T>(
        &self,
        broadcaster_id: &str,
        mut action: F,
    ) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let broadcaster = match self.db.get_broadcaster_by_id(broadcaster_id).await? {
            Some(b) => b,
            None => {
                return Err(format!("Couldn't find a broadcaster with ID {}", broadcaster_id).into())
            }
        };

        let mut token = broadcaster.user_access_token;

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                Err(HelixError::Unauthorized(ref msg)) if attempt == 0 => {
                    tracing::info!(
                        broadcaster_id = %broadcaster_id,
                        broadcaster_login = %broadcaster.channel_login,
                        reason = %msg,
                        "Broadcaster token received 401 Unauthorized; refreshing via refresh_token..."
                    );
                    let token_res = self.helix_client
                        .refresh_user_token(&broadcaster.refresh_token).await?;

                    self.db.update_broadcaster_tokens(
                        broadcaster_id,
                        &token_res.access_token, &token_res.refresh_token
                    ).await?;

                    token = token_res.access_token;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn with_bot_user_token<F, Fut, T>(
        &self,
        mut action: F,
    ) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let (mut token, refresh_token) = {
            let guard = self.bot_info.read();
            let info = guard.as_ref().ok_or("The bot is not initialized")?;
            (info.access_token.clone(), info.refresh_token.clone())
        };

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                Err(HelixError::Unauthorized(ref msg)) if attempt == 0 => {
                    tracing::info!(reason = %msg, "Bot user token received 401 Unauthorized; refreshing token...");
                    let token_res = self.helix_client
                        .refresh_user_token(&refresh_token).await?;

                    let user_info = self.helix_client
                        .get_user_info_by_token(&token_res.access_token).await?;

                    let bot_info = BotInfo {
                        user_login: user_info.login,
                        user_id: user_info.id,
                        access_token: token_res.access_token,
                        refresh_token: token_res.refresh_token,
                    };

                    match serde_json::to_string(&bot_info) {
                        Ok(info_str) => {
                            self.db.set_setting(KEY_BOT_AUTH, &info_str).await?;
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to serialize bot info as string")
                        }
                    }

                    token = bot_info.access_token;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn update_app_access_token(
        &self
    ) -> AppResult<String> {
        let app_token = self.helix_client.request_app_token().await?;

        self.db.set_setting(KEY_APP_TOKEN, &app_token.access_token).await?;

        Ok(app_token.access_token)
    }

    pub async fn get_or_refresh_app_token(&self) -> AppResult<String> {
        if let Some(token) = self.db.get_setting(KEY_APP_TOKEN).await? {
            Ok(token)
        } else {
            self.update_app_access_token().await
        }
    }

    pub async fn create_eventsub_subscription(
        &self,
        broadcaster_user_id: &str,
    ) -> AppResult<()> {
        let callback_url = format!("{}/eventsub", self.app_url);

        let body = api::eventsub::format_subscription(
            &callback_url,
            &self.webhook_secret,
            broadcaster_user_id,
        );

        tracing::info!(
            broadcaster_id = %broadcaster_user_id,
            callback = %callback_url,
            "Creating Twitch EventSub subscription"
        );

        self.with_app_token(|token| {
            let body = body.clone();
            async move {
                self.helix_client.create_subscription(body, &token).await
            }
        }).await
    }

    pub async fn get_cached_or_fetch_balance(&self, channel_id: &str) -> AppResult<CachedMarketBalance> {
        {
            let guard = self.market_balances.read();
            if let Some(cached) = guard.get(channel_id) {
                if Utc::now() - cached.updated_at < Duration::minutes(5) {
                    return Ok(cached.clone());
                }
            }
        }

        self.refresh_broadcaster_balance(channel_id).await
    }

    pub async fn refresh_broadcaster_balance(&self, channel_id: &str) -> AppResult<CachedMarketBalance> {
        let setting = self.db.get_broadcaster_setting(channel_id).await?
            .ok_or_else(|| format!("Broadcaster setting not found for channel {}", channel_id))?;

        if setting.market_api_key.trim().is_empty() {
            return Err("Market API key is not configured for this broadcaster".into());
        }

        let money_res = self.market_client.get_money(&setting.market_api_key).await?;
        if !money_res.success {
            let err = money_res.error.unwrap_or_else(|| "Unknown market API error".to_string());
            tracing::warn!(error = %err, channel_id = %channel_id, "Market get_money returned failure");
            return Err(format!("Market API error: {}", err).into());
        }

        let balance = CachedMarketBalance {
            money: money_res.money.unwrap_or(0.0),
            money_settlement: money_res.money_settlement.unwrap_or(0.0),
            currency: money_res.currency.unwrap_or_else(|| "RUB".to_string()),
            updated_at: Utc::now(),
        };

        tracing::debug!(
            channel_id = %channel_id,
            money = balance.money,
            settlement = balance.money_settlement,
            currency = %balance.currency,
            "Refreshed broadcaster market balance"
        );

        {
            let mut guard = self.market_balances.write();
            guard.insert(channel_id.to_string(), balance.clone());
        }

        if setting.pause_reward_if_no_money {
            self.sync_rewards_pause_by_balance(channel_id, balance.money).await;
        }

        Ok(balance)
    }

    pub async fn sync_rewards_pause_by_balance(&self, channel_id: &str, current_balance: f64) {
        let rewards = match self.db.get_rewards_by_streamer_filtered(channel_id, None, Some(false)).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, channel_id = %channel_id, "Failed to fetch rewards for balance pause sync");
                return;
            }
        };

        for reward in rewards {
            if reward.current_market_price <= 0 {
                continue;
            }

            let max_price = (reward.current_market_price as i64)
                + ((reward.current_market_price as i64 * reward.permissible_market_price_deviation as i64) / 100);
            let cost = market::minor_to_major(max_price, &reward.currency);
            let has_enough_money = current_balance >= cost;

            let target_paused = !has_enough_money;

            if reward.is_paused == target_paused {
                continue;
            }

            let r_id_str = reward.twitch_id.to_string();
            let bc_id = channel_id.to_string();
            let update_res = self.with_broadcaster_token(channel_id, |token| {
                let r_str = r_id_str.clone();
                let b_str = bc_id.clone();
                async move {
                    self.helix_client.update_custom_reward(
                        &b_str,
                        &r_str,
                        UpdateCustomReward {
                            is_paused: Some(target_paused),
                            ..Default::default()
                        },
                        &token,
                    ).await
                }
            }).await;

            match update_res {
                Ok(_) => {
                    if let Err(e) = self.db.set_reward_paused(reward.twitch_id, target_paused).await {
                        warn!(error = %e, reward_id = %reward.twitch_id, "Failed to update reward pause status in DB");
                    } else {
                        tracing::info!(
                            reward_id = %reward.twitch_id,
                            title = %reward.twitch_title,
                            paused = target_paused,
                            cost = cost,
                            balance = current_balance,
                            "Auto-updated reward pause status due to balance check"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        reward_id = %reward.twitch_id,
                        "Failed to update custom reward pause status on Twitch during balance check"
                    );
                }
            }
        }
    }

    pub async fn recover_eventsub_subscriptions(&self) {
        match self.db.get_all_broadcasters().await {
            Ok(broadcasters) => {
                for broadcaster in broadcasters {
                    let is_active = match self.db.get_broadcaster_setting(&broadcaster.channel_id).await {
                        Ok(Some(s)) => s.is_active,
                        _ => true,
                    };

                    if is_active {
                        tracing::info!(
                            broadcaster_login = %broadcaster.channel_login,
                            broadcaster_id = %broadcaster.channel_id,
                            "Subscribing EventSub for active broadcaster on startup"
                        );
                        if let Err(e) = self.create_eventsub_subscription(&broadcaster.channel_id).await {
                            tracing::warn!(
                                error = %e,
                                broadcaster_login = %broadcaster.channel_login,
                                broadcaster_id = %broadcaster.channel_id,
                                "Failed to recover EventSub subscription"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to get broadcasters from DB for EventSub recovery");
            }
        }
    }
}