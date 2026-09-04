use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::messages::{
    MSG_MARKET_ERROR, MSG_ORDER_CREATED, MSG_ORDER_FAILED, MSG_TRADE_LINK_INVALID,
};
use crate::processor::model::EventSubNotification;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::state::AppState;
use crate::steam::trade_link::TradeLink;

pub mod model;
pub mod price_updater;
pub mod order_watcher;
pub mod balance_updater;

pub fn start_broadcaster_tasks(state: Arc<AppState>, channel_id: String) {
    let broadcaster_token = {
        let mut tasks = state.active_broadcaster_tasks.lock();
        if tasks.contains_key(&channel_id) {
            debug!(channel_id = %channel_id, "Broadcaster tasks already running, skipping duplicate startup");
            return;
        }
        let token = state.shutdown_token.child_token();
        tasks.insert(channel_id.clone(), token.clone());
        token
    };

    info!(channel_id = %channel_id, "Starting broadcaster background tasks (price updater, balance updater)");

    let price_updater = price_updater::PriceUpdater::new(state.clone(), channel_id.clone());
    let token_price = broadcaster_token.clone();
    state.spawn_task(async move {
        price_updater.run(token_price).await;
    });

    let balance_updater = balance_updater::BalanceUpdater::new(state.clone(), channel_id);
    let token_balance = broadcaster_token;
    state.spawn_task(async move {
        balance_updater.run(token_balance).await;
    });
}

pub fn stop_broadcaster_tasks(state: &AppState, channel_id: &str) {
    let mut tasks = state.active_broadcaster_tasks.lock();
    if let Some(token) = tasks.remove(channel_id) {
        info!(channel_id = %channel_id, "Stopping broadcaster background tasks");
        token.cancel();
    }
}

