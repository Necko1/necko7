use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::processor::model::EventSubNotification;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::state::AppState;
use crate::steam::trade_link::TradeLink;

pub mod model;
pub mod price_updater;
pub mod order_watcher;
pub mod balance_updater;

pub fn start_broadcaster_tasks(state: Arc<AppState>, channel_id: String) {
    {
        let mut tasks = state.active_broadcaster_tasks.lock();
        if !tasks.insert(channel_id.clone()) {
            return;
        }
    }

    let price_updater = price_updater::PriceUpdater::new(state.clone(), channel_id.clone());
    tokio::spawn(async move {
        price_updater.run().await;
    });

    let balance_updater = balance_updater::BalanceUpdater::new(state, channel_id);
    tokio::spawn(async move {
        balance_updater.run().await;
    });
}

pub async fn start_background_tasks(state: Arc<AppState>) {
    let state_eventsub = state.clone();
    tokio::spawn(async move {
        state_eventsub.recover_eventsub_subscriptions().await;
    });

    let state_orders = state.clone();
    tokio::spawn(async move {
        recover_active_orders(state_orders).await;
    });

    let state_sessions = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = state_sessions.db.delete_expired_sessions().await {
                warn!(error = %e, "Failed to clean up expired sessions");
            } else {
                debug!("Expired sessions cleanup completed");
            }
        }
    });

    match state.db.get_all_broadcasters().await {
        Ok(broadcasters) => {
            for b in broadcasters {
                start_broadcaster_tasks(state.clone(), b.channel_id);
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to load broadcasters from DB at startup for background tasks");
        }
    }
}

async fn recover_active_orders(state: Arc<AppState>) {
    let active_orders = match state.db.get_active_orders().await {
        Ok(orders) => orders,
        Err(e) => {
            error!(error = %e, "Failed to fetch active orders for recovery");
            return;
        }
    };

    if active_orders.is_empty() {
        return;
    }

    info!(count = active_orders.len(), "Resuming tracking for active orders");

    for order in active_orders {
        let reward = match state.db.get_reward_by_twitch_id(order.twitch_reward_id).await {
            Ok(Some(r)) => r,
            _ => continue,
        };

        let setting = match state.db.get_broadcaster_setting(&reward.streamer_id).await {
            Ok(Some(s)) if !s.market_api_key.trim().is_empty() => s,
            _ => continue,
        };

        let order_watcher = OrderWatcher::new(
            state.clone(),
            setting.market_api_key,
            reward.streamer_id,
            WatcherRedemptionData {
                redemption_id: order.twitch_redemption_id,
                reward_id: order.twitch_reward_id,
                user_login: order.user_login,
            },
        );

        tokio::spawn(async move {
            order_watcher.track_redemption().await;
        });
    }
}

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

    if !reward_data.market_autobuy { return; }

    match state.db.insert_redemption_if_new(&NewRedemption {
        twitch_redemption_id: redemption_id,
        twitch_reward_id: reward_id,
        user_id: event.user_id,
        user_login: event.user_login.clone(),
        user_trade_link: event.user_input.clone(),
        twitch_points_cost: event.reward.cost,
        currency: reward_data.currency.clone(),
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
                    &format!("@{} не смог спарсить трейд ссылку, вернул баллы.", event.user_login), // fixme hardcoded chat messages
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
        broadcaster_setting.market_chance_to_transfer,
        trade_link,
        &redemption_id
    ).await {
        Ok(res) if res.success => {
            info!("Market buy-for success for redemption {}", redemption_id);

            let state_for_balance = state.clone();
            let bc_id_for_balance = broadcaster_user_id.clone();
            tokio::spawn(async move {
                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
            });

            let paid_price = res.price.unwrap_or(max_price as i64);

            if let Err(e) = state.db.set_redemption_order_created(
                redemption_id,
                paid_price,
            ).await {
                error!("DB error: {:?}", e);
                return;
            }

            let order_watcher = OrderWatcher::new(
                state.clone(),
                broadcaster_setting.market_api_key,
                broadcaster_user_id.clone(),
                WatcherRedemptionData {
                    redemption_id,
                    reward_id,
                    user_login: event.user_login.clone(),
            });

            tokio::spawn(async move {
                order_watcher.track_redemption().await;
            });

            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &format!("@{} создал ордер на маркете, ожидай трейда в скорем времени (до 5-и минут) или другого сообщения от меня в чате", event.user_login),
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

            let mut return_channel_points = true;

            if error_msg.eq_ignore_ascii_case("not enough funds on account") {
                return_channel_points = broadcaster_setting.refund_if_no_money;

                let state_for_balance = state.clone();
                let bc_id_for_balance = broadcaster_user_id.clone();
                tokio::spawn(async move {
                    let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                });
            }

            if error_msg.eq_ignore_ascii_case("no item found at the specified chance to transfer at the specified price or below") {
                let state_clone = state.clone();
                let bc_id = broadcaster_user_id.clone();
                tokio::spawn(async move {
                    info!(reward_id = %reward_id, "Triggering immediate price update due to market price deviation");
                    if let Err(e) = price_updater::update_single_reward_price(&state_clone, &bc_id, reward_id).await {
                        warn!(error = %e, reward_id = %reward_id, "Failed immediate price update for reward");
                    }
                });
            }

            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                return_channel_points,
                Some(&error_msg)
            ).await;

            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &format!("@{} не удалось создать ордер на маркете, вернул баллы. ошибка {}: {}", event.user_login, code, error_msg),
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
                    &format!("@{} произошла внутренняя ошибка при отправке запроса на маркет. ничего трогать не буду, подробности в логах.", event.user_login),
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
