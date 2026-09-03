use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use crate::state::AppState;

pub struct BalanceUpdater {
    state: Arc<AppState>,
    broadcaster_id: String,
}

impl BalanceUpdater {
    pub fn new(state: Arc<AppState>, broadcaster_id: String) -> Self {
        Self {
            state,
            broadcaster_id,
        }
    }

    pub async fn run(self) {
        info!(broadcaster_id = %self.broadcaster_id, "Starting periodic balance updater task for broadcaster");

        let mut interval = tokio::time::interval(Duration::from_secs(300));
        interval.tick().await;

        loop {
            interval.tick().await;

            let setting = match self.state.db.get_broadcaster_setting(&self.broadcaster_id).await {
                Ok(Some(s)) => s,
                Ok(None) => continue,
                Err(e) => {
                    tracing::error!(error = %e, broadcaster_id = %self.broadcaster_id, "DB error fetching broadcaster setting for balance updater");
                    continue;
                }
            };

            if setting.is_active && !setting.market_api_key.trim().is_empty() {
                if let Err(e) = self.state.refresh_broadcaster_balance(&self.broadcaster_id).await {
                    warn!(error = %e, broadcaster_id = %self.broadcaster_id, "Periodic balance refresh failed");
                }
            }
        }
    }
}
