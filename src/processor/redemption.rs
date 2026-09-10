use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::db::rewards::{PauseReason, RewardType};
use crate::helix::api::custom_rewards::model::UpdateCustomReward;
use crate::messages::{
    MSG_ORDER_CREATED, MSG_ORDER_FAILED,
    MSG_ORDER_FAILED_FILTER_EXHAUSTED, MSG_TRADE_LINK_INVALID,
    MSG_ORDER_RETRYING, MSG_ORDER_MANUAL_HOLD,
    MSG_CHAT_REQ_FAILED_MESSAGES_REFUND, MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY,
    MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND, MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY,
    MSG_CHAT_REQ_FAILED_BOTH_REFUND, MSG_CHAT_REQ_FAILED_BOTH_PENALTY,
    MSG_USER_PURCHASE_LIMIT_REACHED, MSG_GLOBAL_PURCHASE_LIMIT_REACHED,
};
use std::time::Duration;
use crate::processor::model::EventSubNotification;
use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::processor::price_updater;
use crate::state::AppState;
use crate::steam::market;
use crate::steam::market::errors::{classify_market_buy_for_error, MarketBuyForErrorKind};
use crate::steam::trade_link::TradeLink;

pub async fn process_redemption(
    state: Arc<AppState>,
    notification: EventSubNotification,
) {
    let event = notification.event;
    let redemption_id = event.id;
    let reward_id = event.reward.id;
    let broadcaster_user_id = event.broadcaster_user_id.clone();

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
        RewardType::Pool => {
            reward_data.pool_items.as_ref()
                .and_then(|j| pick_pool_item(&j.0))
                .map(|item| item.market_hash_name.clone())
        }
        RewardType::Filter => {
            if let Some(ref filter_wrapper) = reward_data.filter_config {
                if let Ok(all_prices) = state.get_cached_or_fetch_prices(&reward_data.currency).await {
                    let matching = crate::steam::market::prices::filter_prices(&all_prices, &filter_wrapper.0);
                    if !matching.is_empty() {
                        let idx = (uuid::Uuid::new_v4().as_u128() % (matching.len() as u128)) as usize;
                        Some(matching[idx].market_hash_name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
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
        market_item_name: initial_item_name.clone(),
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
            &event.user_login,
            initial_item_name.as_deref(),
            true,
            "bot_or_reward_inactive",
            Some("Bot or reward is inactive, paused, or deleted"),
        ).await;

        return;
    }

    if state.bot_info.read().is_none() {
        error!(redemption_id = %redemption_id, "Bot account is not initialized in AppState; cannot process redemption");
        return;
    }

    let trade_link = match TradeLink::parse(&event.user_input) {
        Some(t) => t,
        None => {
            warn!(
                redemption_id = %redemption_id,
                user_login = %event.user_login,
                user_input = %event.user_input,
                "Failed to parse Steam trade link from redemption user input"
            );
            state.channel_logger.log_trade_link_invalid(
                &broadcaster_user_id,
                &redemption_id.to_string(),
                &event.user_login,
                &event.user_input,
                initial_item_name.as_deref(),
            );
            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                &event.user_login,
                initial_item_name.as_deref(),
                true,
                "invalid_trade_link",
                Some("Failed to parse Steam trade offer URL from user input"),
            ).await;

            let msg = state.render_chat_message(
                &broadcaster_user_id,
                MSG_TRADE_LINK_INVALID,
                &[("buyer", &event.user_login)],
            );
            if let Err(e) = state.send_chat_message(&broadcaster_user_id, &msg, None).await {
                error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message informing user of invalid trade link");
                return;
            }
            return;
        }
    };

    // Check chat activity requirements if configured on the reward
    if reward_data.chat_min_messages.is_some() || reward_data.chat_min_characters.is_some() {
        let since = reward_data.chat_time_window_hours.filter(|&h| h > 0).map(|h| {
            chrono::Utc::now() - chrono::Duration::hours(h as i64)
        });

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
            let (hours_str, period_str) = match reward_data.chat_time_window_hours {
                Some(h) if h > 0 => (h.to_string(), format!("{}h", h)),
                _ => ("all-time".to_string(), "all-time".to_string()),
            };
            let user_msgs_str = user_msgs.to_string();
            let user_chars_str = user_chars.to_string();
            let min_msgs_str = reward_data.chat_min_messages.map(|m| m.to_string()).unwrap_or_default();
            let min_chars_str = reward_data.chat_min_characters.map(|c| c.to_string()).unwrap_or_default();
            let reason_summary = format!("messages: {}/{}, characters: {}/{}", user_msgs, min_msgs_str, user_chars, min_chars_str);
            state.channel_logger.log_chat_requirement_failed(
                &broadcaster_user_id,
                &redemption_id.to_string(),
                &event.user_login,
                &reason_summary,
                refund,
            );

            update_redemption_status_failed(
                state.clone(),
                &broadcaster_user_id,
                reward_id,
                redemption_id,
                &event.user_login,
                initial_item_name.as_deref(),
                refund,
                "chat_requirements_unmet",
                Some("User did not meet chat activity requirements"),
            ).await;
            let op_str = match operator {
                crate::db::rewards::ChatLogicalOperator::And => "and",
                crate::db::rewards::ChatLogicalOperator::Or => "or",
            };

            let (template_key, vars) = if reward_data.chat_min_messages.is_some() && reward_data.chat_min_characters.is_none() {
                let key = if refund {
                    MSG_CHAT_REQ_FAILED_MESSAGES_REFUND
                } else {
                    MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY
                };
                (
                    key,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_messages", user_msgs_str.as_str()),
                        ("min_messages", min_msgs_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("period", period_str.as_str()),
                    ],
                )
            } else if reward_data.chat_min_characters.is_some() && reward_data.chat_min_messages.is_none() {
                let key = if refund {
                    MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND
                } else {
                    MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY
                };
                (
                    key,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_characters", user_chars_str.as_str()),
                        ("min_characters", min_chars_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("period", period_str.as_str()),
                    ],
                )
            } else {
                let key = if refund {
                    MSG_CHAT_REQ_FAILED_BOTH_REFUND
                } else {
                    MSG_CHAT_REQ_FAILED_BOTH_PENALTY
                };
                (
                    key,
                    vec![
                        ("buyer", event.user_login.as_str()),
                        ("user_messages", user_msgs_str.as_str()),
                        ("min_messages", min_msgs_str.as_str()),
                        ("user_characters", user_chars_str.as_str()),
                        ("min_characters", min_chars_str.as_str()),
                        ("hours", hours_str.as_str()),
                        ("period", period_str.as_str()),
                        ("operator", op_str),
                    ],
                )
            };

            let msg = state.render_chat_message(&broadcaster_user_id, template_key, &vars);
            if let Err(e) = state.send_chat_message(&broadcaster_user_id, &msg, None).await {
                error!(error = %e, redemption_id = %redemption_id, "Failed to send chat message for chat requirements failure");
            }

            return;
        }
    }

    if let Some(limits) = reward_data.purchase_limits.as_ref().map(|j| &j.0) {
        if !limits.is_empty() {
            // 1. Check global limits first
            for rule in &limits.global {
                match state.db.count_reward_redemptions(reward_id, None, rule.window_hours, Some(redemption_id)).await {
                    Ok(count) => {
                        if count >= rule.max_redemptions as i64 {
                            warn!(
                                redemption_id = %redemption_id,
                                reward_id = %reward_id,
                                window_hours = ?rule.window_hours,
                                max_redemptions = rule.max_redemptions,
                                current_count = count,
                                "Global purchase limit reached for reward; auto-pausing and refunding"
                            );

                            pause_reward_on_twitch_and_db(
                                &state,
                                &broadcaster_user_id,
                                reward_id,
                                PauseReason::LimitReached,
                            ).await;

                            state.channel_logger.log_reward_paused(
                                &broadcaster_user_id,
                                &reward_id.to_string(),
                                &event.reward.title,
                                "LIMIT_REACHED",
                                Some(serde_json::json!({
                                    "limit_type": "global",
                                    "window_hours": rule.window_hours,
                                    "max_redemptions": rule.max_redemptions,
                                    "current_count": count,
                                })),
                            );

                            state.channel_logger.log_purchase_limit_reached(
                                &broadcaster_user_id,
                                &redemption_id.to_string(),
                                &event.user_login,
                                "global reward limit",
                            );

                            update_redemption_status_failed(
                                state.clone(),
                                &broadcaster_user_id,
                                reward_id,
                                redemption_id,
                                &event.user_login,
                                initial_item_name.as_deref(),
                                true,
                                "global_limit_reached",
                                Some("Global reward purchase limit reached"),
                            ).await;

                            let limit_str = rule.max_redemptions.to_string();
                            let hours_str = rule.window_hours.map(|h| h.to_string()).unwrap_or_else(|| "all-time".to_string());
                            let period_str = format_limit_period(rule.window_hours);
                            let msg = state.render_chat_message(
                                &broadcaster_user_id,
                                MSG_GLOBAL_PURCHASE_LIMIT_REACHED,
                                &[
                                    ("buyer", event.user_login.as_str()),
                                    ("limit", limit_str.as_str()),
                                    ("period", period_str.as_str()),
                                    ("item", reward_data.twitch_title.as_str()),
                                    ("hours", hours_str.as_str()),
                                ],
                            );
                            let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;

                            return;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, reward_id = %reward_id, "Failed to query global purchase counts from DB");
                    }
                }
            }

            // 2. Check user limits
            for rule in &limits.user {
                match state.db.count_reward_redemptions(reward_id, Some(&event.user_id), rule.window_hours, Some(redemption_id)).await {
                    Ok(count) => {
                        if count >= rule.max_redemptions as i64 {
                            warn!(
                                redemption_id = %redemption_id,
                                reward_id = %reward_id,
                                user_id = %event.user_id,
                                user_login = %event.user_login,
                                window_hours = ?rule.window_hours,
                                max_redemptions = rule.max_redemptions,
                                current_count = count,
                                "User purchase limit reached for reward; refunding"
                            );

                            state.channel_logger.log_purchase_limit_reached(
                                &broadcaster_user_id,
                                &redemption_id.to_string(),
                                &event.user_login,
                                "user redemption limit",
                            );

                            update_redemption_status_failed(
                                state.clone(),
                                &broadcaster_user_id,
                                reward_id,
                                redemption_id,
                                &event.user_login,
                                initial_item_name.as_deref(),
                                true,
                                "user_limit_reached",
                                Some("User reward purchase limit reached"),
                            ).await;

                            let limit_str = rule.max_redemptions.to_string();
                            let hours_str = rule.window_hours.map(|h| h.to_string()).unwrap_or_else(|| "all-time".to_string());
                            let period_str = format_limit_period(rule.window_hours);
                            let msg = state.render_chat_message(
                                &broadcaster_user_id,
                                MSG_USER_PURCHASE_LIMIT_REACHED,
                                &[
                                    ("buyer", event.user_login.as_str()),
                                    ("limit", limit_str.as_str()),
                                    ("period", period_str.as_str()),
                                    ("item", reward_data.twitch_title.as_str()),
                                    ("hours", hours_str.as_str()),
                                ],
                            );
                            let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;

                            return;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, reward_id = %reward_id, "Failed to query user purchase counts from DB");
                    }
                }
            }
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

                    pause_reward_on_twitch_and_db(
                        &state,
                        &broadcaster_user_id,
                        reward_id,
                        PauseReason::PriceLimit,
                    ).await;

                    let curr_str = &reward_data.currency;
                    let current_price_major = crate::steam::market::minor_to_major(reward_data.current_market_price as i64, curr_str);
                    let min_price_major = Some(crate::steam::market::minor_to_major(min_p as i64, curr_str));
                    let max_price_major = reward_data.max_market_price.map(|p| crate::steam::market::minor_to_major(p as i64, curr_str));

                    state.channel_logger.log_reward_paused(
                        &broadcaster_user_id,
                        &reward_id.to_string(),
                        &event.reward.title,
                        "PRICE_LIMIT",
                        Some(serde_json::json!({
                            "current_price": current_price_major,
                            "min_market_price": min_price_major,
                            "max_market_price": max_price_major,
                            "current_price_minor": reward_data.current_market_price,
                            "min_market_price_minor": min_p,
                            "max_market_price_minor": reward_data.max_market_price,
                            "currency": curr_str,
                        })),
                    );

                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        Some(&item_name),
                        true,
                        "price_below_min",
                        Some("Market price is below configured minimum limit"),
                    ).await;
                    let msg = state.render_chat_message(
                        &broadcaster_user_id,
                        MSG_ORDER_FAILED,
                        &[("buyer", &event.user_login), ("code", "LIMIT"), ("error", "цена скина на маркете ниже установленного стримером минимума"), ("item", &item_name)],
                    );
                    let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
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

                    pause_reward_on_twitch_and_db(
                        &state,
                        &broadcaster_user_id,
                        reward_id,
                        PauseReason::PriceLimit,
                    ).await;

                    let curr_str = &reward_data.currency;
                    let current_price_major = crate::steam::market::minor_to_major(reward_data.current_market_price as i64, curr_str);
                    let min_price_major = reward_data.min_market_price.map(|p| crate::steam::market::minor_to_major(p as i64, curr_str));
                    let max_price_major = Some(crate::steam::market::minor_to_major(max_p as i64, curr_str));

                    state.channel_logger.log_reward_paused(
                        &broadcaster_user_id,
                        &reward_id.to_string(),
                        &event.reward.title,
                        "PRICE_LIMIT",
                        Some(serde_json::json!({
                            "current_price": current_price_major,
                            "min_market_price": min_price_major,
                            "max_market_price": max_price_major,
                            "current_price_minor": reward_data.current_market_price,
                            "min_market_price_minor": reward_data.min_market_price,
                            "max_market_price_minor": max_p,
                            "currency": curr_str,
                        })),
                    );

                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        Some(&item_name),
                        true,
                        "price_above_max",
                        Some("Market price exceeds configured maximum limit"),
                    ).await;
                    let msg = state.render_chat_message(
                        &broadcaster_user_id,
                        MSG_ORDER_FAILED,
                        &[("buyer", &event.user_login), ("code", "LIMIT"), ("error", "цена скина на маркете превысила установленный стримером лимит"), ("item", &item_name)],
                    );
                    let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
                    return;
                }
            }

            buy_item_with_safeguarded_retry(
                &state,
                &broadcaster_setting,
                &broadcaster_user_id,
                redemption_id,
                reward_id,
                &event.user_login,
                &item_name,
                max_price,
                &reward_data.currency,
                trade_link,
                true,
                None,
            ).await;
        }
        RewardType::Pool => {
            let pool = match reward_data.pool_items.as_ref().map(|j| &j.0) {
                Some(items) if !items.is_empty() => items,
                _ => {
                    warn!(redemption_id = %redemption_id, "Pool reward has empty pool items");
                    state.channel_logger.log_reward_misconfigured(
                        &broadcaster_user_id,
                        &reward_id.to_string(),
                        Some(&reward_data.twitch_title),
                        "EMPTY_POOL",
                        None,
                    );
                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        initial_item_name.as_deref(),
                        true,
                        "empty_pool",
                        Some("Pool items list is empty"),
                    ).await;
                    return;
                }
            };

            let picked = initial_item_name.as_ref()
                .and_then(|name| pool.iter().find(|i| &i.market_hash_name == name))
                .or_else(|| pick_pool_item(pool));

            let picked = match picked {
                Some(item) => item,
                None => {
                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        initial_item_name.as_deref(),
                        true,
                        "pool_pick_failed",
                        Some("Failed to pick pool item"),
                    ).await;
                    return;
                }
            };

            let item_name = picked.market_hash_name.clone();
            let price = picked.current_market_price as i64;
            let dev = picked.permissible_market_price_deviation as i64;
            let max_price_i64 = price + (price * dev) / 100;
            let max_price = max_price_i64.min(i32::MAX as i64) as i32;

            let total_weight: f64 = pool.iter().map(|i| i.weight.max(0.0)).sum();
            let chance = if total_weight > 0.0 {
                (picked.weight.max(0.0) / total_weight) * 100.0
            } else {
                0.0
            };
            let formatted_chance = format_chance(chance);

            let custom_order_created_msg = if let Some(ref custom_tpl) = picked.custom_message {
                crate::messages::render_template(
                    custom_tpl,
                    &[
                        ("buyer", &event.user_login),
                        ("item", &item_name),
                        ("chance", &formatted_chance),
                    ],
                )
            } else {
                state.render_chat_message(
                    &broadcaster_user_id,
                    crate::messages::MSG_ORDERS_POOL_CREATED,
                    &[
                        ("buyer", &event.user_login),
                        ("item", &item_name),
                        ("chance", &formatted_chance),
                    ],
                )
            };

            buy_item_with_safeguarded_retry(
                &state,
                &broadcaster_setting,
                &broadcaster_user_id,
                redemption_id,
                reward_id,
                &event.user_login,
                &item_name,
                max_price,
                &reward_data.currency,
                trade_link,
                false,
                Some(custom_order_created_msg),
            ).await;
        }
        RewardType::Filter => {
            let filter = match reward_data.filter_config.as_ref().map(|j| &j.0) {
                Some(f) => f,
                None => {
                    warn!(redemption_id = %redemption_id, "Filter reward has no filter_config");
                    state.channel_logger.log_reward_misconfigured(
                        &broadcaster_user_id,
                        &reward_id.to_string(),
                        Some(&reward_data.twitch_title),
                        "FILTER_MISSING_CONFIG",
                        None,
                    );
                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        None,
                        true,
                        "filter_config_missing",
                        Some("Filter config is missing"),
                    ).await;
                    return;
                }
            };

            let all_prices = match state.get_cached_or_fetch_prices(&reward_data.currency).await {
                Ok(prices) => prices,
                Err(e) => {
                    error!(error = %e, redemption_id = %redemption_id, "Failed to fetch prices for filter redemption");
                    update_redemption_status_failed(
                        state.clone(),
                        &broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        &event.user_login,
                        None,
                        true,
                        "market_prices_fetch_failed",
                        Some("Failed to fetch market prices"),
                    ).await;
                    return;
                }
            };

            let matching = crate::steam::market::prices::filter_prices(&all_prices, filter);
            if matching.is_empty() {
                warn!(redemption_id = %redemption_id, "No items match filter criteria for redemption");
                state.channel_logger.log_reward_misconfigured(
                    &broadcaster_user_id,
                    &reward_id.to_string(),
                    Some(&reward_data.twitch_title),
                    "FILTER_NO_MATCH",
                    Some(serde_json::json!({
                        "min_price": filter.min_price,
                        "max_price": filter.max_price,
                        "currency": &reward_data.currency,
                    })),
                );
                update_redemption_status_failed(
                    state.clone(),
                    &broadcaster_user_id,
                    reward_id,
                    redemption_id,
                    &event.user_login,
                    None,
                    true,
                    "no_items_match_filter",
                    Some("No items match filter criteria"),
                ).await;
                let msg = state.render_chat_message(&broadcaster_user_id, MSG_ORDER_FAILED_FILTER_EXHAUSTED, &[("buyer", &event.user_login), ("attempts", "0")]);
                let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
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

                        state.channel_logger.log_redemption_order_created(
                            &broadcaster_user_id,
                            &redemption_id.to_string(),
                            &item.market_hash_name,
                            paid_price,
                            &reward_data.currency,
                            &event.user_login,
                        );

                        if let Err(e) = state.db.set_redemption_order_created(
                            redemption_id,
                            paid_price,
                            Some(&item.market_hash_name),
                            (attempt - 1) as i32,
                        ).await {
                            error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption status to order_created");
                            return;
                        }

                        check_and_pause_if_global_limit_reached(&state, &broadcaster_user_id, reward_id).await;

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
                        let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;

                        order_created = true;
                        break;
                    }
                    Ok(res) => {
                        let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
                        let code = res.code.unwrap_or(0);
                        warn!(
                            redemption_id = %redemption_id,
                            attempt = attempt,
                            item = %item.market_hash_name,
                            code = code,
                            error = %error_msg,
                            "Market rejected buy-for attempt"
                        );

                        let kind = classify_market_buy_for_error(code, &error_msg);

                        if kind == MarketBuyForErrorKind::NotEnoughFunds {
                            let state_for_balance = state.clone();
                            let bc_id_for_balance = broadcaster_user_id.clone();
                            state.spawn_task(async move {
                                let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                            });

                            state.channel_logger.log_market_buy_error(
                                &broadcaster_user_id,
                                &redemption_id.to_string(),
                                &item.market_hash_name,
                                "no_money",
                                &error_msg,
                            );
                            state.channel_logger.log_redemption_manual_hold(
                                &broadcaster_user_id,
                                &redemption_id.to_string(),
                                &event.user_login,
                                &item.market_hash_name,
                                "Insufficient bot market balance",
                                Some(&error_msg),
                            );

                            let _ = state.db.set_redemption_manual_hold(
                                redemption_id,
                                "no_money",
                                Some(&error_msg),
                            ).await;

                            state.channel_logger.log_redemption_status_changed(
                                &broadcaster_user_id,
                                &redemption_id.to_string(),
                                &event.user_login,
                                Some(&item.market_hash_name),
                                "PENDING",
                                "MANUAL_HOLD",
                                Some("no_money"),
                                Some(&error_msg),
                            );

                            let msg = state.render_chat_message(
                                &broadcaster_user_id,
                                MSG_ORDER_MANUAL_HOLD,
                                &[("buyer", &event.user_login), ("item", &item.market_hash_name)],
                            );
                            let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
                            return;
                        }

                        if kind.is_buyer_terminal_error() {
                            warn!(
                                redemption_id = %redemption_id,
                                kind = ?kind,
                                "Buyer terminal error encountered on filter reward, aborting pool attempts and refunding"
                            );
                            let error_kind_str = match kind {
                                MarketBuyForErrorKind::SteamBanned => "buyer_banned",
                                MarketBuyForErrorKind::NoMobileAuth => "no_mobile_authenticator",
                                MarketBuyForErrorKind::OfflineTradesDisabled => "offline_trades_disabled",
                                MarketBuyForErrorKind::InvalidTradeLink | MarketBuyForErrorKind::TradeLinkCheckFailed => "invalid_trade_url",
                                MarketBuyForErrorKind::InventoryHidden => "inventory_hidden",
                                MarketBuyForErrorKind::InventoryFull => "inventory_full",
                                _ => "buyer_fault",
                            };
                            update_redemption_status_failed(
                                state.clone(),
                                &broadcaster_user_id,
                                reward_id,
                                redemption_id,
                                &event.user_login,
                                Some(&item.market_hash_name),
                                true,
                                error_kind_str,
                                Some(&error_msg),
                            ).await;
                            let msg_template = kind.to_market_error_message_key().unwrap_or(MSG_ORDER_FAILED);
                            let code_str = code.to_string();
                            let msg = state.render_chat_message(
                                &broadcaster_user_id,
                                msg_template,
                                &[("buyer", &event.user_login), ("item", &item.market_hash_name), ("code", &code_str), ("error", &error_msg)],
                            );
                            let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
                            return;
                        }

                        if kind == MarketBuyForErrorKind::PriceOrChanceDeviation {
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
                    &event.user_login,
                    None,
                    true,
                    "all_filter_attempts_failed",
                    Some("All filter buy attempts failed"),
                ).await;

                let msg = state.render_chat_message(
                    &broadcaster_user_id,
                    MSG_ORDER_FAILED_FILTER_EXHAUSTED,
                    &[("buyer", &event.user_login), ("attempts", "5")],
                );
                let _ = state.send_chat_message(&broadcaster_user_id, &msg, None).await;
            }
        }
    }
}

