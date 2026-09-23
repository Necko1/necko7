use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use uuid::Uuid;

use crate::db::redemptions::RedemptionStatus;
use crate::messages::{MSG_ORDERS_RECONCILIATION_REQUIRED, MSG_TRADES_ACCEPTED};
use crate::processor::inventory_fulfillment::{announce_trade, send_inventory_chat, terminal_trade_template};
use crate::state::AppState;

pub struct WatcherRedemptionData {
    pub redemption_id: Uuid,
    pub custom_id: String,
    pub reward_id: Uuid,
    pub user_login: String,
}

pub struct OrderWatcher {
    state: Arc<AppState>,
    api_key: String,
    broadcaster_id: String,
    redemption: WatcherRedemptionData,
}

impl OrderWatcher {
    pub fn new(state: Arc<AppState>, api_key: String, broadcaster_id: String, redemption: WatcherRedemptionData) -> Self {
        Self { state, api_key, broadcaster_id, redemption }
    }

    pub async fn track_redemption(self, token: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        let started = Instant::now();
        interval.tick().await;
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = interval.tick() => {}
            }
            if started.elapsed() > Duration::from_secs(30 * 60) {
                // A local polling deadline does not prove the Market order ended.
                match self.state.db.require_inventory_reconciliation(self.redemption.redemption_id, &self.redemption.custom_id).await {
                    Ok(true) => {
                        if let Ok(Some(item)) = self.state.db.get_inventory_core(self.redemption.redemption_id).await {
                            send_inventory_chat(&self.state, &self.broadcaster_id, MSG_ORDERS_RECONCILIATION_REQUIRED,
                                &self.redemption.user_login, &item.1, &[]).await;
                        }
                    }
                    Ok(false) => {}
                    Err(e) => error!(error = %e, "Could not persist watcher timeout uncertainty"),
                }
                return;
            }
            let info = match self.state.market_client.get_buy_info(&self.api_key, &self.redemption.custom_id).await {
                Ok(info) if info.success => info,
                Ok(_) => continue,
                Err(e) => { warn!(error = %e, "Market order polling failed"); continue; }
            };
            let Some(trade) = info.data else { continue; };
            let inventory = match self.state.db.get_inventory_core(self.redemption.redemption_id).await {
                Ok(item) => item,
                Err(e) => { error!(error = %e, "Cannot identify inventory ownership for watcher result"); continue; }
            };
            let managed_inventory = matches!(&inventory, Some(item) if item.4 != "LEGACY_REVIEW");
            let legacy_inventory = matches!(&inventory, Some(item) if item.4 == "LEGACY_REVIEW");
            if managed_inventory && !matches!(&inventory, Some(item) if item.1 == trade.market_hash_name) {
                let _ = self.state.db.require_inventory_reconciliation(self.redemption.redemption_id, &self.redemption.custom_id).await;
                return;
            }
            if trade.is_claimed() {
                if managed_inventory {
                    if let Err(e) = self.state.db.attach_inventory_order(self.redemption.redemption_id, &self.redemption.custom_id,
                        Some(&trade.item_id), &trade.market_hash_name).await {
                        error!(error = %e, "Could not attach delivered Market order");
                        continue;
                    }
                }
                let newly_delivered = if managed_inventory {
                    match self.state.db.mark_inventory_delivered(self.redemption.redemption_id, &self.redemption.custom_id).await {
                        Ok(value) => value,
                        Err(e) => { error!(error = %e, "Could not persist delivery"); continue; }
                    }
                } else {
                    self.state.db.update_redemption_status(self.redemption.redemption_id, RedemptionStatus::Completed, None, None).await.is_ok()
                };
                if legacy_inventory && newly_delivered {
                    let _ = self.state.db.mark_legacy_inventory_delivered(self.redemption.redemption_id).await;
                }
                if managed_inventory {
                    if newly_delivered {
                        send_inventory_chat(&self.state, &self.broadcaster_id, MSG_TRADES_ACCEPTED,
                            &self.redemption.user_login, &trade.market_hash_name, &[]).await;
                    }
                    if let Err(e) = crate::processor::inventory_fulfillment::fulfill_delivered_twitch(&self.state, self.redemption.redemption_id).await {
                        error!(error = %e, "Twitch delivery fulfillment remains pending for recovery");
                    }
                } else if newly_delivered {
                    if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
                        self.state.helix_client.update_redemption_status(&self.broadcaster_id,
                            &self.redemption.reward_id.to_string(), &self.redemption.redemption_id.to_string(), false, &token).await
                    }).await { error!(error = %e, "Legacy Twitch delivery fulfillment needs reconciliation"); }
                }
                return;
            }
            if trade.is_failed() {
                if trade.stage != "5" {
                    if managed_inventory {
                        let _ = self.state.db.require_inventory_reconciliation(self.redemption.redemption_id, &self.redemption.custom_id).await;
                    }
                    return;
                }
                if managed_inventory {
                    match self.state.db.set_terminal_trade_failure(self.redemption.redemption_id, &self.redemption.custom_id,
                        trade.causer.as_deref() == Some("buyer"), trade.causer.as_deref(), trade.cancellation_reason.as_deref()).await {
                        Ok(true) => send_inventory_chat(&self.state, &self.broadcaster_id,
                            terminal_trade_template(trade.causer.as_deref() == Some("buyer")),
                            &self.redemption.user_login, &trade.market_hash_name, &[]).await,
                        Ok(false) => {}
                        Err(e) => { error!(error = %e, "Could not persist terminal Market failure"); continue; }
                    }
                } else {
                    let _ = self.state.db.set_redemption_manual_hold(self.redemption.redemption_id, "market_terminal_failure", trade.cancellation_reason.as_deref()).await;
                }
                return;
            }
            if managed_inventory {
                let _ = self.state.db.attach_inventory_order(self.redemption.redemption_id, &self.redemption.custom_id,
                    Some(&trade.item_id), &trade.market_hash_name).await;
                if trade.has_active_trade() {
                    match self.state.db.set_trade_waiting(self.redemption.redemption_id, &self.redemption.custom_id,
                        trade.trade_id.as_deref(), trade.send_until, trade.receive_until).await {
                        Ok(true) => {
                            if let Some(trade_id) = trade.trade_id.as_deref() {
                                announce_trade(&self.state, &self.broadcaster_id, &self.redemption.user_login,
                                    &trade.market_hash_name, trade_id, trade.receive_until).await;
                            }
                        }
                        Ok(false) => {}
                        Err(e) => error!(error = %e, "Could not persist Steam trade state"),
                    }
                }
            }
        }
    }
}
