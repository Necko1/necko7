use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use crate::db::channel_logs::{ChannelLogCategory, ChannelLogLevel, NewChannelLog};
use crate::db::Db;

/// Service for asynchronous, non-blocking recording of channel logs to PostgreSQL.
pub struct ChannelLogger {
    tx: mpsc::Sender<NewChannelLog>,
}

impl ChannelLogger {
    /// Spawns the ChannelLogger background worker that batches and writes logs to PostgreSQL.
    pub fn new(db: Db, shutdown_token: CancellationToken) -> Arc<Self> {
        let (tx, mut rx) = mpsc::channel::<NewChannelLog>(2000);

        tokio::spawn(async move {
            info!("ChannelLogger background worker started");
            let mut batch = Vec::with_capacity(50);
            let mut flush_interval = tokio::time::interval(std::time::Duration::from_millis(500));
            flush_interval.tick().await;

            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => {
                        debug!("ChannelLogger received cancellation, draining queue...");
                        while let Ok(log) = rx.try_recv() {
                            batch.push(log);
                            if batch.len() >= 100 {
                                if let Err(e) = db.insert_channel_logs_batch(&batch).await {
                                    error!(error = %e, "Failed to flush channel logs batch during shutdown");
                                }
                                batch.clear();
                            }
                        }
                        if !batch.is_empty() {
                            if let Err(e) = db.insert_channel_logs_batch(&batch).await {
                                error!(error = %e, "Failed to flush final channel logs batch during shutdown");
                            }
                            batch.clear();
                        }
                        break;
                    }
                    opt = rx.recv() => {
                        match opt {
                            Some(log) => {
                                batch.push(log);
                                if batch.len() >= 50 {
                                    if let Err(e) = db.insert_channel_logs_batch(&batch).await {
                                        error!(error = %e, batch_size = batch.len(), "Failed to insert channel logs batch");
                                    }
                                    batch.clear();
                                }
                            }
                            None => {
                                // Channel closed
                                break;
                            }
                        }
                    }
                    _ = flush_interval.tick() => {
                        if !batch.is_empty() {
                            if let Err(e) = db.insert_channel_logs_batch(&batch).await {
                                error!(error = %e, batch_size = batch.len(), "Failed to flush channel logs on interval");
                            }
                            batch.clear();
                        }
                    }
                }
            }
            info!("ChannelLogger background worker stopped");
        });

        Arc::new(Self { tx })
    }

    /// Generic non-blocking log emission
    pub fn log(
        &self,
        broadcaster_id: impl Into<String>,
        level: ChannelLogLevel,
        category: ChannelLogCategory,
        event_type: impl Into<String>,
        message: impl Into<String>,
        details: Option<serde_json::Value>,
        solution_hint: Option<String>,
    ) {
        let entry = NewChannelLog {
            broadcaster_id: broadcaster_id.into(),
            level,
            category,
            event_type: event_type.into(),
            message: message.into(),
            details,
            solution_hint,
        };

        if let Err(mpsc::error::TrySendError::Full(_)) = self.tx.try_send(entry) {
            warn!("ChannelLogger buffer is full, dropping log entry");
        }
    }

    // =========================================================================
    // Specialized high-level logging helpers
    // =========================================================================

    pub fn log_redemption_order_created(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        item_name: &str,
        cost: i64,
        user_login: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Redemption,
            "REDEMPTION_ORDER_CREATED",
            format!("Created market purchase order for skin \"{}\" for viewer @{}", item_name, user_login),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "item_name": item_name,
                "points_cost": cost,
                "user_login": user_login,
            })),
            None,
        );
    }

    pub fn log_redemption_completed(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        item_name: Option<&str>,
        user_login: &str,
    ) {
        let item_title = item_name.unwrap_or("skin");
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Redemption,
            "REDEMPTION_COMPLETED",
            format!("Reward item delivered successfully to viewer @{} (\"{}\")", user_login, item_title),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "item_name": item_name,
                "user_login": user_login,
            })),
            None,
        );
    }

    pub fn log_trade_link_invalid(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        user_login: &str,
        trade_link: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Warn,
            ChannelLogCategory::Redemption,
            "TRADE_LINK_INVALID",
            format!("Viewer @{} provided an invalid or private Steam trade link", user_login),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "user_login": user_login,
                "trade_link": trade_link,
            })),
            Some("The viewer needs to set their Steam inventory to public and provide a valid Trade URL in reward input. Once resolved, you can retry the redemption manually in the rewards dashboard.".to_string()),
        );
    }

    pub fn log_market_balance_insufficient(
        &self,
        broadcaster_id: &str,
        required_price: Option<i64>,
        current_balance: Option<i64>,
        currency: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Error,
            ChannelLogCategory::Market,
            "MARKET_INSUFFICIENT_BALANCE",
            "Insufficient CSGO Market balance to purchase the requested skin",
            Some(serde_json::json!({
                "required_price": required_price,
                "current_balance": current_balance,
                "currency": currency,
            })),
            Some("Deposit funds into your CSGO Market account (market.csgo.com) so the bot can automatically purchase items when viewers redeem rewards.".to_string()),
        );
    }

    pub fn log_market_buy_error(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        item_name: &str,
        error_kind: &str,
        raw_error: &str,
    ) {
        let (level, hint) = match error_kind {
            "no_money" => (
                ChannelLogLevel::Error,
                "Deposit funds into your CSGO Market account (market.csgo.com). The redemption can be retried manually after topping up balance."
            ),
            "temporary_out_of_stock" => (
                ChannelLogLevel::Warn,
                "Item is temporarily not available at the target price on the market. Wait for new listings or increase the permissible price deviation percentage in reward settings."
            ),
            "buyer_banned" => (
                ChannelLogLevel::Warn,
                "Viewer has trade restrictions on Steam (password change / Steam Guard hold / trade ban). Refund channel points to the viewer."
            ),
            "invalid_trade_url" => (
                ChannelLogLevel::Warn,
                "Viewer's Steam trade link is invalid or expired. Ask the viewer to update their Trade URL and retry."
            ),
            _ => (
                ChannelLogLevel::Error,
                "Check CSGO Market availability and verify that Market API Key is valid in channel settings."
            ),
        };

        self.log(
            broadcaster_id,
            level,
            ChannelLogCategory::Market,
            "MARKET_BUY_FAILED",
            format!("Market item purchase failed for skin \"{}\": {}", item_name, error_kind),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "item_name": item_name,
                "error_kind": error_kind,
                "raw_error": raw_error,
            })),
            Some(hint.to_string()),
        );
    }

    pub fn log_reward_paused(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        reason: &str,
        details: Option<serde_json::Value>,
    ) {
        let (message, hint) = match reason {
            "NO_MONEY" => (
                format!("Reward \"{}\" was automatically paused: insufficient balance on CSGO Market", reward_title),
                "Deposit funds into your CSGO Market account. The reward will resume automatically during the next price update cycle, or you can unpause it manually."
            ),
            "PRICE_LIMIT" => (
                format!("Reward \"{}\" was automatically paused: market price exceeded configured limits", reward_title),
                "Check the current market price of the skin on CSGO Market and adjust maximum price limits in reward settings if desired."
            ),
            "TEMPORARY_OUT_OF_STOCK" => (
                format!("Reward \"{}\" was temporarily paused: item is not available on market within target price range", reward_title),
                "Wait for new listings on the market or increase permissible price deviation percentage in reward settings."
            ),
            _ => (
                format!("Reward \"{}\" was paused (reason: {})", reward_title, reason),
                "Check reward settings and market status in channel management dashboard."
            ),
        };

        let mut log_details = details.unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = log_details.as_object_mut() {
            obj.insert("reward_id".to_string(), serde_json::Value::String(reward_id.to_string()));
            obj.insert("reward_title".to_string(), serde_json::Value::String(reward_title.to_string()));
            obj.insert("reason".to_string(), serde_json::Value::String(reason.to_string()));
        }

        self.log(
            broadcaster_id,
            ChannelLogLevel::Warn,
            ChannelLogCategory::Reward,
            "REWARD_AUTO_PAUSED",
            message,
            Some(log_details),
            Some(hint.to_string()),
        );
    }

    pub fn log_reward_price_updated(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        old_cost: i64,
        new_cost: i64,
        market_price: i64,
        currency: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Debug,
            ChannelLogCategory::Reward,
            "REWARD_PRICE_UPDATED",
            format!("Updated reward cost for \"{}\": {} -> {} Channel Points (market price: {} {})", reward_title, old_cost, new_cost, market_price, currency),
            Some(serde_json::json!({
                "reward_id": reward_id,
                "reward_title": reward_title,
                "old_cost": old_cost,
                "new_cost": new_cost,
                "market_price": market_price,
                "currency": currency,
            })),
            None,
        );
    }

    pub fn log_chat_send_error(
        &self,
        broadcaster_id: &str,
        error_msg: &str,
        drop_reason_code: Option<&str>,
        drop_reason_msg: Option<&str>,
    ) {
        let is_restricted = drop_reason_code.map(|c| {
            c.contains("follower") || c.contains("sub") || c.contains("slow") || c.contains("rate") || c.contains("restricted")
        }).unwrap_or(false);

        let hint = if is_restricted || error_msg.to_lowercase().contains("follower") || error_msg.to_lowercase().contains("rate") {
            "Grant the bot VIP (/vip <bot_name>) or Moderator (/mod <bot_name>) status in your Twitch channel chat. This allows the bot to send chat notifications bypassing followers-only, subs-only, slow mode restrictions, and Twitch rate limits."
        } else {
            "Check the bot account status in your Twitch channel chat. Granting VIP (/vip) or Moderator (/mod) status to the bot is strongly recommended for uninterrupted notifications."
        };

        self.log(
            broadcaster_id,
            ChannelLogLevel::Warn,
            ChannelLogCategory::Bot,
            "BOT_CHAT_MESSAGE_DROPPED",
            format!("Chat bot failed to send message to channel chat: {}", error_msg),
            Some(serde_json::json!({
                "error": error_msg,
                "drop_code": drop_reason_code,
                "drop_message": drop_reason_msg,
            })),
            Some(hint.to_string()),
        );
    }

    pub fn log_broadcaster_token_error(&self, broadcaster_id: &str, error_msg: &str) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Error,
            ChannelLogCategory::Auth,
            "BROADCASTER_TOKEN_INVALID",
            format!("Broadcaster Twitch OAuth token error: {}", error_msg),
            Some(serde_json::json!({ "error": error_msg })),
            Some("Your Twitch channel authorization has expired or was revoked. Reconnect your channel in authorization settings (/api/v1/auth/connect).".to_string()),
        );
    }

    pub fn log_chat_requirement_failed(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        user_login: &str,
        reason: &str,
        is_refund: bool,
    ) {
        let action_text = if is_refund { "channel points refunded" } else { "channel points penalized" };
        self.log(
            broadcaster_id,
            ChannelLogLevel::Warn,
            ChannelLogCategory::Redemption,
            "CHAT_REQUIREMENT_FAILED",
            format!("Viewer @{} did not meet chat activity requirements ({}), {}", user_login, reason, action_text),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "user_login": user_login,
                "reason": reason,
                "refunded": is_refund,
            })),
            Some("The viewer did not have enough chat messages or characters within the configured time window. Chat requirements can be adjusted in reward properties.".to_string()),
        );
    }

    pub fn log_purchase_limit_reached(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        user_login: &str,
        limit_type: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Redemption,
            "PURCHASE_LIMIT_REACHED",
            format!("Redemption from viewer @{} was rejected: purchase limit reached ({})", user_login, limit_type),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "user_login": user_login,
                "limit_type": limit_type,
            })),
            Some("Reward redemption limit was reached (personal or global). Channel points were returned to the viewer.".to_string()),
        );
    }
}