pub async fn start_background_tasks(state: Arc<AppState>) {
    let state_eventsub = state.clone();
    state.spawn_task(async move {
        state_eventsub.recover_eventsub_subscriptions().await;
    });

    let state_orders = state.clone();
    state.spawn_task(async move {
        recover_active_orders(state_orders).await;
    });

    let state_sessions = state.clone();
    let session_token = state.shutdown_token.clone();
    state.spawn_task(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = session_token.cancelled() => {
                    debug!("Session cleanup task received stop signal");
                    break;
                }
                _ = interval.tick() => {}
            }

            if session_token.is_cancelled() {
                break;
            }

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
        if state.shutdown_token.is_cancelled() {
            info!("Shutdown in progress, stopping order recovery");
            break;
        }

        let reward = match state.db.get_reward_by_twitch_id(order.twitch_reward_id).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                warn!(redemption_id = %order.twitch_redemption_id, reward_id = %order.twitch_reward_id, "Active order has missing reward in DB, skipping recovery");
                continue;
            }
            Err(e) => {
                error!(error = %e, redemption_id = %order.twitch_redemption_id, "DB error fetching reward during order recovery");
                continue;
            }
        };

        let setting = match state.db.get_broadcaster_setting(&reward.streamer_id).await {
            Ok(Some(s)) if !s.market_api_key.trim().is_empty() => s,
            Ok(Some(_)) => {
                warn!(streamer_id = %reward.streamer_id, redemption_id = %order.twitch_redemption_id, "Broadcaster has no market API key configured, skipping order recovery");
                continue;
            }
            Ok(None) => {
                warn!(streamer_id = %reward.streamer_id, redemption_id = %order.twitch_redemption_id, "Broadcaster setting not found in DB, skipping order recovery");
                continue;
            }
            Err(e) => {
                error!(error = %e, streamer_id = %reward.streamer_id, "DB error fetching broadcaster setting during order recovery");
                continue;
            }
        };

        info!(redemption_id = %order.twitch_redemption_id, user_login = %order.user_login, "Resuming OrderWatcher for recovered active order");

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

        let token = state.shutdown_token.clone();
        state.spawn_task(async move {
            order_watcher.track_redemption(token).await;
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

    info!(
        redemption_id = %redemption_id,
        reward_id = %reward_id,
        broadcaster_id = %broadcaster_user_id,
        user_login = %event.user_login,
        "Processing EventSub redemption"
    );

    let reward_data = match state.db.get_reward_by_twitch_id(reward_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            debug!(reward_id = %reward_id, redemption_id = %redemption_id, "Reward redeemed but not found in DB (ignoring)");
            return;
        }
        Err(e) => {
            error!(error = %e, reward_id = %reward_id, redemption_id = %redemption_id, "DB error fetching reward during redemption processing");
            return;
        }
    };

    if !reward_data.market_autobuy {
        debug!(reward_id = %reward_id, redemption_id = %redemption_id, "Reward has market_autobuy disabled, skipping processing");
        return;
    }

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
            info!(redemption_id = %redemption_id, "Redemption is already being processed, ignoring duplicate");
            return;
        }
        Err(e) => {
            error!(error = %e, redemption_id = %redemption_id, reward_id = %reward_id, "DB error inserting new redemption record");
            return;
        }
    }

    let broadcaster_setting = match state.db.get_broadcaster_setting(&broadcaster_user_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            error!(
                broadcaster_user_id = %broadcaster_user_id,
                broadcaster_login = %event.broadcaster_user_login,
                redemption_id = %redemption_id,
                "Broadcaster settings not found in DB during redemption processing"
            );
            return;
        }
        Err(e) => {
            error!(error = %e, broadcaster_user_id = %broadcaster_user_id, redemption_id = %redemption_id, "DB error fetching broadcaster setting");
            return;
        }
    };

    if !broadcaster_setting.is_active || reward_data.is_deleted || reward_data.is_paused {
        warn!(
            redemption_id = %redemption_id,
            broadcaster_active = broadcaster_setting.is_active,
            reward_deleted = reward_data.is_deleted,
            reward_paused = reward_data.is_paused,
            "Redemption cancelled: broadcaster is inactive or reward is deleted/paused"
        );
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
                error!(redemption_id = %redemption_id, "Bot account is not initialized in AppState; cannot process redemption");
                return;
            }
        }
    };

    let trade_link = match TradeLink::parse(&event.user_input) {
        Some(t) => t,
        None => {
            warn!(
                redemption_id = %redemption_id,
                user_login = %event.user_login,
                user_input = %event.user_input,
                "Failed to parse Steam trade link from redemption user input"
            );
            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                true,
                Some("Couldn't parse trade link in user input")
            ).await;

            let msg = state.render_chat_message(
                &broadcaster_user_id,
                MSG_TRADE_LINK_INVALID,
                &[("buyer", &event.user_login)],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message informing user of invalid trade link");
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
            info!(
                redemption_id = %redemption_id,
                item = %reward_data.market_item_name,
                price = ?res.price,
                market_id = ?res.id,
                "Market buy-for succeeded, order created"
            );

            let state_for_balance = state.clone();
            let bc_id_for_balance = broadcaster_user_id.clone();
            state.spawn_task(async move {
                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
            });

            let paid_price = res.price.unwrap_or(max_price as i64);

            if let Err(e) = state.db.set_redemption_order_created(
                redemption_id,
                paid_price,
            ).await {
                error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption status to order_created");
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

            let token = state.shutdown_token.clone();
            state.spawn_task(async move {
                order_watcher.track_redemption(token).await;
            });

            let msg = state.render_chat_message(
                &broadcaster_user_id,
                MSG_ORDER_CREATED,
                &[("buyer", &event.user_login)],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for created order");
                return;
            }
        }
        Ok(res) => {
            let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
            let code = res.code.unwrap_or(0);
            warn!(
                redemption_id = %redemption_id,
                code = code,
                error = %error_msg,
                "Market rejected buy-for"
            );

            let mut return_channel_points = true;

            if error_msg.eq_ignore_ascii_case("not enough funds on account") {
                return_channel_points = broadcaster_setting.refund_if_no_money;

                let state_for_balance = state.clone();
                let bc_id_for_balance = broadcaster_user_id.clone();
                state.spawn_task(async move {
                    let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                });
            }

            if error_msg.eq_ignore_ascii_case("no item found at the specified chance to transfer at the specified price or below") {
                let state_clone = state.clone();
                let bc_id = broadcaster_user_id.clone();
                state.spawn_task(async move {
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

            let code_str = code.to_string();
            let msg = state.render_chat_message(
                &broadcaster_user_id,
                MSG_ORDER_FAILED,
                &[
                    ("buyer", &event.user_login),
                    ("code", &code_str),
                    ("error", &error_msg),
                ],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for rejected order");
                return;
            }
        }
        Err(e) => {
            error!(
                error = %e,
                redemption_id = %redemption_id,
                item = %reward_data.market_item_name,
                "Failed to send HTTP request to Market"
            );
            let msg = state.render_chat_message(
                &broadcaster_user_id,
                MSG_MARKET_ERROR,
                &[("buyer", &event.user_login)],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for market network error");
                return;
            }
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
        error!(
            error = %e,
            redemption_id = %redemption_id,
            reward_id = %reward_id,
            broadcaster_user_id = %broadcaster_user_id,
            return_points = return_channel_points,
            "Failed to update redemption status on Twitch Helix"
        );
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
        error!(
            error = %e,
            redemption_id = %redemption_id,
            status = ?redemption_status,
            fail_description = ?fail_description,
            "DB error updating failed redemption status"
        );
        return;
    }

    info!(
        redemption_id = %redemption_id,
        status = ?redemption_status,
        fail_description = ?fail_description,
        "Redemption status marked as failed successfully"
    );
}
