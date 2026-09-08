use std::sync::Arc;
use std::time::Duration;
use tokio::time::Interval;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::datetime::DateTimeExt;
use crate::db::redemptions::RedemptionStatus;
use crate::db::rewards::RewardType;
use crate::messages::{
    MSG_TRADE_ACCEPTED, MSG_TRADE_CREATED, MSG_TRADE_FAILED_BUYER_PENALTY,
    MSG_TRADE_FAILED_BUYER_REFUND, MSG_TRADE_TIMEOUT,
    MSG_ORDER_CREATED, MSG_ORDER_RETRYING, MSG_ORDER_MANUAL_HOLD,
};
use crate::state::AppState;
use crate::steam::market::errors::{classify_market_buy_for_error, MarketBuyForErrorKind};
use crate::steam::market::sell_buy::GetBuyInfoData;
use crate::steam::trade_link::TradeLink;

enum OrderStage {
    Pending,
    Sent,
    Claimed,
    NotClaimed,

    Exit,
}

pub struct WatcherRedemptionData {
    pub redemption_id: Uuid,
    pub custom_id: String,
    pub reward_id: Uuid,
    pub user_login: String,
}

pub struct OrderWatcher {
    state: Arc<AppState>,
    api_key: String,

    stage: OrderStage,

    broadcaster_id: String,
    redemption: WatcherRedemptionData,

    interval: Interval,
    started_at: std::time::Instant,
}

impl OrderWatcher {
    pub fn new(state: Arc<AppState>, api_key: String, broadcaster_id: String, redemption: WatcherRedemptionData) -> Self {
        let interval = tokio::time::interval(Duration::from_secs(15));

        Self {
            state,
            api_key,
            stage: OrderStage::Pending,
            broadcaster_id,
            redemption,
            interval,
            started_at: std::time::Instant::now(),
        }
    }

