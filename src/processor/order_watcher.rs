use std::sync::Arc;
use std::time::Duration;
use chrono::DateTime;
use tokio::time::Interval;
use tracing::{error, warn};
use uuid::Uuid;
use crate::datetime::DateTimeExt;
use crate::db::redemptions::RedemptionStatus;
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
                    warn!(error = %err, "error getting buy info");
                    continue
                }
            };

            if let Some(err) = current_trade_info.error {
                error!("error getting buy info: {}", err);
            } else if !current_trade_info.success || current_trade_info.data.is_none() {
                error!("error getting buy info");
            }

            let trade_data = match current_trade_info.data {
                Some(data) => data,
                None => {
                    warn!("no data, skipping tick.");
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

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        let remaining = current_trade.receive_until.unwrap().remaining_pretty();
        let tradeoffer = format!("https://steamcommunity.com/tradeoffer/{}/",
                                 current_trade.trade_id.unwrap());

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &format!("@{}, трейд был создан, у тебя есть {} чтобы его принять - {}",
                         self.redemption.user_login, remaining, tradeoffer),
                None, None,
                &token).await
        }).await {
            error!("Failed to send chat message: {}", e);
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

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &format!("@{} щекочет мой мозг, видимо трейд принял. \
                не забудь об отзыве - @(ладно пока не надо отзывов на эту хуйню)",
                         self.redemption.user_login),
                None, None,
                &token).await
        }).await {
            error!("Failed to send chat message: {}", e);
        };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            RedemptionStatus::Completed,
            None, None
        ).await {
            error!("Failed to update redemption status (db): {}", e);
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
            error!("Failed to update redemption status (helix): {}", e);
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
                error!("broadcaster settings are not found apparently. sybau bro");
                self.stage = OrderStage::Exit;
                return
            }
            Err(e) => {
                error!("DB Error: {:?}", e);
                return
            }
        };

        let sender_id = match self.get_sender_id() {
            Some(sid) => sid,
            None => { return; }
        };

        let buyer_fault = current_trade.causer.is_some_and(|c| c.eq("buyer"));

        let message = if refund_on_buyer_fail && buyer_fault {
            format!("@{} въебал трейд? красавчик. повезло, что стример сказал возвращать баллы в таких случаях.", self.redemption.user_login)
        } else if !refund_on_buyer_fail && buyer_fault {
            format!("@{} въебал трейд? красавчик. какое счастье, что стример сказал мне нихуя не возвращать в таких случаях. \
            в следующий раз будь аккуратнее 😁😁😁😁", self.redemption.user_login)
        } else {
            format!("@{} сорянчик, продавец долбоёб кажется решил нихуя не отправлять. \
            ну или другая причина, крч возвращаю баллы, можешь попробовать ещё раз купить", self.redemption.user_login)
        };

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &message,
                None, None,
                &token).await
        }).await {
            error!("Failed to send chat message: {}", e);
        };

        let should_refund = !buyer_fault || refund_on_buyer_fail;

        let redemption_status = if should_refund { RedemptionStatus::FailedRefund }
        else { RedemptionStatus::FailedPenalty };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            redemption_status,
            None, None
        ).await {
            error!("Failed to update redemption status (db): {}", e);
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
            error!("Failed to update redemption status (helix): {}", e);
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

        if let Err(e) = self.state.with_bot_user_token(async |token| {
            self.state.helix_client.send_chat_message(
                &self.broadcaster_id,
                &sender_id,
                &format!("@{} трейд превысил максимальное время ожидания (30 минут). баллы возвращать не буду во избежение потери денег.", self.redemption.user_login),
                None, None,
                &token).await
        }).await {
            error!("Failed to send chat message: {}", e);
        };

        if let Err(e) = self.state.db.update_redemption_status(
            self.redemption.redemption_id,
            RedemptionStatus::FailedPenalty,
            Some("timeout"),
            Some("Timed out after 30 minutes waiting for trade completion"),
        ).await {
            error!("Failed to update redemption status (db): {}", e);
        }

        if let Err(e) = self.state.with_broadcaster_token(&self.broadcaster_id, async |token| {
            self.state.helix_client.update_redemption_status(
                &self.broadcaster_id,
                &self.redemption.reward_id.to_string(),
                &self.redemption.redemption_id.to_string(),
                false,
                &token).await
        }).await {
            error!("Failed to update redemption status (helix): {}", e);
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
                error!("the bot is not initialized for some reason");
                self.stage = OrderStage::Exit;
                return None;
            }
        };

        Some(info.user_id.clone())
    }
}