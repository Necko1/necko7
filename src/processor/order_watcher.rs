use std::sync::Arc;
use std::time::Duration;
use chrono::DateTime;
use tokio::time::Interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::datetime::DateTimeExt;
use crate::db::redemptions::RedemptionStatus;
use crate::messages::{
    MSG_TRADE_ACCEPTED, MSG_TRADE_CREATED, MSG_TRADE_FAILED_BUYER_PENALTY,
    MSG_TRADE_FAILED_BUYER_REFUND, MSG_TRADE_FAILED_SELLER_REFUND, MSG_TRADE_TIMEOUT,
};
use crate::state::AppState;
use crate::steam::market::sell_buy::GetBuyInfoData;

enum OrderStage {
    Pending,
    Sent,
    Claimed,
    NotClaimed,

    Exit,
}

pub struct WatcherRedemptionData {
    pub redemption_id: Uuid,
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

    pub async fn track_redemption(mut self) {
        self.interval.tick().await;

        loop {
            self.interval.tick().await;

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
                &self.api_key, &self.redemption.redemption_id).await
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
        if current_trade.receive_until.is_none() {
            self.process_not_claimed(current_trade).await;
            return;
        }

        if current_trade.receive_until == Some(DateTime::UNIX_EPOCH)
            || current_trade.trade_id.is_none() { return; }

        self.stage = OrderStage::Sent;
        info!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            trade_id = ?current_trade.trade_id,
            "Steam trade offer detected, transitioning to Sent stage"
        );

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        let remaining = current_trade.receive_until.unwrap().remaining_pretty();
        let tradeoffer = format!("https://steamcommunity.com/tradeoffer/{}/",
                                 current_trade.trade_id.unwrap());

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_CREATED,
            &[
                ("buyer", &self.redemption.user_login),
                ("remaining", &remaining),
                ("tradeoffer", &tradeoffer),
            ],
        );

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &msg,
                None, None,
                &token).await
        }).await {
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
        if current_trade.settlement.is_none() {
            self.process_not_claimed(current_trade).await;
            return;
        }

        if current_trade.settlement == Some(DateTime::UNIX_EPOCH) { return; }

        self.stage = OrderStage::Claimed;
        info!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            "Steam trade accepted by user! Transitioning to Claimed stage"
        );

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_ACCEPTED,
            &[("buyer", &self.redemption.user_login)],
        );

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &msg,
                None, None,
                &token).await
        }).await {
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
            error!(error = %e, redemption_id = %self.redemption.redemption_id, reward_id = %self.redemption.reward_id, broadcaster_id = %self.broadcaster_id, "Failed to fulfill redemption on Twitch Helix");
            return;
        };

        let state_for_balance = self.state.clone();
        let bc_id_for_balance = self.broadcaster_id.clone();
        tokio::spawn(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });
    }

    async fn process_not_claimed(&mut self, current_trade: GetBuyInfoData) {
        if !current_trade.stage.eq("5") { // TRADE_STAGE_TIMED_OUT
            return;
        }

        self.stage = OrderStage::NotClaimed;

        let refund_on_buyer_fail = match self.state.db.get_broadcaster_setting(&self.broadcaster_id).await {
            Ok(Some(s)) => s.refund_on_buyer_fail,
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

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        let buyer_fault = current_trade.causer.is_some_and(|c| c.eq("buyer"));
        let should_refund = !buyer_fault || refund_on_buyer_fail;

        warn!(
            redemption_id = %self.redemption.redemption_id,
            user_login = %self.redemption.user_login,
            buyer_fault,
            refund_on_buyer_fail,
            should_refund,
            "Trade was not claimed / timed out on market"
        );

        let msg_id = if refund_on_buyer_fail && buyer_fault {
            MSG_TRADE_FAILED_BUYER_REFUND
        } else if !refund_on_buyer_fail && buyer_fault {
            MSG_TRADE_FAILED_BUYER_PENALTY
        } else {
            MSG_TRADE_FAILED_SELLER_REFUND
        };

        let message = self.state.render_chat_message(
            &self.broadcaster_id,
            msg_id,
            &[("buyer", &self.redemption.user_login)],
        );

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &message,
                None, None,
                &token).await
        }).await {
            error!(error = %e, redemption_id = %self.redemption.redemption_id, broadcaster_id = %self.broadcaster_id, "Failed to send not-claimed chat message");
        };

        let redemption_status = if should_refund { RedemptionStatus::FailedRefund }
        else { RedemptionStatus::FailedPenalty };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            redemption_status,
            None, None
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
            error!(error = %e, redemption_id = %self.redemption.redemption_id, reward_id = %self.redemption.reward_id, broadcaster_id = %self.broadcaster_id, "Failed to update redemption status on Twitch Helix");
            return;
        };

        let state_for_balance = self.state.clone();
        let bc_id_for_balance = self.broadcaster_id.clone();
        tokio::spawn(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });
    }

    async fn process_timed_out(&mut self) {
        self.stage = OrderStage::Exit;

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => return,
        };

        let msg = self.state.render_chat_message(
            &self.broadcaster_id,
            MSG_TRADE_TIMEOUT,
            &[("buyer", &self.redemption.user_login)],
        );

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &msg,
                None, None,
                &token).await
        }).await {
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
        tokio::spawn(async move {
            let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
        });
    }

    fn get_sender_id(&mut self) -> Option<String> {
        let guard = self.state.bot_info.read();
        let info = match guard.as_ref() {
            Some(i) => i,
            None => {
                error!(
                    redemption_id = %self.redemption.redemption_id,
                    broadcaster_id = %self.broadcaster_id,
                    "Bot account is not initialized in AppState during order tracking"
                );
                self.stage = OrderStage::Exit;
                return None;
            }
        };

        Some(info.user_id.clone())
    }
}