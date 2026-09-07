use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use crate::db::channel_logs::{ChannelLogCategory, ChannelLogLevel, NewChannelLog};
use crate::db::Db;
use crate::steam::market;

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
        paid_price_minor: i64,
        currency: &str,
        user_login: &str,
    ) {
        let paid_price_major = market::minor_to_major(paid_price_minor, currency);
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Redemption,
            "REDEMPTION_ORDER_CREATED",
            format!("Created market purchase order for skin \"{}\" for viewer @{} (price: {:.2} {})", item_name, user_login, paid_price_major, currency),
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "item_name": item_name,
                "market_price": paid_price_major,
                "market_price_minor": paid_price_minor,
                "currency": currency,
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
        required_price_minor: Option<i64>,
        current_balance: Option<f64>,
        currency: &str,
    ) {
        let req_major = required_price_minor.map(|p| market::minor_to_major(p, currency));
        let price_msg = match (req_major, current_balance) {
            (Some(req), Some(bal)) => format!(" (required: {:.2} {}, balance: {:.2} {})", req, currency, bal, currency),
            (Some(req), None) => format!(" (required: {:.2} {})", req, currency),
            _ => String::new(),
        };

        self.log(
            broadcaster_id,
            ChannelLogLevel::Error,
            ChannelLogCategory::Market,
            "MARKET_INSUFFICIENT_BALANCE",
            format!("Insufficient CSGO Market balance to purchase the requested skin{}", price_msg),
            Some(serde_json::json!({
                "required_price": req_major,
                "required_price_minor": required_price_minor,
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
            "inventory_hidden" => (
                ChannelLogLevel::Warn,
                "Viewer's Steam inventory or profile is private. Ask the viewer to set their inventory to Public in Steam Privacy Settings before retrying."
            ),
            "inventory_full" => (
                ChannelLogLevel::Warn,
                "Viewer's Steam inventory is full (reached 1000 items limit). The viewer needs to free up inventory space before retrying."
            ),
            "network_error" => (
                ChannelLogLevel::Error,
                "CSGO Market API is currently unreachable. Check your server internet connection or CSGO Market service status."
            ),
            "market_retry_failed" => (
                ChannelLogLevel::Warn,
                "Manual purchase retry was rejected by CSGO Market. Verify item availability or check error message details."
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
            "PRICE_LIMIT" => {
                let curr = details.as_ref().and_then(|d| d.get("currency")).and_then(|v| v.as_str()).unwrap_or("");
                let price_text = if let Some(p) = details.as_ref().and_then(|d| d.get("current_price")).and_then(|v| v.as_f64()) {
                    format!(" ({:.2} {})", p, curr)
                } else {
                    String::new()
                };
                (
                    format!("Reward \"{}\" was automatically paused: market price{} exceeded configured limits", reward_title, price_text),
                    "Check the current market price of the skin on CSGO Market and adjust maximum price limits in reward settings if desired."
                )
            },
            "LIMIT_REACHED" => (
                format!("Reward \"{}\" was automatically paused: global purchase limit reached", reward_title),
                "The reward will automatically resume when the time window passes, or you can adjust/remove purchase limits in reward settings."
            ),
            "MANUAL" => {
                let actor_info = if let (Some(login), Some(id)) = (
                    details.as_ref().and_then(|d| d.get("actor_user_login")).and_then(|v| v.as_str()),
                    details.as_ref().and_then(|d| d.get("actor_user_id")).and_then(|v| v.as_str()),
                ) {
                    format!(" by @{} (ID: {})", login, id)
                } else {
                    " by channel editor".to_string()
                };
                (
                    format!("Reward \"{}\" was paused manually{}", reward_title, actor_info),
                    "The reward is currently hidden from viewers. You can unpause it at any time in reward settings."
                )
            },
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

        let (event_type, level) = if reason == "MANUAL" {
            ("REWARD_MANUALLY_PAUSED", ChannelLogLevel::Info)
        } else {
            ("REWARD_AUTO_PAUSED", ChannelLogLevel::Warn)
        };

        self.log(
            broadcaster_id,
            level,
            ChannelLogCategory::Reward,
            event_type,
            message,
            Some(log_details),
            Some(hint.to_string()),
        );
    }

    pub fn log_reward_unpaused(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        reason: &str,
        details: Option<serde_json::Value>,
    ) {
        let (message, hint) = match reason {
            "NO_MONEY" => (
                format!("Reward \"{}\" was automatically unpaused: CSGO Market balance restored", reward_title),
                "Market balance is now sufficient. Viewers can redeem this reward again."
            ),
            "PRICE_LIMIT" => (
                format!("Reward \"{}\" was automatically unpaused: market price returned within configured limits", reward_title),
                "Skin price on the market is back within your configured min/max range."
            ),
            "LIMIT_REACHED" => (
                format!("Reward \"{}\" was automatically unpaused: purchase limit time window has passed", reward_title),
                "Purchase count is back below the limit. Viewers can redeem this reward again."
            ),
            "MANUAL" => {
                let actor_info = if let (Some(login), Some(id)) = (
                    details.as_ref().and_then(|d| d.get("actor_user_login")).and_then(|v| v.as_str()),
                    details.as_ref().and_then(|d| d.get("actor_user_id")).and_then(|v| v.as_str()),
                ) {
                    format!(" by @{} (ID: {})", login, id)
                } else {
                    " by channel editor".to_string()
                };
                (
                    format!("Reward \"{}\" was unpaused manually{}", reward_title, actor_info),
                    "The reward is now active and available to viewers in chat."
                )
            },
            _ => (
                format!("Reward \"{}\" was unpaused (reason: {})", reward_title, reason),
                "The reward is now active on your channel."
            ),
        };

        let mut log_details = details.unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = log_details.as_object_mut() {
            obj.insert("reward_id".to_string(), serde_json::Value::String(reward_id.to_string()));
            obj.insert("reward_title".to_string(), serde_json::Value::String(reward_title.to_string()));
            obj.insert("reason".to_string(), serde_json::Value::String(reason.to_string()));
        }

        let event_type = if reason == "MANUAL" {
            "REWARD_MANUALLY_UNPAUSED"
        } else {
            "REWARD_AUTO_UNPAUSED"
        };

        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Reward,
            event_type,
            message,
            Some(log_details),
            Some(hint.to_string()),
        );
    }

    pub fn log_redemption_manual_action(
        &self,
        broadcaster_id: &str,
        redemption_id: &str,
        user_login: &str,
        item_name: Option<&str>,
        action: &str,
        actor_user_id: &str,
        actor_user_login: &str,
    ) {
        let item_title = item_name.unwrap_or("skin");
        let (event_type, message) = match action {
            "REFUND" => (
                "REDEMPTION_MANUALLY_REFUNDED",
                format!("Redemption for @{} (\"{}\") was manually refunded by @{} (ID: {})", user_login, item_title, actor_user_login, actor_user_id),
            ),
            "PENALTY" => (
                "REDEMPTION_MANUALLY_PENALIZED",
                format!("Redemption for @{} (\"{}\") was manually penalized by @{} (ID: {})", user_login, item_title, actor_user_login, actor_user_id),
            ),
            "RETRY" => (
                "REDEMPTION_MANUALLY_RETRIED",
                format!("Redemption purchase for @{} (\"{}\") was manually retried on market by @{} (ID: {})", user_login, item_title, actor_user_login, actor_user_id),
            ),
            _ => (
                "REDEMPTION_MANUAL_ACTION",
                format!("Redemption for @{} (\"{}\") was modified ({}) by @{} (ID: {})", user_login, item_title, action, actor_user_login, actor_user_id),
            ),
        };

        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Redemption,
            event_type,
            message,
            Some(serde_json::json!({
                "redemption_id": redemption_id,
                "user_login": user_login,
                "item_name": item_name,
                "action": action,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_reward_manually_created(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        reward_type: &str,
        pricing_mode: &str,
        actor_user_id: &str,
        actor_user_login: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Reward,
            "REWARD_MANUALLY_CREATED",
            format!("Reward \"{}\" ({}, {}) was created by @{} (ID: {})", reward_title, reward_type, pricing_mode, actor_user_login, actor_user_id),
            Some(serde_json::json!({
                "reward_id": reward_id,
                "reward_title": reward_title,
                "reward_type": reward_type,
                "pricing_mode": pricing_mode,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_reward_manually_updated(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        actor_user_id: &str,
        actor_user_login: &str,
        updated_fields: Vec<String>,
    ) {
        let fields_str = if updated_fields.is_empty() {
            "settings".to_string()
        } else {
            updated_fields.join(", ")
        };
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Reward,
            "REWARD_MANUALLY_UPDATED",
            format!("Reward \"{}\" settings ({}) were updated by @{} (ID: {})", reward_title, fields_str, actor_user_login, actor_user_id),
            Some(serde_json::json!({
                "reward_id": reward_id,
                "reward_title": reward_title,
                "updated_fields": updated_fields,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_reward_manually_deleted(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: &str,
        actor_user_id: &str,
        actor_user_login: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Reward,
            "REWARD_MANUALLY_DELETED",
            format!("Reward \"{}\" was deleted by @{} (ID: {})", reward_title, actor_user_login, actor_user_id),
            Some(serde_json::json!({
                "reward_id": reward_id,
                "reward_title": reward_title,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_settings_manually_updated(
        &self,
        broadcaster_id: &str,
        actor_user_id: &str,
        actor_user_login: &str,
        changed_settings: Vec<String>,
    ) {
        let changes_str = if changed_settings.is_empty() {
            "settings".to_string()
        } else {
            changed_settings.join(", ")
        };
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::System,
            "SETTINGS_MANUALLY_UPDATED",
            format!("Channel settings ({}) were updated by @{} (ID: {})", changes_str, actor_user_login, actor_user_id),
            Some(serde_json::json!({
                "changed_settings": changed_settings,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_chat_messages_manually_updated(
        &self,
        broadcaster_id: &str,
        actor_user_id: &str,
        actor_user_login: &str,
    ) {
        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::System,
            "CHAT_TEMPLATES_MANUALLY_UPDATED",
            format!("Chat message templates were updated by @{} (ID: {})", actor_user_login, actor_user_id),
            Some(serde_json::json!({
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_permission_manually_changed(
        &self,
        broadcaster_id: &str,
        action: &str,
        target_user_id: &str,
        target_user_login: &str,
        actor_user_id: &str,
        actor_user_login: &str,
    ) {
        let (event_type, msg) = match action {
            "GRANT" => (
                "PERMISSION_GRANTED",
                format!("Editor permission was granted to @{} (ID: {}) by @{} (ID: {})", target_user_login, target_user_id, actor_user_login, actor_user_id),
            ),
            "REVOKE" => (
                "PERMISSION_REVOKED",
                format!("Editor permission was revoked from @{} (ID: {}) by @{} (ID: {})", target_user_login, target_user_id, actor_user_login, actor_user_id),
            ),
            _ => (
                "PERMISSION_CHANGED",
                format!("Permissions for @{} (ID: {}) were modified ({}) by @{} (ID: {})", target_user_login, target_user_id, action, actor_user_login, actor_user_id),
            ),
        };

        self.log(
            broadcaster_id,
            ChannelLogLevel::Info,
            ChannelLogCategory::Auth,
            event_type,
            msg,
            Some(serde_json::json!({
                "action": action,
                "target_user_id": target_user_id,
                "target_user_login": target_user_login,
                "actor_user_id": actor_user_id,
                "actor_user_login": actor_user_login,
            })),
            None,
        );
    }

    pub fn log_reward_misconfigured(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_title: Option<&str>,
        error_type: &str,
        details: Option<serde_json::Value>,
    ) {
        let title = reward_title.unwrap_or("Reward");
        let (message, hint) = match error_type {
            "EMPTY_POOL" => (
                format!("{} pool item list is empty; redemptions cannot be fulfilled", title),
                "Add items to the pool in reward settings, or switch the reward type to Fixed skin or Price Filter."
            ),
            "FILTER_NO_MATCH" => (
                format!("{} price filter did not match any available skins on CSGO Market", title),
                "Check prices.json availability or broaden your min/max price range and name criteria in reward settings."
            ),
            "FILTER_MISSING_CONFIG" => (
                format!("{} is configured as Filter reward but has no filter configuration", title),
                "Configure price and name criteria in reward settings."
            ),
            _ => (
                format!("{} configuration error: {}", title, error_type),
                "Check reward settings in channel management dashboard."
            ),
        };

        let mut log_details = details.unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = log_details.as_object_mut() {
            obj.insert("reward_id".to_string(), serde_json::Value::String(reward_id.to_string()));
            if let Some(t) = reward_title {
                obj.insert("reward_title".to_string(), serde_json::Value::String(t.to_string()));
            }
            obj.insert("error_type".to_string(), serde_json::Value::String(error_type.to_string()));
        }

        self.log(
            broadcaster_id,
            ChannelLogLevel::Error,
            ChannelLogCategory::Reward,
            "REWARD_CONFIG_ERROR",
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
        pricing_mode: crate::db::rewards::PricingMode,
        currency: &str,
        old_market_price_minor: i32,
        new_market_price_minor: i32,
        old_channel_points: Option<u32>,
        new_channel_points: Option<u32>,
        manual_channel_points: Option<i32>,
    ) {
        let old_market_price_major = market::minor_to_major(old_market_price_minor as i64, currency);
        let new_market_price_major = market::minor_to_major(new_market_price_minor as i64, currency);

        match pricing_mode {
            crate::db::rewards::PricingMode::Manual => {
                let pts = manual_channel_points.unwrap_or(0);
                let message = if old_market_price_minor != new_market_price_minor {
                    format!(
                        "Updated internal market price for \"{}\": {:.2} -> {:.2} {} (Channel Points: {})",
                        reward_title, old_market_price_major, new_market_price_major, currency, pts
                    )
                } else {
                    format!(
                        "Internal market price for \"{}\" is {:.2} {} (Channel Points: {})",
                        reward_title, new_market_price_major, currency, pts
                    )
                };

                self.log(
                    broadcaster_id,
                    ChannelLogLevel::Debug,
                    ChannelLogCategory::Reward,
                    "REWARD_PRICE_UPDATED",
                    message,
                    Some(serde_json::json!({
                        "reward_id": reward_id,
                        "reward_title": reward_title,
                        "pricing_mode": "MANUAL",
                        "currency": currency,
                        "old_market_price": old_market_price_major,
                        "new_market_price": new_market_price_major,
                        "old_market_price_minor": old_market_price_minor,
                        "new_market_price_minor": new_market_price_minor,
                        "manual_channel_points": pts,
                    })),
                    None,
                );
            }
            crate::db::rewards::PricingMode::Auto => {
                let new_pts = new_channel_points.unwrap_or(0);
                let message = if let Some(old_pts) = old_channel_points {
                    if old_pts != new_pts {
                        format!(
                            "Updated reward cost for \"{}\": {} -> {} Channel Points (market price: {:.2} -> {:.2} {})",
                            reward_title, old_pts, new_pts, old_market_price_major, new_market_price_major, currency
                        )
                    } else if old_market_price_minor != new_market_price_minor {
                        format!(
                            "Updated reward market price for \"{}\": {:.2} -> {:.2} {} (Channel Points: {})",
                            reward_title, old_market_price_major, new_market_price_major, currency, new_pts
                        )
                    } else {
                        format!(
                            "Reward cost for \"{}\" is {} Channel Points (market price: {:.2} {})",
                            reward_title, new_pts, new_market_price_major, currency
                        )
                    }
                } else {
                    format!(
                        "Updated reward cost for \"{}\": {} Channel Points (market price: {:.2} {})",
                        reward_title, new_pts, new_market_price_major, currency
                    )
                };

                self.log(
                    broadcaster_id,
                    ChannelLogLevel::Debug,
                    ChannelLogCategory::Reward,
                    "REWARD_PRICE_UPDATED",
                    message,
                    Some(serde_json::json!({
                        "reward_id": reward_id,
                        "reward_title": reward_title,
                        "pricing_mode": "AUTO",
                        "currency": currency,
                        "old_market_price": old_market_price_major,
                        "new_market_price": new_market_price_major,
                        "old_market_price_minor": old_market_price_minor,
                        "new_market_price_minor": new_market_price_minor,
                        "old_channel_points": old_channel_points,
                        "new_channel_points": new_pts,
                    })),
                    None,
                );
            }
        }
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