    pub async fn track_redemption(mut self, token: CancellationToken) {
        self.interval.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!(
                        redemption_id = %self.redemption.redemption_id,
                        user_login = %self.redemption.user_login,
                        "OrderWatcher stopped gracefully; tracking will resume upon restart"
                    );
                    break;
                }
                _ = self.interval.tick() => {}
            }

            if token.is_cancelled() {
                break;
            }

            if self.started_at.elapsed() > Duration::from_mins(30) {
                warn!(
                    redemption_id = %self.redemption.redemption_id,
                    user_login = %self.redemption.user_login,
                    "Order timed out after 30 minutes. Marking as failed penalty."
                );
                self.process_timed_out().await;
                break;
            }

            let current_trade_info = match self.state.market_client.get_buy_info(
                &self.api_key, &self.redemption.custom_id).await
            {
                Ok(info) => info,
                Err(err) => {
                    warn!(error = %err, redemption_id = %self.redemption.redemption_id, "HTTP error fetching market buy info");
                    continue;
                }
            };

            if let Some(ref err) = current_trade_info.error {
                error!(error = %err, redemption_id = %self.redemption.redemption_id, "Market API error in buy info");
            } else if !current_trade_info.success || current_trade_info.data.is_none() {
                warn!(redemption_id = %self.redemption.redemption_id, "Market buy info returned unsuccessful or empty");
            }

            let trade_data = match current_trade_info.data {
                Some(data) => data,
                None => {
                    debug!(redemption_id = %self.redemption.redemption_id, "No trade data available yet, skipping tick");
                    continue;
                }
            };

            match self.stage {
                OrderStage::Pending => self.process_pending_stage(trade_data).await,
                OrderStage::Sent => self.process_sent_stage(trade_data).await,
                _ => break
            }
        }
    }

    async fn process_pending_stage(&mut self, current_trade: GetBuyInfoData) {
        // 1. If settlement is set or stage is 2, the trade was already accepted
        // (fast accept by viewer or watcher resumed after restart). Jump straight to process_sent_stage.
        if current_trade.is_claimed() {
            self.stage = OrderStage::Sent;
            info!(
                redemption_id = %self.redemption.redemption_id,
                user_login = %self.redemption.user_login,
                "Trade was already claimed/settled while in Pending stage, transitioning to Claimed immediately"
            );
            self.process_sent_stage(current_trade).await;
            return;
        }

        // 2. If trade was cancelled or failed on market:
        if current_trade.is_failed() {
            self.process_not_claimed(current_trade).await;
            return;
        }

        // 3. Check if seller sent the Steam trade offer:
        if !current_trade.has_active_trade() {
            // Trade offer not yet sent by seller or trade_id not available, wait for next tick
            return;
        }

        self.stage = OrderStage::Sent;
        info!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            trade_id = ?current_trade.trade_id,
            "Steam trade offer detected, transitioning to Sent stage"
        );

        let remaining = current_trade.receive_until.unwrap().remaining_pretty();
        let tradeoffer = format!("https://steamcommunity.com/tradeoffer/{}/",
                                 current_trade.trade_id.as_deref().unwrap_or(""));

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_CREATED,
            &[
                ("buyer", &self.redemption.user_login),
                ("remaining", &remaining),
                ("tradeoffer", &tradeoffer),
                ("item", &current_trade.market_hash_name),
            ],
        );

        if let Err(e) = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await {
            error!(
                error = %e,
                redemption_id = %self.redemption.redemption_id,
                broadcaster_id = %self.broadcaster_id,
                "Failed to send trade created chat message"
            );
            return;
        };
    }

    async fn process_sent_stage(&mut self, current_trade: GetBuyInfoData) {
        // 1. If trade was cancelled or failed on market:
        if current_trade.is_failed() {
            self.process_not_claimed(current_trade).await;
            return;
        }

        // 2. Waiting for user to accept the trade offer
        if !current_trade.is_claimed() {
            return;
        }

        self.stage = OrderStage::Claimed;
        info!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            "Steam trade accepted by user! Transitioning to Claimed stage"
        );

        self.state.channel_logger.log_redemption_completed(
            &self.broadcaster_id,
            &self.redemption.redemption_id.to_string(),
            Some(&current_trade.market_hash_name),
            &self.redemption.user_login,
        );

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_ACCEPTED,
            &[
                ("buyer", &self.redemption.user_login),
                ("item", &current_trade.market_hash_name),
            ],
        );

        if let Err(e) = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await {
            error!(
                error = %e,
                redemption_id = %self.redemption.redemption_id,
                broadcaster_id = %self.broadcaster_id,
                "Failed to send trade accepted chat message"
            );
        };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            RedemptionStatus::Completed,
            None, None
        ).await {
            error!(error = %e, redemption_id = %self.redemption.redemption_id, "Failed to update redemption status to Completed in DB");
            return;
        };

        if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
            self.state.helix_client.update_redemption_status(
                &self.broadcaster_id,
                &self.redemption.reward_id.to_string(),
                &self.redemption.redemption_id.to_string(),
                false,
                &token).await
        }).await {
            warn!(
                error = %e,
                redemption_id = %self.redemption.redemption_id,
                reward_id = %self.redemption.reward_id,
                broadcaster_id = %self.broadcaster_id,
                "Failed to update redemption status on Twitch Helix (it may have already been fulfilled/canceled)"
            );
        };

        let state_for_balance = self.state.clone();
        let bc_id_for_balance = self.broadcaster_id.clone();
        self.state.spawn_task(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });
    }

    async fn process_not_claimed(&mut self, current_trade: GetBuyInfoData) {
        if !current_trade.is_failed() {
            return;
        }

        self.stage = OrderStage::NotClaimed;

        let broadcaster_setting = match self.state.db.get_broadcaster_setting(&self.broadcaster_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                error!(broadcaster_id = %self.broadcaster_id, redemption_id = %self.redemption.redemption_id, "Broadcaster settings not found in DB during unhandled trade failure");
                self.stage = OrderStage::Exit;
                return;
            }
            Err(e) => {
                error!(error = %e, broadcaster_id = %self.broadcaster_id, redemption_id = %self.redemption.redemption_id, "DB Error fetching broadcaster settings");
                return;
            }
        };
        let refund_on_buyer_fail = broadcaster_setting.refund_on_buyer_fail;

        let buyer_fault = current_trade.causer.is_some_and(|c| c.eq("buyer"));

        if buyer_fault {
            let should_refund = refund_on_buyer_fail;
            warn!(
                redemption_id = %self.redemption.redemption_id,
                user_login = %self.redemption.user_login,
                buyer_fault = true,
                refund_on_buyer_fail,
                should_refund,
                "Trade was not claimed by buyer / timed out on market"
            );

            let hint = "Viewer did not accept the trade offer on Steam in time or declined it. Points were refunded or penalized according to channel settings. You can retry the redemption manually if needed.";
            self.state.channel_logger.log(
                &self.broadcaster_id,
                crate::db::channel_logs::ChannelLogLevel::Warn,
                crate::db::channel_logs::ChannelLogCategory::Redemption,
                "TRADE_NOT_CLAIMED",
                format!("Trade offer for item \"{}\" to @{} was not completed (fault: viewer)", current_trade.market_hash_name, self.redemption.user_login),
                Some(serde_json::json!({
                    "redemption_id": self.redemption.redemption_id,
                    "user_login": self.redemption.user_login,
                    "item_name": current_trade.market_hash_name,
                    "buyer_fault": true,
                    "refunded": should_refund,
                })),
                Some(hint.to_string()),
            );

            let msg_id = if should_refund {
                MSG_TRADE_FAILED_BUYER_REFUND
            } else {
                MSG_TRADE_FAILED_BUYER_PENALTY
            };

            let message = self.state.render_chat_message(
                &self.broadcaster_id,
                msg_id,
                &[
                    ("buyer", &self.redemption.user_login),
                    ("item", &current_trade.market_hash_name),
                ],
            );

            if let Err(e) = self.state.send_chat_message(&self.broadcaster_id, &message, None).await {
                error!(error = %e, redemption_id = %self.redemption.redemption_id, broadcaster_id = %self.broadcaster_id, "Failed to send not-claimed chat message");
            };

            let redemption_status = if should_refund { RedemptionStatus::FailedRefund } else { RedemptionStatus::FailedPenalty };

            if let Err(e) = self.state.db.update_redemption_status(
                self.redemption.redemption_id,
                redemption_status,
                Some("buyer_not_claimed"), None
            ).await {
                error!(error = %e, redemption_id = %self.redemption.redemption_id, status = ?redemption_status, "Failed to update redemption status in DB");
                return;
            };

            if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
                self.state.helix_client.update_redemption_status(
                    &self.broadcaster_id,
                    &self.redemption.reward_id.to_string(),
                    &self.redemption.redemption_id.to_string(),
                    should_refund,
                    &token).await
            }).await {
                warn!(
                    error = %e,
                    redemption_id = %self.redemption.redemption_id,
                    reward_id = %self.redemption.reward_id,
                    broadcaster_id = %self.broadcaster_id,
                    "Failed to update redemption status on Twitch Helix (it may have already been fulfilled/canceled)"
                );
            };

            let state_for_balance = self.state.clone();
            let bc_id_for_balance = self.broadcaster_id.clone();
            self.state.spawn_task(async move {
                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
            });
            return;
        }

        // --- Seller fault (!buyer_fault) ---
        warn!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            "Market seller did not deliver trade offer in time"
        );

        let state_for_balance = self.state.clone();
        let bc_id_for_balance = self.broadcaster_id.clone();
        self.state.spawn_task(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });

        let reward = match self.state.db.get_reward_by_twitch_id(self.redemption.reward_id).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                error!(reward_id = %self.redemption.reward_id, "Reward not found in DB during seller retry");
                self.stage = OrderStage::Exit;
                return;
            }
            Err(e) => {
                error!(error = %e, reward_id = %self.redemption.reward_id, "DB error fetching reward during seller retry");
                self.stage = OrderStage::Exit;
                return;
            }
        };

        loop {
            let redemption = match self.state.db.get_redemption(self.redemption.redemption_id).await {
                Ok(Some(r)) => r,
                Ok(None) => {
                    error!(redemption_id = %self.redemption.redemption_id, "Redemption not found in DB during seller failure handling");
                    self.stage = OrderStage::Exit;
                    return;
                }
                Err(e) => {
                    error!(error = %e, redemption_id = %self.redemption.redemption_id, "DB error fetching redemption during seller failure handling");
                    self.stage = OrderStage::Exit;
                    return;
                }
            };

            let trade_link = match TradeLink::parse(&redemption.user_trade_link) {
                Some(tl) => tl,
                None => {
                    error!(redemption_id = %self.redemption.redemption_id, "Invalid trade link stored during seller retry");
                    self.stage = OrderStage::Exit;
                    return;
                }
            };

            if redemption.retry_count >= 2 {
                warn!(redemption_id = %self.redemption.redemption_id, "Seller failed and retries already exhausted, placing on MANUAL_HOLD");
                let _ = self.state.db.set_redemption_manual_hold(
                    self.redemption.redemption_id,
                    "seller_timeout_retries_exhausted",
                    Some("Market seller did not deliver item and auto-retry limit reached"),
                ).await;
                self.state.channel_logger.log_redemption_manual_hold(
                    &self.broadcaster_id,
                    &self.redemption.redemption_id.to_string(),
                    &self.redemption.user_login,
                    &current_trade.market_hash_name,
                    "Market seller did not deliver item and retry limit reached",
                    None,
                );
                let message = self.state.render_chat_message(
                    &self.broadcaster_id,
                    MSG_ORDER_MANUAL_HOLD,
                    &[
                        ("buyer", &self.redemption.user_login),
                        ("item", &current_trade.market_hash_name),
                    ],
                );
                let _ = self.state.send_chat_message(&self.broadcaster_id, &message, None).await;
                self.stage = OrderStage::Exit;
                return;
            }

            let new_retry_count = match self.state.db.increment_retry_count(self.redemption.redemption_id).await {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, redemption_id = %self.redemption.redemption_id, "Failed to increment retry count in DB");
                    self.stage = OrderStage::Exit;
                    return;
                }
            };
            let new_custom_id = format!("{}-{}", self.redemption.redemption_id, new_retry_count);

            info!(
                redemption_id = %self.redemption.redemption_id,
                retry_count = new_retry_count,
                item = %current_trade.market_hash_name,
                "Seller failed; retrying market purchase automatically"
            );

            // Notify chat about retry
            let retrying_msg = self.state.render_chat_message(
                &self.broadcaster_id,
                MSG_ORDER_RETRYING,
                &[("buyer", &self.redemption.user_login), ("item", &current_trade.market_hash_name)],
            );
            let _ = self.state.send_chat_message(&self.broadcaster_id, &retrying_msg, None).await;

            // Wait 5 seconds before retrying
            tokio::time::sleep(Duration::from_secs(5)).await;

            // Double check if order was already created on market
            if let Ok(info) = self.state.market_client.get_buy_info(&self.api_key, &new_custom_id).await {
                if info.success && info.data.as_ref().map_or(false, |d| !d.is_failed()) {
                    let data = info.data.unwrap();
                    let paid_price = (data.paid * 100.0) as i64;
                    let _ = self.state.db.set_redemption_order_created(
                        self.redemption.redemption_id,
                        paid_price,
                        Some(&current_trade.market_hash_name),
                        new_retry_count,
                    ).await;

                    self.redemption.custom_id = new_custom_id;
                    self.stage = OrderStage::Pending;
                    self.started_at = std::time::Instant::now();
                    return;
                }
            }

            let (base_price, dev) = match reward.reward_type {
                RewardType::Pool => {
                    let pool_item = reward.pool_items.as_ref()
                        .and_then(|j| j.0.iter().find(|i| i.market_hash_name == current_trade.market_hash_name));
                    if let Some(pi) = pool_item {
                        (pi.current_market_price as i64, pi.permissible_market_price_deviation as i64)
                    } else {
                        (reward.current_market_price as i64, reward.permissible_market_price_deviation as i64)
                    }
                }
                _ => (reward.current_market_price as i64, reward.permissible_market_price_deviation as i64),
            };
            let max_price_i64 = base_price + (base_price * dev / 100);
            let max_price = max_price_i64.clamp(0, i32::MAX as i64) as i32;

            let buy_res = self.state.market_client.buy_for(
                &self.api_key,
                &current_trade.market_hash_name,
                max_price,
                broadcaster_setting.market_chance_to_transfer,
                trade_link,
                &new_custom_id,
            ).await;

            match buy_res {
                Ok(res) if res.success => {
                    let paid_price = res.price.unwrap_or(max_price as i64);
                    let _ = self.state.db.set_redemption_order_created(
                        self.redemption.redemption_id,
                        paid_price,
                        Some(&current_trade.market_hash_name),
                        new_retry_count,
                    ).await;

                    self.state.channel_logger.log_redemption_order_created(
                        &self.broadcaster_id,
                        &self.redemption.redemption_id.to_string(),
                        &current_trade.market_hash_name,
                        paid_price,
                        &reward.currency,
                        &self.redemption.user_login,
                    );

                    let msg = self.state.render_chat_message(
                        &self.broadcaster_id,
                        MSG_ORDER_CREATED,
                        &[("buyer", &self.redemption.user_login), ("item", &current_trade.market_hash_name)],
                    );
                    let _ = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await;

                    self.redemption.custom_id = new_custom_id;
                    self.stage = OrderStage::Pending;
                    self.started_at = std::time::Instant::now();
                    return;
                }
                Ok(res) => {
                    let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
                    let code = res.code.unwrap_or(0);
                    let kind = classify_market_buy_for_error(code, &error_msg);

                    if kind == MarketBuyForErrorKind::NotEnoughFunds {
                        warn!(redemption_id = %self.redemption.redemption_id, "Market balance insufficient during seller retry, placing on MANUAL_HOLD");
                        let _ = self.state.db.set_redemption_manual_hold(
                            self.redemption.redemption_id,
                            "no_money",
                            Some(&error_msg),
                        ).await;
                        self.state.channel_logger.log_redemption_manual_hold(
                            &self.broadcaster_id,
                            &self.redemption.redemption_id.to_string(),
                            &self.redemption.user_login,
                            &current_trade.market_hash_name,
                            "Insufficient bot balance on seller retry",
                            Some(&error_msg),
                        );
                        let msg = self.state.render_chat_message(
                            &self.broadcaster_id,
                            MSG_ORDER_MANUAL_HOLD,
                            &[("buyer", &self.redemption.user_login), ("item", &current_trade.market_hash_name)],
                        );
                        let _ = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await;
                        self.stage = OrderStage::Exit;
                        return;
                    }

                    if new_retry_count >= 2 {
                        warn!(redemption_id = %self.redemption.redemption_id, "Retries exhausted during seller failure, placing on MANUAL_HOLD");
                        let _ = self.state.db.set_redemption_manual_hold(
                            self.redemption.redemption_id,
                            "seller_timeout_retries_exhausted",
                            Some(&error_msg),
                        ).await;
                        self.state.channel_logger.log_redemption_manual_hold(
                            &self.broadcaster_id,
                            &self.redemption.redemption_id.to_string(),
                            &self.redemption.user_login,
                            &current_trade.market_hash_name,
                            "Seller timeout and auto-retries exhausted",
                            Some(&error_msg),
                        );
                        let msg = self.state.render_chat_message(
                            &self.broadcaster_id,
                            MSG_ORDER_MANUAL_HOLD,
                            &[("buyer", &self.redemption.user_login), ("item", &current_trade.market_hash_name)],
                        );
                        let _ = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await;
                        self.stage = OrderStage::Exit;
                        return;
                    }
                }
                Err(e) => {
                    error!(error = %e, redemption_id = %self.redemption.redemption_id, "Network error during seller retry");
                    if new_retry_count >= 2 {
                        let _ = self.state.db.set_redemption_manual_hold(
                            self.redemption.redemption_id,
                            "network_error_retries_exhausted",
                            Some(&e.to_string()),
                        ).await;
                        self.state.channel_logger.log_redemption_manual_hold(
                            &self.broadcaster_id,
                            &self.redemption.redemption_id.to_string(),
                            &self.redemption.user_login,
                            &current_trade.market_hash_name,
                            "Network error on seller retry, retries exhausted",
                            Some(&e.to_string()),
                        );
                        let msg = self.state.render_chat_message(
                            &self.broadcaster_id,
                            MSG_ORDER_MANUAL_HOLD,
                            &[("buyer", &self.redemption.user_login), ("item", &current_trade.market_hash_name)],
                        );
                        let _ = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await;
                        self.stage = OrderStage::Exit;
                        return;
                    }
                }
            }
        }
    }

    async fn process_timed_out(&mut self) {
        self.stage = OrderStage::Exit;

        self.state.channel_logger.log(
            &self.broadcaster_id,
            crate::db::channel_logs::ChannelLogLevel::Warn,
            crate::db::channel_logs::ChannelLogCategory::Redemption,
            "TRADE_WATCHER_TIMEOUT",
            format!("Trade offer delivery timed out (30 minutes) for viewer @{}", self.redemption.user_login),
            Some(serde_json::json!({
                "redemption_id": self.redemption.redemption_id,
                "user_login": self.redemption.user_login,
            })),
            Some("Check trade transfer status in Steam trade history or CSGO Market orders.".to_string()),
        );

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_TIMEOUT,
            &[("buyer", &self.redemption.user_login)],
        );

        if let Err(e) = self.state.send_chat_message(&self.broadcaster_id, &msg, None).await {
            error!(error = %e, redemption_id = %self.redemption.redemption_id, broadcaster_id = %self.broadcaster_id, "Failed to send timeout chat message");
        };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            RedemptionStatus::FailedPenalty,
            Some("timeout"),
            Some("Timed out after 30 minutes waiting for trade completion"),
        ).await {
            error!(error = %e, redemption_id = %self.redemption.redemption_id, "Failed to update timed out redemption status in DB");
        }

        if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
            self.state.helix_client.update_redemption_status(
                &self.broadcaster_id,
                &self.redemption.reward_id.to_string(),
                &self.redemption.redemption_id.to_string(),
                false,
                &token).await
        }).await {
            error!(error = %e, redemption_id = %self.redemption.redemption_id, reward_id = %self.redemption.reward_id, broadcaster_id = %self.broadcaster_id, "Failed to fulfill/penalize timed out redemption on Twitch Helix");
        }

        let state_for_balance = self.state.clone();
        let bc_id_for_balance = self.broadcaster_id.clone();
        self.state.spawn_task(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });
    }
}