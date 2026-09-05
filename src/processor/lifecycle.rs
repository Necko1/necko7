use std::sync::Arc;
use tracing::{debug, error, info, warn};
use crate::processor::balance_updater::BalanceUpdater;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::processor::price_updater::PriceUpdater;
use crate::state::AppState;

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

    let state_price = Arc::clone(&state);
    let state_balance = Arc::clone(&state);
    let cid_price = channel_id.clone();
    let cid_balance = channel_id.clone();
    let state_cleanup = Arc::clone(&state);
    let cid_cleanup = channel_id.clone();
    let token_price = broadcaster_token.clone();
    let token_balance = broadcaster_token;
    state.spawn_task(async move {
        let t1 = PriceUpdater::new(state_price, cid_price).run(token_price);
        let t2 = BalanceUpdater::new(state_balance, cid_balance).run(token_balance);
        tokio::join!(t1, t2);
        state_cleanup.active_broadcaster_tasks.lock().remove(&cid_cleanup);
        debug!(channel_id = %cid_cleanup, "Broadcaster background tasks completed and cleaned up from active tasks map");
    });

    let state_chat = Arc::clone(&state);
    let cid_chat = channel_id.clone();
    state.spawn_task(async move {
        if let Err(e) = state_chat.subscribe_broadcaster_chat_ws(&cid_chat).await {
            warn!(error = %e, channel_id = %cid_chat, "Failed to subscribe broadcaster to chat WebSocket on task startup");
        }
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
    let state_chat = state.clone();
    state.spawn_task(async move {
        crate::processor::chat_listener::run_chat_listener(state_chat).await;
    });

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

        let custom_id = if order.retry_count == 0 {
            order.twitch_redemption_id.to_string()
        } else {
            format!("{}-{}", order.twitch_redemption_id, order.retry_count)
        };

        let order_watcher = OrderWatcher::new(
            state.clone(),
            setting.market_api_key,
            reward.streamer_id,
            WatcherRedemptionData {
                redemption_id: order.twitch_redemption_id,
                custom_id,
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