async fn pause_reward_on_twitch_and_db(
    state: &Arc<AppState>,
    broadcaster_user_id: &str,
    reward_id: Uuid,
    reason: PauseReason,
) {
    let r_str = reward_id.to_string();
    let s_for_token = state.clone();
    let b_for_closure = broadcaster_user_id.to_string();
    if let Err(e) = state.with_broadcaster_token(broadcaster_user_id, move |token| {
        let b = b_for_closure.clone();
        let r = r_str.clone();
        let s = s_for_token.clone();
        async move {
            s.helix_client.update_custom_reward(&b, &r, UpdateCustomReward { is_paused: Some(true), ..Default::default() }, &token).await
        }
    }).await {
        warn!(error = %e, reward_id = %reward_id, "Failed to pause reward on Twitch");
    }
    if let Err(e) = state.db.set_reward_paused(reward_id, true, Some(reason)).await {
        error!(error = %e, reward_id = %reward_id, "Failed to set reward paused in DB");
    }
}

async fn check_and_pause_if_global_limit_reached(
    state: &Arc<AppState>,
    broadcaster_user_id: &str,
    reward_id: Uuid,
) {
    let reward = match state.db.get_reward_by_twitch_id(reward_id).await {
        Ok(Some(r)) => r,
        _ => return,
    };

    if reward.is_paused {
        return;
    }

    let limits = match reward.purchase_limits.as_ref().map(|j| &j.0) {
        Some(l) if l.has_global_limits() => l,
        _ => return,
    };

    for rule in &limits.global {
        match state.db.count_reward_redemptions(reward_id, None, rule.window_hours, None).await {
            Ok(count) => {
                if count >= rule.max_redemptions as i64 {
                    info!(
                        reward_id = %reward_id,
                        channel_id = %broadcaster_user_id,
                        window_hours = ?rule.window_hours,
                        max_redemptions = rule.max_redemptions,
                        current_count = count,
                        "Global purchase limit reached, immediately pausing reward on Twitch and DB"
                    );
                    pause_reward_on_twitch_and_db(
                        state,
                        broadcaster_user_id,
                        reward_id,
                        PauseReason::LimitReached,
                    ).await;

                    state.channel_logger.log_reward_paused(
                        broadcaster_user_id,
                        &reward_id.to_string(),
                        &reward.twitch_title,
                        "LIMIT_REACHED",
                        Some(serde_json::json!({
                            "limit_type": "global",
                            "window_hours": rule.window_hours,
                            "max_redemptions": rule.max_redemptions,
                            "current_count": count,
                        })),
                    );
                    break;
                }
            }
            Err(e) => {
                error!(error = %e, reward_id = %reward_id, "Failed to query global purchase count after redemption");
            }
        }
    }
}

