use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use uuid::Uuid;

use crate::db::redemptions::RedemptionStatus;
use crate::state::AppState;

pub struct WatcherRedemptionData {
    pub redemption_id: Uuid,
    pub custom_id: String,
    pub reward_id: Uuid,
    pub activated_at: DateTime<Utc>,
}

/// Compatibility watcher for pre-inventory orders without a durable attempt.
/// Live inventory attempts use the database-backed scheduler instead.
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
        loop {
            let delay = if Utc::now() - self.redemption.activated_at < chrono::Duration::minutes(30) { 60 } else { 300 };
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
            }
            let info = match self.state.market_client.get_buy_info(&self.api_key, &self.redemption.custom_id).await {
                Ok(info) if info.success => info,
                Ok(_) => continue,
                Err(e) => { warn!(error = %e, "Legacy Market order polling failed"); continue; }
            };
            let Some(trade) = info.data else { continue; };
            if self.state.db.get_inventory_core(self.redemption.redemption_id).await
                .ok().flatten().is_some_and(|item| item.4 != "LEGACY_REVIEW") {
                warn!(redemption_id = %self.redemption.redemption_id, "Live inventory order reached legacy watcher; stopping");
                return;
            }
            match trade.stage.as_str() {
                "2" => {
                    if let Err(e) = self.state.db.update_redemption_status(self.redemption.redemption_id,
                        RedemptionStatus::Completed, None, None).await {
                        error!(error = %e, "Could not record legacy final Market delivery");
                        continue;
                    }
                    let _ = self.state.db.mark_legacy_inventory_delivered(self.redemption.redemption_id).await;
                    if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
                        self.state.helix_client.update_redemption_status(&self.broadcaster_id,
                            &self.redemption.reward_id.to_string(), &self.redemption.redemption_id.to_string(), false, &token).await
                    }).await { error!(error = %e, "Legacy Twitch delivery fulfillment needs reconciliation"); }
                    return;
                }
                "5" => {
                    let _ = self.state.db.set_redemption_manual_hold(self.redemption.redemption_id,
                        "market_terminal_failure", trade.cancellation_reason.as_deref()).await;
                    return;
                }
                _ => {}
            }
        }
    }
}
