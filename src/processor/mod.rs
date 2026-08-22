use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::processor::model::EventSubNotification;
use crate::state::AppState;
use crate::steam::trade_link::TradeLink;

pub mod model;

pub async fn process_redemption(
    state: Arc<AppState>,
    notification: EventSubNotification,
) {
    let event = notification.event;
    let redemption_id = event.id;
    let reward_id = event.reward.id;
    let broadcaster_user_id = event.broadcaster_user_id;

    info!("Processing redemption {} for user {}", redemption_id, event.user_login);

    let reward_data = match state.db.get_reward_by_twitch_id(reward_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            debug!("Reward {} redeemed but not found in DB", reward_id);
            // it's just prolly not created by the bot lol
            return;
        }
        Err(e) => {
            error!("DB error: {:?}", e);
            return;
        }
    };

    match state.db.insert_redemption_if_new(&NewRedemption {
        twitch_redemption_id: redemption_id,
        twitch_reward_id: reward_id,
        user_id: event.user_id,
        user_login: event.user_login.clone(),
        user_trade_link: event.user_input.clone(),
        twitch_points_cost: event.reward.cost,
        status: RedemptionStatus::Pending,
    }).await {
        Ok(Some(_)) => {},
        Ok(None) => {
            info!("Redemption {} is already being processed. Ignoring.", redemption_id);
            return;
        }
        Err(e) => {
            error!("DB error: {:?}", e);
            return;
        }
    }

    let broadcaster_setting = match state.db.get_broadcaster_setting(&broadcaster_user_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            error!("Broadcaster {} ({}) not found in DB",
                event.broadcaster_user_login, broadcaster_user_id);
            return;
        }
        Err(e) => {
            error!("DB error: {:?}", e);
            return;
        }
    };

    if !broadcaster_setting.is_active || reward_data.is_deleted || reward_data.is_paused {
        update_redemption_status_failed(
            state.clone(),
            &broadcaster_user_id,
            reward_id,
            redemption_id,
            true,
            Some("Bot or reward is not active")
        ).await;

        return;
    }

    let bot_channel_id = {
        let guard = state.bot_info.read();

        match guard.as_ref() {
            Some(s) => s.user_id.clone(),
            None => {
                error!("Bot account is not initialized.");
                return;
            }
        }
    };

    let trade_link = match TradeLink::parse(&event.user_input) {
        Some(t) => t,
        None => {
            warn!("Invalid trade link provided by {}", event.user_login);
            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                true,
                Some("Couldn't parse trade link in user input")
            ).await;

            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    "не смог спарсить трейд ссылку, вернул баллы.", // fixme hardcoded chat messages
                    None, None,
                    &token).await
            }).await {
                error!("Failed to send chat message: {}", e);
                return;
            }
            return;
        }
    };

    let max_price = reward_data.current_market_price
        + (reward_data.current_market_price * reward_data.permissible_market_price_deviation / 100);

    match state.market_client.buy_for(
        &broadcaster_setting.market_api_key,
        &reward_data.market_item_name,
        max_price,
        trade_link,
        &redemption_id
    ).await {
        Ok(res) if res.success => {
            info!("Market buy-for success for redemption {}", redemption_id);
            if let Err(e) = state.db.update_redemption_status(
                redemption_id,
                RedemptionStatus::OrderCreated,
                None,
                None
            ).await {
                error!("DB error: {:?}", e);
                return;
            }

            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    "создал ордер на маркете, ожидай трейда в скорем времени или другого сообщения от меня в чате",
                    None, None,
                    &token).await
            }).await {
                error!("Failed to send chat message: {}", e);
                return;
            }
        }
        Ok(res) => {
            let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
            let code = res.code.unwrap_or(0);
            warn!("Market rejected buy-for (code {}): {}", code, error_msg);

            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                true,
                Some(&error_msg)
            ).await;

            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &format!("не удалось создать ордер на маркете, вернул баллы. ошибка {}: {}", code, error_msg),
                    None, None,
                    &token).await
            }).await {
                error!("Failed to send chat message: {}", e);
                return;
            }
        }
        Err(e) => {
            error!("Failed to send HTTP request to Market: {:?}", e);
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    "произошла внутренняя ошибка при отправке запроса на маркет. ничего трогать не буду, подробности в логах.",
                    None, None,
                    &token).await
            }).await {
                error!("Failed to send chat message: {}", e);
                return;
            }
            // НЕ ВОЗВРАЩАТЬ БАЛЛЫ И НЕ МЕНЯТЬ СТАТУС
        }
    }
}

async fn update_redemption_status_failed(
    state: Arc<AppState>,
    broadcaster_user_id: &str,
    reward_id: Uuid,
    redemption_id: Uuid,

    return_channel_points: bool,
    fail_description: Option<&str>,
) {
    if let Err(e) = state.with_broadcaster_token(broadcaster_user_id, async |token| {
        state.helix_client.update_redemption_status(
            broadcaster_user_id,
            &reward_id.to_string(),
            &redemption_id.to_string(),
            return_channel_points,
            &token).await
    }).await {
        error!("Failed to update redemption status: {}", e);
        return;
    }

    let redemption_status = if return_channel_points {
        RedemptionStatus::FailedRefund
    } else { RedemptionStatus::FailedPenalty };

    if let Err(e) = state.db.update_redemption_status(
        redemption_id,
        redemption_status,
        None,
        fail_description
    ).await {
        error!("DB error: {:?}", e);
        return;
    }
}
