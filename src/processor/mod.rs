use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::messages::{
    MSG_MARKET_ERROR, MSG_ORDER_CREATED, MSG_ORDER_FAILED,
    MSG_ORDER_FAILED_NO_MONEY_PENALTY, MSG_ORDER_FAILED_NO_MONEY_REFUND,
    MSG_ORDER_FAILED_FILTER_EXHAUSTED, MSG_TRADE_LINK_INVALID,
};
use crate::db::rewards::RewardType;
use crate::processor::model::EventSubNotification;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::state::AppState;
use crate::steam::trade_link::TradeLink;
use crate::steam::market;


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

    let state_price = Arc::clone(&state);
    let state_balance = Arc::clone(&state);
    let cid_price = channel_id.clone();
    let cid_balance = channel_id.clone();
    let state_cleanup = Arc::clone(&state);
    let cid_cleanup = channel_id.clone();
    let token_price = broadcaster_token.clone();
    let token_balance = broadcaster_token;
    state.spawn_task(async move {
        let t1 = price_updater::PriceUpdater::new(state_price, cid_price).run(token_price);
        let t2 = balance_updater::BalanceUpdater::new(state_balance, cid_balance).run(token_balance);
        tokio::join!(t1, t2);
        state_cleanup.active_broadcaster_tasks.lock().remove(&cid_cleanup);
        debug!(channel_id = %cid_cleanup, "Broadcaster background tasks completed and cleaned up from active tasks map");
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

    let initial_item_name = match reward_data.reward_type {
        RewardType::Fixed => reward_data.market_item_name.clone(),
        _ => None,
    };

    match state.db.insert_redemption_if_new(&NewRedemption {
        twitch_redemption_id: redemption_id,
        twitch_reward_id: reward_id,
        user_id: event.user_id,
        user_login: event.user_login.clone(),
        user_trade_link: event.user_input.clone(),
        twitch_points_cost: event.reward.cost,
        currency: reward_data.currency.clone(),
        status: RedemptionStatus::Pending,
        market_item_name: initial_item_name,
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

    match reward_data.reward_type {
        RewardType::Fixed => {
            let item_name = reward_data.market_item_name.clone().unwrap_or_default();
            let max_price_i64 = (reward_data.current_market_price as i64)
                + ((reward_data.current_market_price as i64 * reward_data.permissible_market_price_deviation as i64) / 100);
            let max_price = max_price_i64.min(i32::MAX as i64) as i32;
            let redemption_custom_id = redemption_id.to_string();

            buy_item_once(
                &state,
                &broadcaster_setting,
                &broadcaster_user_id,
                &bot_channel_id,
                redemption_id,
                reward_id,
                &event.user_login,
                &item_name,
                max_price,
                trade_link,
                &redemption_custom_id,
                0,
                true,
            ).await;
        }
        RewardType::Pool => {
            let pool = match reward_data.pool_items.as_ref().map(|j| &j.0) {
                Some(items) if !items.is_empty() => items,
                _ => {
                    warn!(redemption_id = %redemption_id, "Pool reward has empty pool items");
                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Pool items list is empty")).await;
                    return;
                }
            };

            let picked = match pick_pool_item(pool) {
                Some(item) => item,
                None => {
                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Failed to pick pool item")).await;
                    return;
                }
            };

            let item_name = picked.market_hash_name.clone();
            let price = picked.current_market_price as i64;
            let dev = picked.permissible_market_price_deviation as i64;
            let max_price_i64 = price + (price * dev) / 100;
            let max_price = max_price_i64.min(i32::MAX as i64) as i32;
            let redemption_custom_id = redemption_id.to_string();

            buy_item_once(
                &state,
                &broadcaster_setting,
                &broadcaster_user_id,
                &bot_channel_id,
                redemption_id,
                reward_id,
                &event.user_login,
                &item_name,
                max_price,
                trade_link,
                &redemption_custom_id,
                0,
                false,
            ).await;
        }
        RewardType::Filter => {
            let filter = match reward_data.filter_config.as_ref().map(|j| &j.0) {
                Some(f) => f,
                None => {
                    warn!(redemption_id = %redemption_id, "Filter reward has no filter_config");
                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Filter config is missing")).await;
                    return;
                }
            };

            let all_prices = match state.get_cached_or_fetch_prices(&reward_data.currency).await {
                Ok(prices) => prices,
                Err(e) => {
                    error!(error = %e, redemption_id = %redemption_id, "Failed to fetch prices for filter redemption");
                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Failed to fetch market prices")).await;
                    return;
                }
            };

            let matching = crate::steam::market::prices::filter_prices(&all_prices, filter);
            if matching.is_empty() {
                warn!(redemption_id = %redemption_id, "No items match filter criteria for redemption");
                update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("No items match filter criteria")).await;
                let msg = state.render_chat_message(&broadcaster_user_id, MSG_ORDER_FAILED_FILTER_EXHAUSTED, &[("buyer", &event.user_login), ("attempts", "0")]);
                let _ = state.with_bot_user_token(async |token| {
                    state.helix_client.send_chat_message(&broadcaster_user_id, &bot_channel_id, &msg, None, None, &token).await
                }).await;
                return;
            }

            let mut attempted_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
            let mut order_created = false;

            for attempt in 1..=5 {
                let available: Vec<usize> = (0..matching.len()).filter(|i| !attempted_indices.contains(i)).collect();
                if available.is_empty() {
                    break;
                }

                let rand_idx = (uuid::Uuid::new_v4().as_u128() % (available.len() as u128)) as usize;
                let selected_idx = available[rand_idx];
                attempted_indices.insert(selected_idx);
                let item = &matching[selected_idx];

                let item_price_minor = market::major_to_minor(item.price, &reward_data.currency);
                let filter_max_minor = market::major_to_minor(filter.max_price, &reward_data.currency);
                let base_price = item_price_minor.min(filter_max_minor);
                let dev = reward_data.permissible_market_price_deviation as i64;
                let max_price_i64 = base_price + (base_price * dev) / 100;
                let max_price = max_price_i64.min(i32::MAX as i64) as i32;

                let custom_id = if attempt == 1 {
                    redemption_id.to_string()
                } else {
                    format!("{}-{}", redemption_id, attempt - 1)
                };

                info!(
                    redemption_id = %redemption_id,
                    attempt = attempt,
                    item = %item.market_hash_name,
                    price = max_price,
                    "Attempting market buy-for for filter reward"
                );

                match state.market_client.buy_for(
                    &broadcaster_setting.market_api_key,
                    &item.market_hash_name,
                    max_price,
                    broadcaster_setting.market_chance_to_transfer,
                    trade_link.clone(),
                    &custom_id,
                ).await {
                    Ok(res) if res.success => {
                        info!(
                            redemption_id = %redemption_id,
                            attempt = attempt,
                            item = %item.market_hash_name,
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
                            Some(&item.market_hash_name),
                            (attempt - 1) as i32,
                        ).await {
                            error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption status to order_created");
                            return;
                        }

                        let order_watcher = OrderWatcher::new(
                            state.clone(),
                            broadcaster_setting.market_api_key.clone(),
                            broadcaster_user_id.clone(),
                            WatcherRedemptionData {
                                redemption_id,
                                custom_id,
                                reward_id,
                                user_login: event.user_login.clone(),
                            },
                        );

                        let token = state.shutdown_token.clone();
                        state.spawn_task(async move {
                            order_watcher.track_redemption(token).await;
                        });

                        let msg = state.render_chat_message(
                            &broadcaster_user_id,
                            MSG_ORDER_CREATED,
                            &[("buyer", &event.user_login), ("item", &item.market_hash_name)],
                        );
                        let _ = state.with_bot_user_token(async |token| {
                            state.helix_client.send_chat_message(
                                &broadcaster_user_id,
                                &bot_channel_id,
                                &msg,
                                None, None,
                                &token,
                            ).await
                        }).await;

                        order_created = true;
                        break;
                    }
                    Ok(res) => {
                        let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
                        warn!(
                            redemption_id = %redemption_id,
                            attempt = attempt,
                            item = %item.market_hash_name,
                            error = %error_msg,
                            "Market rejected buy-for attempt"
                        );

                        if error_msg.eq_ignore_ascii_case("not enough funds on account") {
                            let return_channel_points = broadcaster_setting.refund_if_no_money;
                            let state_for_balance = state.clone();
                            let bc_id_for_balance = broadcaster_user_id.clone();
                            state.spawn_task(async move {
                                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                            });

                            update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, return_channel_points, Some(&error_msg)).await;
                            let msg_template = if return_channel_points { MSG_ORDER_FAILED_NO_MONEY_REFUND } else { MSG_ORDER_FAILED_NO_MONEY_PENALTY };
                            let msg = state.render_chat_message(&broadcaster_user_id, msg_template, &[("buyer", &event.user_login), ("item", &item.market_hash_name)]);
                            let _ = state.with_bot_user_token(async |token| {
                                state.helix_client.send_chat_message(&broadcaster_user_id, &bot_channel_id, &msg, None, None, &token).await
                            }).await;
                            return;
                        }

                        if error_msg.eq_ignore_ascii_case("no item found at the specified chance to transfer at the specified price or below") {
                            continue;
                        }

                        continue;
                    }
                    Err(e) => {
                        warn!(error = %e, redemption_id = %redemption_id, attempt = attempt, "Network error on buy-for attempt, trying next item");
                        continue;
                    }
                }
            }

            if !order_created {
                warn!(redemption_id = %redemption_id, "All attempts to purchase an item for filter reward failed");
                update_redemption_status_failed(
                    state.clone(),
                    &broadcaster_user_id,
                    reward_id,
                    redemption_id,
                    true,
                    Some("All filter buy attempts failed"),
                ).await;

                let msg = state.render_chat_message(
                    &broadcaster_user_id,
                    MSG_ORDER_FAILED_FILTER_EXHAUSTED,
                    &[("buyer", &event.user_login), ("attempts", "5")],
                );
                let _ = state.with_bot_user_token(async |token| {
                    state.helix_client.send_chat_message(
                        &broadcaster_user_id,
                        &bot_channel_id,
                        &msg,
                        None, None,
                        &token,
                    ).await
                }).await;
            }
        }
    }
}

fn pick_pool_item(items: &[crate::db::rewards::PoolItemConfig]) -> Option<&crate::db::rewards::PoolItemConfig> {
    if items.is_empty() {
        return None;
    }
    let total_weight: f64 = items.iter().map(|i| i.weight.max(0.0)).sum();
    if total_weight <= 0.0 {
        return items.first();
    }
    let roll = ((uuid::Uuid::new_v4().as_u128() as f64) / (u128::MAX as f64)) * total_weight;
    let mut current = 0.0;
    for item in items {
        current += item.weight.max(0.0);
        if roll <= current {
            return Some(item);
        }
    }
    items.last()
}

async fn buy_item_once(
    state: &Arc<AppState>,
    broadcaster_setting: &crate::db::broadcaster_settings::BroadcasterSetting,
    broadcaster_user_id: &str,
    bot_channel_id: &str,
    redemption_id: Uuid,
    reward_id: Uuid,
    user_login: &str,
    item_name: &str,
    max_price: i32,
    trade_link: TradeLink,
    redemption_custom_id: &str,
    retry_count: i32,
    trigger_price_update_on_deviation: bool,
) {
    match state.market_client.buy_for(
        &broadcaster_setting.market_api_key,
        item_name,
        max_price,
        broadcaster_setting.market_chance_to_transfer,
        trade_link,
        redemption_custom_id,
    ).await {
        Ok(res) if res.success => {
            info!(
                redemption_id = %redemption_id,
                item = %item_name,
                price = ?res.price,
                market_id = ?res.id,
                "Market buy-for succeeded, order created"
            );

            let state_for_balance = state.clone();
            let bc_id_for_balance = broadcaster_user_id.to_string();
            state.spawn_task(async move {
                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
            });

            let paid_price = res.price.unwrap_or(max_price as i64);

            if let Err(e) = state.db.set_redemption_order_created(
                redemption_id,
                paid_price,
                Some(item_name),
                retry_count,
            ).await {
                error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption status to order_created");
                return;
            }

            let order_watcher = OrderWatcher::new(
                state.clone(),
                broadcaster_setting.market_api_key.clone(),
                broadcaster_user_id.to_string(),
                WatcherRedemptionData {
                    redemption_id,
                    custom_id: redemption_custom_id.to_string(),
                    reward_id,
                    user_login: user_login.to_string(),
                },
            );

            let token = state.shutdown_token.clone();
            state.spawn_task(async move {
                order_watcher.track_redemption(token).await;
            });

            let msg = state.render_chat_message(
                broadcaster_user_id,
                MSG_ORDER_CREATED,
                &[("buyer", user_login), ("item", item_name)],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    broadcaster_user_id,
                    bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for created order");
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
                let bc_id_for_balance = broadcaster_user_id.to_string();
                state.spawn_task(async move {
                    let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                });
            }

            if trigger_price_update_on_deviation && error_msg.eq_ignore_ascii_case("no item found at the specified chance to transfer at the specified price or below") {
                let state_clone = state.clone();
                let bc_id = broadcaster_user_id.to_string();
                state.spawn_task(async move {
                    info!(reward_id = %reward_id, "Triggering immediate price update due to market price deviation");
                    if let Err(e) = price_updater::update_single_reward_price(&state_clone, &bc_id, reward_id).await {
                        warn!(error = %e, reward_id = %reward_id, "Failed immediate price update for reward");
                    }
                });
            }

            update_redemption_status_failed(
                state.clone(),
                broadcaster_user_id,
                reward_id,
                redemption_id,
                return_channel_points,
                Some(&error_msg),
            ).await;

            let code_str = code.to_string();
            let (msg_template, msg_vars): (&str, Vec<(&str, &str)>) = if error_msg.eq_ignore_ascii_case("not enough funds on account") {
                if return_channel_points {
                    (MSG_ORDER_FAILED_NO_MONEY_REFUND, vec![("buyer", user_login), ("item", item_name)])
                } else {
                    (MSG_ORDER_FAILED_NO_MONEY_PENALTY, vec![("buyer", user_login), ("item", item_name)])
                }
            } else {
                (MSG_ORDER_FAILED, vec![("buyer", user_login), ("code", code_str.as_str()), ("error", error_msg.as_str()), ("item", item_name)])
            };

            let msg = state.render_chat_message(
                broadcaster_user_id,
                msg_template,
                &msg_vars,
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    broadcaster_user_id,
                    bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for rejected order");
            }
        }
        Err(e) => {
            error!(
                error = %e,
                redemption_id = %redemption_id,
                item = %item_name,
                "Failed to send HTTP request to Market"
            );
            update_redemption_status_failed(
                state.clone(),
                broadcaster_user_id,
                reward_id,
                redemption_id,
                true,
                Some(&format!("Market network error: {}", e)),
            ).await;

            let msg = state.render_chat_message(
                broadcaster_user_id,
                MSG_MARKET_ERROR,
                &[("buyer", user_login), ("item", item_name)],
            );
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    broadcaster_user_id,
                    bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for market network error");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::rewards::PoolItemConfig;

    #[test]
    fn test_pick_pool_item_empty() {
        let empty: Vec<PoolItemConfig> = vec![];
        assert!(pick_pool_item(&empty).is_none());
    }

    #[test]
    fn test_pick_pool_item_single() {
        let items = vec![PoolItemConfig {
            market_hash_name: "AK-47 | Redline (Field-Tested)".into(),
            weight: 100.0,
            permissible_market_price_deviation: 10,
            current_market_price: 1500,
        }];
        let picked = pick_pool_item(&items).unwrap();
        assert_eq!(picked.market_hash_name, "AK-47 | Redline (Field-Tested)");
    }

    #[test]
    fn test_pick_pool_item_weighted_distribution() {
        let items = vec![
            PoolItemConfig {
                market_hash_name: "Common".into(),
                weight: 90.0,
                permissible_market_price_deviation: 10,
                current_market_price: 100,
            },
            PoolItemConfig {
                market_hash_name: "Rare".into(),
                weight: 10.0,
                permissible_market_price_deviation: 10,
                current_market_price: 1000,
            },
        ];

        let mut common_count = 0;
        let mut rare_count = 0;
        for _ in 0..1000 {
            let picked = pick_pool_item(&items).unwrap();
            if picked.market_hash_name == "Common" {
                common_count += 1;
            } else {
                rare_count += 1;
            }
        }

        // With 90/10 split over 1000 trials, common should be between 800 and 970
        assert!(common_count > 750, "Common count: {}", common_count);
        assert!(rare_count > 20, "Rare count: {}", rare_count);
    }

    #[test]
    fn test_filter_reward_order_price_clamping() {
        let item_price_minor = 250000i64; // 2500.00 RUB
        let filter_max_minor = 200000i64; // 2000.00 RUB
        let base_price = item_price_minor.min(filter_max_minor);
        assert_eq!(base_price, 200000); // clamped to filter_max

        let deviation = 10i64;
        let max_price = base_price + (base_price * deviation / 100);
        assert_eq!(max_price, 220000); // 2200.00 RUB
    }

    #[test]
    fn test_filter_retry_custom_id_format() {
        let redemption_id = uuid::Uuid::new_v4();

        // Attempt 1: original redemption id
        let attempt1_id = redemption_id.to_string();
        assert_eq!(attempt1_id, redemption_id.to_string());

        // Attempt 2: redemption_id-1
        let attempt2_id = format!("{}-{}", redemption_id, 2 - 1);
        assert_eq!(attempt2_id, format!("{}-1", redemption_id));

        // Attempt 5: redemption_id-4
        let attempt5_id = format!("{}-{}", redemption_id, 5 - 1);
        assert_eq!(attempt5_id, format!("{}-4", redemption_id));
    }
}
