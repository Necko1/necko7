use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;
use crate::db::redemptions::{NewRedemption, RedemptionStatus};
use crate::db::rewards::{PauseReason, RewardType};
use crate::helix::api::custom_rewards::model::UpdateCustomReward;
use crate::messages::{
    MSG_CHAT_REQ_FAILED_MESSAGES_REFUND, MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY,
    MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND, MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY,
    MSG_CHAT_REQ_FAILED_BOTH_REFUND, MSG_CHAT_REQ_FAILED_BOTH_PENALTY,
    MSG_USER_PURCHASE_LIMIT_REACHED, MSG_GLOBAL_PURCHASE_LIMIT_REACHED,
    MSG_ORDERS_WAITING_VIEWER, MSG_ORDERS_WAITING_OPERATOR, MSG_ORDERS_REDEEMED,
};
use crate::processor::model::EventSubNotification;
use crate::state::AppState;
use crate::steam::market;

pub async fn process_redemption(
    state: Arc<AppState>,
    notification: EventSubNotification,
) {
    process_redemption_inner(state, notification, false).await;
}

pub async fn resume_pending_inventory_resolution(state: Arc<AppState>, redemption_id: Uuid) {
    let redemption = match state.db.get_redemption(redemption_id).await {
        Ok(Some(row)) if row.status == RedemptionStatus::Pending => row,
        _ => return,
    };
    if state.db.inventory_exists(redemption_id).await.unwrap_or(true) { return; }
    let reward = match state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await {
        Ok(Some(row)) => row,
        _ => return,
    };
    let notification = EventSubNotification {
        subscription: crate::processor::model::EventSubSubscription { id: "inventory-recovery".into(), r#type: "recovery".into() },
        event: crate::processor::model::RedemptionEvent {
            id: redemption_id, broadcaster_user_id: reward.streamer_id.clone(),
            broadcaster_user_login: reward.streamer_id.clone(),
            user_id: redemption.user_id, user_login: redemption.user_login.clone(),
            user_name: redemption.user_login, user_input: redemption.user_trade_link,
            status: "unfulfilled".into(),
            reward: crate::processor::model::RedemptionReward {
                id: reward.twitch_id, title: reward.twitch_title,
                cost: redemption.twitch_points_cost, prompt: None,
            },
            redeemed_at: redemption.created_at,
        },
    };
    process_redemption_inner(state, notification, true).await;
}

async fn process_redemption_inner(
    state: Arc<AppState>,
    notification: EventSubNotification,
    resuming: bool,
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

    let stored_item_name = if resuming {
        match state.db.get_redemption(redemption_id).await {
            Ok(Some(row)) => row.market_item_name,
            _ => return,
        }
    } else { None };
    let initial_item_name = if stored_item_name.is_some() { stored_item_name } else { match reward_data.reward_type {
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
    }};

    if !resuming { match state.db.insert_redemption_if_new(&NewRedemption {
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
        Ok(Some(_)) => {
            crate::processor::inventory_fulfillment::send_inventory_chat(
                &state, &broadcaster_user_id, MSG_ORDERS_REDEEMED,
                &event.user_login, &event.reward.title, &[],
            ).await;
        },
        Ok(None) => {
            info!(redemption_id = %redemption_id, "Redemption is already being processed, ignoring duplicate");
            return;
        }
        Err(e) => {
            error!(error = %e, redemption_id = %redemption_id, reward_id = %reward_id, "DB error inserting new redemption record");
            return;
        }
    }}

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

    // Selection is performed once. The one selected name and ceiling are committed
    // together, before any external buy-for call, regardless of auto-buy settings.
    let selected: Option<(String, i32)> = match reward_data.reward_type {
        RewardType::Fixed => {
            let name = initial_item_name.clone().filter(|n| !n.is_empty());
            if name != reward_data.market_item_name {
                warn!(%redemption_id, "Fixed reward item changed before inventory could be created; operator review required");
                return;
            }
            let price = reward_data.current_market_price as i64;
            let ceiling = price + price * reward_data.permissible_market_price_deviation as i64 / 100;
            if reward_data.min_market_price.is_some_and(|min| price < min as i64)
                || reward_data.max_market_price.is_some_and(|max| ceiling > max as i64) {
                update_redemption_status_failed(state.clone(), &broadcaster_user_id, reward_id,
                    redemption_id, &event.user_login, name.as_deref(), true,
                    "price_limit", Some("Configured reward price limit exceeded")).await;
                return;
            }
            name.map(|name| (name, ceiling.clamp(0, i32::MAX as i64) as i32))
        }
        RewardType::Pool => reward_data.pool_items.as_ref().and_then(|pool| {
            initial_item_name.as_ref().and_then(|name| pool.0.iter().find(|item| &item.market_hash_name == name))
        }).map(|item| {
            let price = item.current_market_price as i64;
            let ceiling = price + price * item.permissible_market_price_deviation as i64 / 100;
            (item.market_hash_name.clone(), ceiling.clamp(0, i32::MAX as i64) as i32)
        }),
        RewardType::Filter => {
            if let Some(filter) = reward_data.filter_config.as_ref() {
                match state.get_cached_or_fetch_prices(&reward_data.currency).await {
                    Ok(prices) => {
                        let matching = crate::steam::market::prices::filter_prices(&prices, &filter.0);
                        let chosen = if let Some(ref name) = initial_item_name {
                            matching.iter().find(|item| &item.market_hash_name == name)
                        } else {
                            matching.first()
                        };
                        chosen.map(|item| {
                            let price = market::major_to_minor(item.price, &reward_data.currency);
                            let cap = market::major_to_minor(filter.0.max_price, &reward_data.currency);
                            let base = price.min(cap);
                            let ceiling = base + base * reward_data.permissible_market_price_deviation as i64 / 100;
                            (item.market_hash_name.clone(), ceiling.clamp(0, i32::MAX as i64) as i32)
                        })
                    }
                    Err(e) => { warn!(error = %e, %redemption_id, "Cannot resolve filter item yet"); None }
                }
            } else { None }
        }
    };
    let Some((item_name, fixed_price)) = selected else {
        warn!(%redemption_id, "No concrete item could be selected; redemption remains pending");
        return;
    };
    let viewer_settings = match state.db.get_viewer_settings(&event.user_id).await {
        Ok(settings) => settings,
        Err(e) => { error!(error = %e, %redemption_id, "Cannot read viewer auto-buy preference"); return; }
    };
    let mode = if !reward_data.market_autobuy { "OPERATOR" }
        else if viewer_settings.auto_buy_enabled { "AUTO" } else { "VIEWER" };
    let created = match state.db.create_inventory_item(redemption_id, &item_name, fixed_price as i64,
        mode, reward_data.retry_on_buyer_failure).await {
        Ok(created) => created,
        Err(e) => {
            error!(error = %e, %redemption_id, "Cannot persist inventory item");
            return;
        }
    };
    let actual_mode = state.db.get_inventory_core(redemption_id).await.ok().flatten().map(|item| item.4);
    if created {
        let template = match actual_mode.as_deref() {
            Some("VIEWER") => Some(MSG_ORDERS_WAITING_VIEWER),
            Some("OPERATOR") => Some(MSG_ORDERS_WAITING_OPERATOR),
            _ => None,
        };
        if let Some(template) = template {
            crate::processor::inventory_fulfillment::send_inventory_chat(
                &state, &reward_data.streamer_id, template, &event.user_login, &item_name, &[],
            ).await;
        }
    }
    if actual_mode.as_deref() == Some("AUTO") {
        if let Err(e) = crate::processor::inventory_fulfillment::purchase(&state, redemption_id, false, false, None).await {
            error!(error = %e, %redemption_id, "Initial Market attempt could not complete locally");
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

pub(crate) async fn check_and_pause_if_global_limit_reached(
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

}
