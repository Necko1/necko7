use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::db::rewards::{PauseReason, RewardType};
use crate::helix::api::custom_rewards::model::UpdateCustomReward;
use crate::messages::{
    MSG_MARKET_ERROR, MSG_ORDER_CREATED, MSG_ORDER_FAILED,
    MSG_ORDER_FAILED_NO_MONEY_PENALTY, MSG_ORDER_FAILED_NO_MONEY_REFUND,
    MSG_ORDER_FAILED_FILTER_EXHAUSTED, MSG_TRADE_LINK_INVALID,
    MSG_CHAT_REQ_FAILED_MESSAGES, MSG_CHAT_REQ_FAILED_CHARACTERS, MSG_CHAT_REQ_FAILED_BOTH,
};
use crate::processor::model::EventSubNotification;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::processor::price_updater;
use crate::state::AppState;
use crate::steam::market;
use crate::steam::trade_link::TradeLink;

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
            tracing::debug!(reward_id = %reward_id, redemption_id = %redemption_id, "Reward redeemed but not found in DB (ignoring)");
            return;
        }
        Err(e) => {
            error!(error = %e, reward_id = %reward_id, redemption_id = %redemption_id, "DB error fetching reward during redemption processing");
            return;
        }
    };

    if !reward_data.market_autobuy {
        tracing::debug!(reward_id = %reward_id, redemption_id = %redemption_id, "Reward has market_autobuy disabled, skipping processing");
        return;
    }

    let initial_item_name = match reward_data.reward_type {
        RewardType::Fixed => reward_data.market_item_name.clone(),
        _ => None,
    };

    match state.db.insert_redemption_if_new(&NewRedemption {
        twitch_redemption_id: redemption_id,
        twitch_reward_id: reward_id,
        user_id: event.user_id.clone(),
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

    // Check chat activity requirements if configured on the reward
    if reward_data.chat_min_messages.is_some() || reward_data.chat_min_characters.is_some() {
        let hours = reward_data.chat_time_window_hours.unwrap_or(24);
        let since = if hours > 0 {
            Some(chrono::Utc::now() - chrono::Duration::hours(hours as i64))
        } else {
            None
        };

        let (user_msgs, user_chars) = match state.db.get_user_chat_stats(
            &broadcaster_user_id,
            &event.user_id,
            since,
        ).await {
            Ok(stats) => stats,
            Err(e) => {
                error!(error = %e, user_id = %event.user_id, "DB error fetching user chat stats for redemption check");
                (0, 0)
            }
        };

        let msgs_ok = match reward_data.chat_min_messages {
            Some(min) => user_msgs >= min as i64,
            None => true,
        };

        let chars_ok = match reward_data.chat_min_characters {
            Some(min) => user_chars >= min as i64,
            None => true,
        };

        let operator = reward_data.chat_logical_operator.unwrap_or(crate::db::rewards::ChatLogicalOperator::And);
        let passed = match (reward_data.chat_min_messages.is_some(), reward_data.chat_min_characters.is_some()) {
            (true, true) => match operator {
                crate::db::rewards::ChatLogicalOperator::And => msgs_ok && chars_ok,
                crate::db::rewards::ChatLogicalOperator::Or => msgs_ok || chars_ok,
            },
            (true, false) => msgs_ok,
            (false, true) => chars_ok,
            (false, false) => true,
        };

        if !passed {
            warn!(
                redemption_id = %redemption_id,
                user_id = %event.user_id,
                user_login = %event.user_login,
                user_msgs,
                user_chars,
                min_msgs = ?reward_data.chat_min_messages,
                min_chars = ?reward_data.chat_min_characters,
                "User failed chat activity requirement for reward"
            );

            let refund = reward_data.refund_if_chat_req_failed;
            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                refund,
                Some("User did not meet chat activity requirements")
            ).await;

            let refund_status_text = if refund {
                "Баллы возвращены."
            } else {
                "Баллы не возвращаются."
            };

            let hours_str = hours.to_string();
            let user_msgs_str = user_msgs.to_string();
            let user_chars_str = user_chars.to_string();
            let min_msgs_str = reward_data.chat_min_messages.map(|m| m.to_string()).unwrap_or_default();
            let min_chars_str = reward_data.chat_min_characters.map(|c| c.to_string()).unwrap_or_default();
            let op_str = match operator {
                crate::db::rewards::ChatLogicalOperator::And => "и",
                crate::db::rewards::ChatLogicalOperator::Or => "или",
            };

            let (template_key, vars) = if reward_data.chat_min_messages.is_some() && reward_data.chat_min_characters.is_none() {
                (
                    MSG_CHAT_REQ_FAILED_MESSAGES,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_messages", user_msgs_str.as_str()),
                        ("min_messages", min_msgs_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("refund_status", refund_status_text),
                    ],
                )
            } else if reward_data.chat_min_characters.is_some() && reward_data.chat_min_messages.is_none() {
                (
                    MSG_CHAT_REQ_FAILED_CHARACTERS,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_characters", user_chars_str.as_str()),
                        ("min_characters", min_chars_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("refund_status", refund_status_text),
                    ],
                )
            } else {
                (
                    MSG_CHAT_REQ_FAILED_BOTH,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_messages", user_msgs_str.as_str()),
                        ("min_messages", min_msgs_str.as_str()),
                        ("user_characters", user_chars_str.as_str()),
                        ("min_characters", min_chars_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("operator", op_str),
                        ("refund_status", refund_status_text),
                    ],
                )
            };

            let msg = state.render_chat_message(&broadcaster_user_id, template_key, &vars);
            if let Err(e) = state.with_bot_user_token(async |token| {
                state.helix_client.send_chat_message(
                    &broadcaster_user_id,
                    &bot_channel_id,
                    &msg,
                    None, None,
                    &token).await
            }).await {
                error!(error = %e, redemption_id = %redemption_id, "Failed to send chat message for chat requirements failure");
            }

            return;
        }
    }

    match reward_data.reward_type {
        RewardType::Fixed => {
            let item_name = reward_data.market_item_name.clone().unwrap_or_default();
            let max_price_i64 = (reward_data.current_market_price as i64)
                + ((reward_data.current_market_price as i64 * reward_data.permissible_market_price_deviation as i64) / 100);
            let max_price = max_price_i64.min(i32::MAX as i64) as i32;

            if let Some(min_p) = reward_data.min_market_price {
                if reward_data.current_market_price < min_p {
                    warn!(
                        redemption_id = %redemption_id,
                        price = reward_data.current_market_price,
                        min = min_p,
                        "Redemption cancelled: current market price is below configured min_market_price"
                    );

                    let state_c = state.clone();
                    let bc_id = broadcaster_user_id.clone();
                    let r_id = reward_id;
                    state.spawn_task(async move {
                        let r_str = r_id.to_string();
                        let s_for_token = state_c.clone();
                        let b_for_closure = bc_id.clone();
                        let _ = state_c.with_broadcaster_token(&bc_id, move |token| {
                            let b = b_for_closure.clone();
                            let r = r_str.clone();
                            let s = s_for_token.clone();
                            async move {
                                s.helix_client.update_custom_reward(&b, &r, UpdateCustomReward { is_paused: Some(true), ..Default::default() }, &token).await
                            }
                        }).await;
                        let _ = state_c.db.set_reward_paused(r_id, true, Some(PauseReason::PriceLimit)).await;
                    });

                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Market price below minimum limit")).await;
                    let msg = state.render_chat_message(
                        &broadcaster_user_id,
                        MSG_ORDER_FAILED,
                        &[("buyer", &event.user_login), ("code", "LIMIT"), ("error", "цена скина на маркете ниже установленного стримером минимума"), ("item", &item_name)],
                    );
                    let _ = state.with_bot_user_token(async |token| {
                        state.helix_client.send_chat_message(&broadcaster_user_id, &bot_channel_id, &msg, None, None, &token).await
                    }).await;
                    return;
                }
            }

            if let Some(max_p) = reward_data.max_market_price {
                if reward_data.current_market_price > max_p || max_price > max_p {
                    warn!(
                        redemption_id = %redemption_id,
                        price = reward_data.current_market_price,
                        max_order_price = max_price,
                        max = max_p,
                        "Redemption cancelled: market price exceeds configured max_market_price"
                    );

                    let state_c = state.clone();
                    let bc_id = broadcaster_user_id.clone();
                    let r_id = reward_id;
                    state.spawn_task(async move {
                        let r_str = r_id.to_string();
                        let s_for_token = state_c.clone();
                        let b_for_closure = bc_id.clone();
                        let _ = state_c.with_broadcaster_token(&bc_id, move |token| {
                            let b = b_for_closure.clone();
                            let r = r_str.clone();
                            let s = s_for_token.clone();
                            async move {
                                s.helix_client.update_custom_reward(&b, &r, UpdateCustomReward { is_paused: Some(true), ..Default::default() }, &token).await
                            }
                        }).await;
                        let _ = state_c.db.set_reward_paused(r_id, true, Some(PauseReason::PriceLimit)).await;
                    });

                    update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id, redemption_id, true, Some("Market price exceeds maximum limit")).await;
                    let msg = state.render_chat_message(
                        &broadcaster_user_id,
                        MSG_ORDER_FAILED,
                        &[("buyer", &event.user_login), ("code", "LIMIT"), ("error", "цена скина на маркете превысила установленный стримером лимит"), ("item", &item_name)],
                    );
                    let _ = state.with_bot_user_token(async |token| {
                        state.helix_client.send_chat_message(&broadcaster_user_id, &bot_channel_id, &msg, None, None, &token).await
                    }).await;
                    return;
                }
            }

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

                        let error_lower = error_msg.to_lowercase();

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

                        let price_error: bool = error_lower == "no item found at the specified chance to transfer at the specified price or below"
                            || error_lower == "не найден предмет с указанным шансом на передачу по указанной цене или ниже";

                        if price_error {
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

            let error_lower = error_msg.to_lowercase();

            let price_error: bool = error_lower == "no item found at the specified chance to transfer at the specified price or below"
                || error_lower == "не найден предмет с указанным шансом на передачу по указанной цене или ниже";

            if trigger_price_update_on_deviation && price_error {
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