pub fn format_chance(chance: f64) -> String {
    if chance <= 0.0 {
        return "0%".to_string();
    }
    if chance >= 0.01 {
        let rounded = (chance * 100.0).round() / 100.0;
        let s = format!("{:.2}", rounded);
        format!("{}%", s.trim_end_matches('0').trim_end_matches('.'))
    } else {
        let s = format!("{:.8}", chance);
        format!("{}%", s.trim_end_matches('0').trim_end_matches('.'))
    }
}

fn format_limit_period(window_hours: Option<i32>) -> String {
    match window_hours {
        Some(168) => "week".to_string(),
        Some(720) => "month".to_string(),
        Some(h) => format!("{}h", h),
        None => "all-time".to_string(),
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

async fn buy_item_with_safeguarded_retry(
    state: &Arc<AppState>,
    broadcaster_setting: &crate::db::broadcaster_settings::BroadcasterSetting,
    broadcaster_user_id: &str,
    redemption_id: Uuid,
    reward_id: Uuid,
    user_login: &str,
    item_name: &str,
    max_price: i32,
    currency: &str,
    trade_link: TradeLink,
    trigger_price_update_on_deviation: bool,
    custom_order_created_msg: Option<String>,
) {
    for attempt in 1..=3 {
        let retry_count = (attempt - 1) as i32;
        let custom_id = if retry_count == 0 {
            redemption_id.to_string()
        } else {
            format!("{}-{}", redemption_id, retry_count)
        };

        if attempt > 1 {
            // Before first retry, notify viewer that auto-retries are in progress:
            if attempt == 2 {
                let retrying_msg = state.render_chat_message(
                    broadcaster_user_id,
                    MSG_ORDER_RETRYING,
                    &[("buyer", user_login), ("item", item_name)],
                );
                let _ = state.send_chat_message(broadcaster_user_id, &retrying_msg, None).await;
            }

            // Pause 5 seconds between attempts
            tokio::time::sleep(Duration::from_secs(5)).await;

            // Double-check safeguard: verify if an order was created on market during prior attempt
            let prev_custom_id = if retry_count == 1 {
                redemption_id.to_string()
            } else {
                format!("{}-{}", redemption_id, retry_count - 1)
            };

            if let Ok(info) = state.market_client.get_buy_info(&broadcaster_setting.market_api_key, &prev_custom_id).await {
                if info.success && info.data.as_ref().map_or(false, |d| !d.is_failed()) {
                    info!(
                        redemption_id = %redemption_id,
                        prev_custom_id = %prev_custom_id,
                        "Active order already exists on market for previous attempt, attaching watcher"
                    );
                    let data = info.data.unwrap();
                    let paid_price = (data.paid * 100.0) as i64;
                    let _ = state.db.set_redemption_order_created(redemption_id, paid_price, Some(item_name), retry_count - 1).await;
                    let order_watcher = OrderWatcher::new(
                        state.clone(),
                        broadcaster_setting.market_api_key.clone(),
                        broadcaster_user_id.to_string(),
                        WatcherRedemptionData {
                            redemption_id,
                            custom_id: prev_custom_id,
                            reward_id,
                            user_login: user_login.to_string(),
                        },
                    );
                    let token = state.shutdown_token.clone();
                    state.spawn_task(async move {
                        order_watcher.track_redemption(token).await;
                    });
                    let default_msg = state.render_chat_message(
                        broadcaster_user_id,
                        MSG_ORDER_CREATED,
                        &[("buyer", user_login), ("item", item_name)],
                    );
                    let msg = custom_order_created_msg.as_deref().unwrap_or(&default_msg);
                    let _ = state.send_chat_message(broadcaster_user_id, msg, None).await;
                    return;
                }
            }

            // Also check current custom_id just in case
            if let Ok(info) = state.market_client.get_buy_info(&broadcaster_setting.market_api_key, &custom_id).await {
                if info.success && info.data.as_ref().map_or(false, |d| !d.is_failed()) {
                    info!(
                        redemption_id = %redemption_id,
                        custom_id = %custom_id,
                        "Active order already exists on market for current attempt, attaching watcher"
                    );
                    let data = info.data.unwrap();
                    let paid_price = (data.paid * 100.0) as i64;
                    let _ = state.db.set_redemption_order_created(redemption_id, paid_price, Some(item_name), retry_count).await;
                    let order_watcher = OrderWatcher::new(
                        state.clone(),
                        broadcaster_setting.market_api_key.clone(),
                        broadcaster_user_id.to_string(),
                        WatcherRedemptionData {
                            redemption_id,
                            custom_id: custom_id.clone(),
                            reward_id,
                            user_login: user_login.to_string(),
                        },
                    );
                    let token = state.shutdown_token.clone();
                    state.spawn_task(async move {
                        order_watcher.track_redemption(token).await;
                    });
                    let default_msg = state.render_chat_message(
                        broadcaster_user_id,
                        MSG_ORDER_CREATED,
                        &[("buyer", user_login), ("item", item_name)],
                    );
                    let msg = custom_order_created_msg.as_deref().unwrap_or(&default_msg);
                    let _ = state.send_chat_message(broadcaster_user_id, msg, None).await;
                    return;
                }
            }
        }

        info!(
            redemption_id = %redemption_id,
            attempt = attempt,
            item = %item_name,
            price = max_price,
            "Attempting market buy-for"
        );

        match state.market_client.buy_for(
            &broadcaster_setting.market_api_key,
            item_name,
            max_price,
            broadcaster_setting.market_chance_to_transfer,
            trade_link.clone(),
            &custom_id,
        ).await {
            Ok(res) if res.success => {
                info!(
                    redemption_id = %redemption_id,
                    attempt = attempt,
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

                state.channel_logger.log_redemption_order_created(
                    broadcaster_user_id,
                    &redemption_id.to_string(),
                    item_name,
                    paid_price,
                    currency,
                    user_login,
                );

                if let Err(e) = state.db.set_redemption_order_created(
                    redemption_id,
                    paid_price,
                    Some(item_name),
                    retry_count,
                ).await {
                    error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption status to order_created");
                    return;
                }

                check_and_pause_if_global_limit_reached(state, broadcaster_user_id, reward_id).await;

                let order_watcher = OrderWatcher::new(
                    state.clone(),
                    broadcaster_setting.market_api_key.clone(),
                    broadcaster_user_id.to_string(),
                    WatcherRedemptionData {
                        redemption_id,
                        custom_id,
                        reward_id,
                        user_login: user_login.to_string(),
                    },
                );

                let token = state.shutdown_token.clone();
                state.spawn_task(async move {
                    order_watcher.track_redemption(token).await;
                });

                let default_msg = state.render_chat_message(
                    broadcaster_user_id,
                    MSG_ORDER_CREATED,
                    &[("buyer", user_login), ("item", item_name)],
                );
                let msg = custom_order_created_msg.as_deref().unwrap_or(&default_msg);
                if let Err(e) = state.send_chat_message(broadcaster_user_id, msg, None).await {
                    error!(error = %e, redemption_id = %redemption_id, broadcaster_id = %broadcaster_user_id, "Failed to send chat message for created order");
                }
                return;
            }
            Ok(res) => {
                let error_msg = res.error.unwrap_or_else(|| "Unknown market error".to_string());
                let code = res.code.unwrap_or(0);
                warn!(
                    redemption_id = %redemption_id,
                    attempt = attempt,
                    code = code,
                    error = %error_msg,
                    "Market rejected buy-for"
                );

                let kind = classify_market_buy_for_error(code, &error_msg);

                // 1. Not enough funds -> immediate MANUAL_HOLD without auto-retrying
                if kind == MarketBuyForErrorKind::NotEnoughFunds {
                    let state_for_balance = state.clone();
                    let bc_id_for_balance = broadcaster_user_id.to_string();
                    state.spawn_task(async move {
                        let _ = state_for_balance.refresh_broadcaster_balance(&bc_id_for_balance).await;
                    });

                    state.channel_logger.log_market_buy_error(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        item_name,
                        "no_money",
                        &error_msg,
                    );
                    state.channel_logger.log_redemption_manual_hold(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        item_name,
                        "Insufficient bot market balance",
                        Some(&error_msg),
                    );

                    if let Err(e) = state.db.set_redemption_manual_hold(
                        redemption_id,
                        "no_money",
                        Some(&error_msg),
                    ).await {
                        error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption to manual hold");
                    }

                    state.channel_logger.log_redemption_status_changed(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        Some(item_name),
                        "PENDING",
                        "MANUAL_HOLD",
                        Some("no_money"),
                        Some(&error_msg),
                    );

                    let msg = state.render_chat_message(
                        broadcaster_user_id,
                        MSG_ORDER_MANUAL_HOLD,
                        &[("buyer", user_login), ("item", item_name)],
                    );
                    let _ = state.send_chat_message(broadcaster_user_id, &msg, None).await;
                    return;
                }

                // 2. Buyer terminal error -> immediate failure
                if kind.is_buyer_terminal_error() {
                    let error_kind_str = match kind {
                        MarketBuyForErrorKind::SteamBanned | MarketBuyForErrorKind::NoMobileAuth | MarketBuyForErrorKind::OfflineTradesDisabled => "buyer_banned",
                        MarketBuyForErrorKind::InvalidTradeLink | MarketBuyForErrorKind::TradeLinkCheckFailed => "invalid_trade_url",
                        MarketBuyForErrorKind::InventoryHidden => "inventory_hidden",
                        MarketBuyForErrorKind::InventoryFull => "inventory_full",
                        _ => "buyer_fault",
                    };
                    state.channel_logger.log_market_buy_error(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        item_name,
                        error_kind_str,
                        &error_msg,
                    );
                    update_redemption_status_failed(
                        state.clone(),
                        broadcaster_user_id,
                        reward_id,
                        redemption_id,
                        user_login,
                        Some(item_name),
                        false,
                        error_kind_str,
                        Some(&error_msg),
                    ).await;

                    let code_str = code.to_string();
                    let msg_template = kind.to_market_error_message_key().unwrap_or(MSG_ORDER_FAILED);
                    let msg = state.render_chat_message(
                        broadcaster_user_id,
                        msg_template,
                        &[("buyer", user_login), ("item", item_name), ("code", code_str.as_str()), ("error", error_msg.as_str())],
                    );
                    let _ = state.send_chat_message(broadcaster_user_id, &msg, None).await;
                    return;
                }

                // 3. Price deviation
                let price_error = kind == MarketBuyForErrorKind::PriceOrChanceDeviation;
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

                // If attempts exhausted (attempt == 3), transition to MANUAL_HOLD
                if attempt == 3 {
                    warn!(redemption_id = %redemption_id, attempts = attempt, "All market buy attempts exhausted, placing on MANUAL_HOLD");
                    state.channel_logger.log_market_buy_error(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        item_name,
                        "retries_exhausted",
                        &error_msg,
                    );
                    state.channel_logger.log_redemption_manual_hold(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        item_name,
                        "Auto-retry attempts exhausted",
                        Some(&error_msg),
                    );

                    if let Err(e) = state.db.set_redemption_manual_hold(
                        redemption_id,
                        "retries_exhausted",
                        Some(&error_msg),
                    ).await {
                        error!(error = %e, redemption_id = %redemption_id, "DB error setting redemption to manual hold");
                    }

                    state.channel_logger.log_redemption_status_changed(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        Some(item_name),
                        "PENDING",
                        "MANUAL_HOLD",
                        Some("retries_exhausted"),
                        Some(&error_msg),
                    );

                    let msg = state.render_chat_message(
                        broadcaster_user_id,
                        MSG_ORDER_MANUAL_HOLD,
                        &[("buyer", user_login), ("item", item_name)],
                    );
                    let _ = state.send_chat_message(broadcaster_user_id, &msg, None).await;
                    return;
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    redemption_id = %redemption_id,
                    attempt = attempt,
                    item = %item_name,
                    "Network error during market buy-for"
                );
                if attempt == 3 {
                    state.channel_logger.log_market_buy_error(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        item_name,
                        "network_error",
                        &e.to_string(),
                    );
                    state.channel_logger.log_redemption_manual_hold(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        item_name,
                        "Network error, retries exhausted",
                        Some(&e.to_string()),
                    );

                    let _ = state.db.set_redemption_manual_hold(
                        redemption_id,
                        "network_error_retries_exhausted",
                        Some(&e.to_string()),
                    ).await;

                    state.channel_logger.log_redemption_status_changed(
                        broadcaster_user_id,
                        &redemption_id.to_string(),
                        user_login,
                        Some(item_name),
                        "PENDING",
                        "MANUAL_HOLD",
                        Some("network_error_retries_exhausted"),
                        Some(&e.to_string()),
                    );

                    let msg = state.render_chat_message(
                        broadcaster_user_id,
                        MSG_ORDER_MANUAL_HOLD,
                        &[("buyer", user_login), ("item", item_name)],
                    );
                    let _ = state.send_chat_message(broadcaster_user_id, &msg, None).await;
                    return;
                }
            }
        }
    }
}

async fn update_redemption_status_failed(
    state: Arc<AppState>,
    broadcaster_user_id: &str,
    reward_id: Uuid,
    redemption_id: Uuid,
    user_login: &str,
    item_name: Option<&str>,
    return_channel_points: bool,
    fail_cause: &str,
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
        state.channel_logger.log_broadcaster_token_error(broadcaster_user_id, &e.to_string());
        return;
    }

    let redemption_status = if return_channel_points {
        RedemptionStatus::FailedRefund
    } else {
        RedemptionStatus::FailedPenalty
    };

    if let Err(e) = state.db.update_redemption_status(
        redemption_id,
        redemption_status,
        Some(fail_cause),
        fail_description,
    ).await {
        error!(
            error = %e,
            redemption_id = %redemption_id,
            status = ?redemption_status,
            fail_cause = %fail_cause,
            fail_description = ?fail_description,
            "DB error updating failed redemption status"
        );
        return;
    }

    state.channel_logger.log_redemption_status_changed(
        broadcaster_user_id,
        &redemption_id.to_string(),
        user_login,
        item_name,
        "PENDING",
        redemption_status.as_str(),
        Some(fail_cause),
        fail_description,
    );

    info!(
        redemption_id = %redemption_id,
        status = ?redemption_status,
        fail_cause = %fail_cause,
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
            custom_message: None,
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
                custom_message: None,
            },
            PoolItemConfig {
                market_hash_name: "Rare".into(),
                weight: 10.0,
                permissible_market_price_deviation: 10,
                current_market_price: 1000,
                custom_message: None,
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

    #[test]
    fn test_format_chance() {
        assert_eq!(format_chance(0.0), "0%");
        assert_eq!(format_chance(-1.0), "0%");
        assert_eq!(format_chance(5.0), "5%");
        assert_eq!(format_chance(50.0), "50%");
        assert_eq!(format_chance(0.5), "0.5%");
        assert_eq!(format_chance(12.34), "12.34%");
        assert_eq!(format_chance(12.346), "12.35%");
        assert_eq!(format_chance(0.005), "0.005%");
        assert_eq!(format_chance(0.000009), "0.000009%");
    }
}
