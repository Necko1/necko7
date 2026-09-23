use std::sync::Arc;
use tracing::{error, warn};
use uuid::Uuid;

use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
use crate::messages::{
    MSG_MARKET_ERR_UNKNOWN,
    MSG_ORDERS_CREATED, MSG_ORDERS_INSUFFICIENT_FUNDS, MSG_ORDERS_RECONCILIATION_REQUIRED,
    MSG_ORDERS_TRADE_LINK_REQUIRED, MSG_ORDERS_UNAVAILABLE,
    MSG_ORDERS_REFUNDED, MSG_TRADES_ACCEPTED, MSG_TRADES_CREATED,
    MSG_TRADES_FAILED_BUYER, MSG_TRADES_FAILED_SELLER,
};
use crate::state::AppState;
use crate::steam::market::errors::{classify_market_buy_for_error, MarketBuyForErrorKind};
use crate::steam::trade_link::TradeLink;

fn resolve_trade_link(message: &str, saved: Option<&str>, use_saved_link: bool) -> Option<TradeLink> {
    if use_saved_link {
        saved.and_then(TradeLink::parse)
    } else {
        TradeLink::parse(message).or_else(|| saved.and_then(TradeLink::parse))
    }
}

fn is_definitive_rejection(kind: MarketBuyForErrorKind, order_id: Option<&str>) -> bool {
    order_id.is_none() && !matches!(kind, MarketBuyForErrorKind::Unknown | MarketBuyForErrorKind::Other)
}

fn rejected_order_template(kind: MarketBuyForErrorKind) -> Option<&'static str> {
    match kind {
        MarketBuyForErrorKind::NotEnoughFunds => Some(MSG_ORDERS_INSUFFICIENT_FUNDS),
        MarketBuyForErrorKind::PriceOrChanceDeviation => Some(MSG_ORDERS_UNAVAILABLE),
        MarketBuyForErrorKind::Unknown | MarketBuyForErrorKind::Other => None,
        _ => kind.to_market_error_message_key(),
    }
}

fn format_inventory_price(amount: i64, currency: &str) -> String {
    let major = crate::steam::market::minor_to_major(amount, currency);
    let code = currency.to_ascii_uppercase();
    if matches!(code.as_str(), "USD" | "EUR") {
        format!("{major:.3} {code}")
    } else {
        format!("{major:.2} {code}")
    }
}

/// Chat is informational. A Twitch chat failure must not undo a persisted
/// inventory or Market transition, and callers only invoke this on a new event.
pub async fn send_inventory_chat(
    state: &Arc<AppState>, channel_id: &str, template: &str, buyer: &str, item: &str,
    extra: &[(&str, &str)],
) {
    let mut vars = vec![("buyer", buyer), ("item", item)];
    vars.extend_from_slice(extra);
    let message = state.render_chat_message(channel_id, template, &vars);
    if let Err(e) = state.send_chat_message(channel_id, &message, None).await {
        warn!(error = %e, %channel_id, %template, "Could not send inventory status to channel chat");
    }
}

pub fn terminal_trade_template(buyer_fault: bool) -> &'static str {
    if buyer_fault { MSG_TRADES_FAILED_BUYER } else { MSG_TRADES_FAILED_SELLER }
}

pub async fn announce_trade(
    state: &Arc<AppState>, channel_id: &str, buyer: &str, item: &str,
    trade_id: &str, receive_until: Option<chrono::DateTime<chrono::Utc>>,
) {
    use crate::datetime::DateTimeExt;
    let tradeoffer = format!("https://steamcommunity.com/tradeoffer/{trade_id}/");
    let remaining = receive_until.map(|at| at.remaining_pretty()).unwrap_or_else(|| "a limited time".to_string());
    send_inventory_chat(state, channel_id, MSG_TRADES_CREATED, buyer, item,
        &[("tradeoffer", &tradeoffer), ("remaining", &remaining)]).await;
}

/// A new order is only made by initial auto-buy or an explicit authorized action.
/// The durable CALLING row is deliberately treated as ambiguous after a crash.
pub async fn purchase(state: &Arc<AppState>, redemption_id: Uuid, viewer_action: bool, use_saved_link: bool) -> Result<&'static str, String> {
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?
        .ok_or("Redemption not found")?;
    let inventory = state.db.get_inventory_core(redemption_id).await.map_err(|e| e.to_string())?
        .ok_or("Inventory item not found")?;
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?
        .ok_or("Reward not found")?;
    let setting = state.db.get_broadcaster_setting(&reward.streamer_id).await.map_err(|e| e.to_string())?
        .ok_or("Channel settings not found")?;
    if inventory.4 == "LEGACY_REVIEW" { return Ok("RECONCILIATION_REQUIRED"); }

    if let Some(previous) = state.db.latest_inventory_attempt(redemption_id).await.map_err(|e| e.to_string())? {
        if previous.status != "REJECTED" {
            if !reconcile(state, redemption_id, &previous.custom_id).await? { return Ok("RECONCILIATION_REQUIRED"); }
        }
    }

    let viewer_settings = state.db.get_viewer_settings(&redemption.user_id).await.map_err(|e| e.to_string())?;
    let parsed = resolve_trade_link(&redemption.user_trade_link, viewer_settings.trade_link.as_deref(), use_saved_link);
    let Some(trade_link) = parsed else {
        if state.db.require_inventory_trade_link(redemption_id).await.map_err(|e| e.to_string())? {
            send_inventory_chat(state, &reward.streamer_id, MSG_ORDERS_TRADE_LINK_REQUIRED,
                &redemption.user_login, &inventory.1, &[]).await;
        }
        return Ok("TRADE_LINK_REQUIRED");
    };
    let trade_link_text = format!("https://steamcommunity.com/tradeoffer/new/?partner={}&token={}", trade_link.partner, trade_link.token);
    let max_price = i32::try_from(inventory.2).map_err(|_| "Inventory fixed price exceeds Market request range")?;
    let Some(custom_id) = state.db.begin_inventory_attempt(redemption_id, &trade_link_text, viewer_action).await.map_err(|e| e.to_string())? else {
        return Ok("BLOCKED");
    };

    let market_result = state.market_client.buy_for(
        &setting.market_api_key, &inventory.1, max_price, setting.market_chance_to_transfer,
        trade_link, &custom_id,
    ).await;
    match market_result {
        Ok(response) if response.success => {
            if let Err(e) = state.db.attach_inventory_order(redemption_id, &custom_id, response.id.as_deref(), &inventory.1).await {
                error!(error = %e, %redemption_id, "Market order may exist but local attachment failed");
                return Err(e.to_string());
            }
            let retry_count = custom_id.strip_prefix(&format!("{redemption_id}-"))
                .and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
            state.db.set_redemption_order_created(redemption_id, response.price.unwrap_or(inventory.2), Some(&inventory.1), retry_count).await.map_err(|e| e.to_string())?;
            crate::processor::redemption::check_and_pause_if_global_limit_reached(state, &reward.streamer_id, redemption.twitch_reward_id).await;
            send_inventory_chat(state, &reward.streamer_id, MSG_ORDERS_CREATED,
                &redemption.user_login, &inventory.1, &[]).await;
            spawn_watcher(state, redemption_id, redemption.twitch_reward_id, &reward.streamer_id, &redemption.user_login, &setting.market_api_key, custom_id);
            Ok("ORDER_CREATED")
        }
        Ok(response) => {
            let detail = response.error.unwrap_or_else(|| "Market rejected purchase".to_string());
            let kind = classify_market_buy_for_error(response.code.unwrap_or(0), &detail);
            if !is_definitive_rejection(kind, response.id.as_deref()) {
                state.db.mark_attempt_ambiguous(redemption_id, &custom_id, &detail).await.map_err(|e| e.to_string())?;
                let template = if response.id.is_none() && kind == MarketBuyForErrorKind::Unknown {
                    MSG_MARKET_ERR_UNKNOWN
                } else {
                    MSG_ORDERS_RECONCILIATION_REQUIRED
                };
                send_inventory_chat(state, &reward.streamer_id, template,
                    &redemption.user_login, &inventory.1, &[]).await;
                return Ok("RECONCILIATION_REQUIRED");
            }
            let label = match kind {
                MarketBuyForErrorKind::NotEnoughFunds => "no_money",
                MarketBuyForErrorKind::PriceOrChanceDeviation => "item_unavailable",
                k if k.is_buyer_terminal_error() => "trade_link",
                _ => "market_rejected",
            };
            let changed = state.db.mark_attempt_rejected(redemption_id, &custom_id, label, &detail).await.map_err(|e| e.to_string())?;
            if changed {
                if let Some(template) = rejected_order_template(kind) {
                    let price = format_inventory_price(inventory.2, &inventory.3);
                    send_inventory_chat(state, &reward.streamer_id, template,
                        &redemption.user_login, &inventory.1, &[("price", &price)]).await;
                } else {
                    warn!(?kind, %redemption_id, "No chat template for definitive Market rejection");
                }
            }
            Ok(match label { "no_money" => "INSUFFICIENT_FUNDS", "trade_link" => "TRADE_LINK_REQUIRED", _ => "RETRY_AVAILABLE" })
        }
        Err(e) => {
            warn!(error = %e, %redemption_id, "Market buy-for outcome is unknown; no automatic retry");
            state.db.mark_attempt_ambiguous(redemption_id, &custom_id, &e.to_string()).await.map_err(|e| e.to_string())?;
            send_inventory_chat(state, &reward.streamer_id, MSG_ORDERS_RECONCILIATION_REQUIRED,
                &redemption.user_login, &inventory.1, &[]).await;
            Ok("RECONCILIATION_REQUIRED")
        }
    }
}

/// Returns true only when the previous attempt is confirmed terminal and cannot
/// still deliver. Missing/unsuccessful lookup is never proof of non-creation.
pub async fn reconcile(state: &Arc<AppState>, redemption_id: Uuid, custom_id: &str) -> Result<bool, String> {
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Redemption not found")?;
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?.ok_or("Reward not found")?;
    let setting = state.db.get_broadcaster_setting(&reward.streamer_id).await.map_err(|e| e.to_string())?.ok_or("Channel settings not found")?;
    let info = match state.market_client.get_buy_info(&setting.market_api_key, custom_id).await {
        Ok(info) if info.success => info,
        _ => return Ok(false),
    };
    let Some(data) = info.data else { return Ok(false); };
    let inventory = state.db.get_inventory_core(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Inventory item not found")?;
    if data.market_hash_name != inventory.1 {
        state.db.require_inventory_reconciliation(redemption_id, custom_id).await.map_err(|e| e.to_string())?;
        return Ok(false);
    }
    if data.is_claimed() {
        state.db.attach_inventory_order(redemption_id, custom_id, Some(&data.item_id), &data.market_hash_name).await.map_err(|e| e.to_string())?;
        if state.db.mark_inventory_delivered(redemption_id, custom_id).await.map_err(|e| e.to_string())? {
            send_inventory_chat(state, &reward.streamer_id, MSG_TRADES_ACCEPTED,
                &redemption.user_login, &inventory.1, &[]).await;
        }
        if let Err(e) = fulfill_delivered_twitch(state, redemption_id).await {
            error!(error = %e, %redemption_id, "Twitch fulfillment remains pending for recovery");
        }
        return Ok(false);
    }
    if data.stage == "5" {
        let buyer = data.causer.as_deref() == Some("buyer");
        state.db.attach_inventory_order(redemption_id, custom_id, Some(&data.item_id), &data.market_hash_name).await.map_err(|e| e.to_string())?;
        if state.db.set_terminal_trade_failure(redemption_id, custom_id, buyer, data.causer.as_deref(), data.cancellation_reason.as_deref()).await.map_err(|e| e.to_string())? {
            send_inventory_chat(state, &reward.streamer_id, terminal_trade_template(buyer),
                &redemption.user_login, &inventory.1, &[]).await;
        }
        return Ok(true);
    }
    if data.causer.is_some() {
        state.db.require_inventory_reconciliation(redemption_id, custom_id).await.map_err(|e| e.to_string())?;
        return Ok(false);
    }
    state.db.attach_inventory_order(redemption_id, custom_id, Some(&data.item_id), &data.market_hash_name).await.map_err(|e| e.to_string())?;
    if redemption.status == crate::db::redemptions::RedemptionStatus::Pending {
        let retry_count = custom_id.strip_prefix(&format!("{redemption_id}-")).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
        state.db.set_redemption_order_created(redemption_id, crate::steam::market::major_to_minor(data.paid, &inventory.3), Some(&inventory.1), retry_count).await.map_err(|e| e.to_string())?;
        send_inventory_chat(state, &reward.streamer_id, MSG_ORDERS_CREATED,
            &redemption.user_login, &inventory.1, &[]).await;
        spawn_watcher(state, redemption_id, redemption.twitch_reward_id, &reward.streamer_id, &redemption.user_login, &setting.market_api_key, custom_id.to_string());
    }
    if data.has_active_trade() {
        if state.db.set_trade_waiting(redemption_id, custom_id, data.trade_id.as_deref(), data.send_until, data.receive_until).await.map_err(|e| e.to_string())? {
            if let Some(trade_id) = data.trade_id.as_deref() {
                announce_trade(state, &reward.streamer_id, &redemption.user_login, &inventory.1,
                    trade_id, data.receive_until).await;
            }
        }
    }
    Ok(false)
}

/// Retry only the Twitch status update for an already delivered item. This never
/// creates another Market order or changes the inventory economic snapshot.
pub async fn fulfill_delivered_twitch(state: &Arc<AppState>, redemption_id: Uuid) -> Result<(), String> {
    if !state.db.inventory_twitch_fulfillment_pending(redemption_id).await.map_err(|e| e.to_string())? { return Ok(()); }
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?
        .ok_or("Redemption not found")?;
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?
        .ok_or("Reward not found")?;
    state.with_broadcaster_token(&reward.streamer_id, async |token| {
        state.helix_client.update_redemption_status(&reward.streamer_id,
            &redemption.twitch_reward_id.to_string(), &redemption_id.to_string(), false, &token).await
    }).await.map_err(|e| e.to_string())?;
    state.db.mark_inventory_twitch_fulfilled(redemption_id).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn refund(state: &Arc<AppState>, redemption_id: Uuid) -> Result<&'static str, String> {
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Redemption not found")?;
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?.ok_or("Reward not found")?;
    let inventory = state.db.get_inventory_core(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Inventory item not found")?;
    if let Some(latest) = state.db.latest_inventory_attempt(redemption_id).await.map_err(|e| e.to_string())? {
        if latest.status != "REJECTED" && !reconcile(state, redemption_id, &latest.custom_id).await? {
            return Ok("RECONCILIATION_REQUIRED");
        }
    }
    if !state.db.reserve_inventory_refund(redemption_id).await.map_err(|e| e.to_string())? {
        return Ok("BLOCKED");
    }
    let result = state.with_broadcaster_token(&reward.streamer_id, async |token| {
        state.helix_client.update_redemption_status(&reward.streamer_id,
            &redemption.twitch_reward_id.to_string(), &redemption_id.to_string(), true, &token).await
    }).await;
    let succeeded = result.is_ok();
    state.db.finish_inventory_refund(redemption_id, succeeded).await.map_err(|e| e.to_string())?;
    match result {
        Ok(_) => {
            send_inventory_chat(state, &reward.streamer_id, MSG_ORDERS_REFUNDED,
                &redemption.user_login, &inventory.1, &[]).await;
            Ok("REFUNDED")
        }
        Err(e) => Err(format!("Twitch refund result must be reconciled: {e}")),
    }
}

pub fn spawn_watcher(state: &Arc<AppState>, redemption_id: Uuid, reward_id: Uuid, channel_id: &str, user_login: &str, api_key: &str, custom_id: String) {
    let watcher = OrderWatcher::new(state.clone(), api_key.to_string(), channel_id.to_string(), WatcherRedemptionData {
        redemption_id, custom_id, reward_id, user_login: user_login.to_string(),
    });
    let token = state.shutdown_token.clone();
    state.spawn_task(async move { watcher.track_redemption(token).await; });
}

#[cfg(test)]
mod tests {
    use super::{format_inventory_price, is_definitive_rejection, rejected_order_template, resolve_trade_link, terminal_trade_template};
    use crate::messages::{
        MSG_ORDERS_INSUFFICIENT_FUNDS,
        MSG_ORDERS_UNAVAILABLE, MSG_MARKET_ERR_BOT_BANNED, MSG_MARKET_ERR_INVENTORY_HIDDEN,
        MSG_MARKET_ERR_TRADE_LINK_INVALID, MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED,
        MSG_MARKET_ERR_STEAM_BANNED, MSG_MARKET_ERR_NO_MOBILE_AUTH,
        MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED, MSG_MARKET_ERR_INVENTORY_FULL,
        MSG_TRADES_FAILED_BUYER, MSG_TRADES_FAILED_SELLER,
    };
    use crate::steam::market::errors::MarketBuyForErrorKind;

    #[test]
    fn chat_templates_match_persisted_fulfillment_outcomes() {
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::NotEnoughFunds), Some(MSG_ORDERS_INSUFFICIENT_FUNDS));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::PriceOrChanceDeviation), Some(MSG_ORDERS_UNAVAILABLE));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::InvalidTradeLink), Some(MSG_MARKET_ERR_TRADE_LINK_INVALID));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::InventoryHidden), Some(MSG_MARKET_ERR_INVENTORY_HIDDEN));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::CheckBotBanned), Some(MSG_MARKET_ERR_BOT_BANNED));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::TradeLinkCheckFailed), Some(MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::SteamBanned), Some(MSG_MARKET_ERR_STEAM_BANNED));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::NoMobileAuth), Some(MSG_MARKET_ERR_NO_MOBILE_AUTH));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::OfflineTradesDisabled), Some(MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::InventoryFull), Some(MSG_MARKET_ERR_INVENTORY_FULL));
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::Unknown), None);
        assert_eq!(rejected_order_template(MarketBuyForErrorKind::Other), None);
        assert_eq!(terminal_trade_template(true), MSG_TRADES_FAILED_BUYER);
        assert_eq!(terminal_trade_template(false), MSG_TRADES_FAILED_SELLER);
    }

    #[test]
    fn unavailable_message_uses_full_fixed_price_in_major_units() {
        use crate::messages::{render_template, CategorizedChatMessages};
        let template = CategorizedChatMessages::default().orders.unavailable;
        for (minor, currency, expected) in [
            (2750, "RUB", "27.50 RUB"),
            (2750, "USD", "2.750 USD"),
            (2750, "EUR", "2.750 EUR"),
        ] {
            let price = format_inventory_price(minor, currency);
            assert_eq!(price, expected);
            assert!(render_template(&template, &[("buyer", "viewer"), ("item", "AK-47"), ("price", &price)]).contains(expected));
        }
    }

    #[test]
    fn message_link_has_priority_and_saved_link_requires_explicit_override() {
        let message = "Please send https://steamcommunity.com/tradeoffer/new/?partner=11&token=abc";
        let saved = "https://steamcommunity.com/tradeoffer/new/?partner=22&token=xyz";
        assert_eq!(resolve_trade_link(message, Some(saved), false).unwrap().partner, "11");
        assert_eq!(resolve_trade_link(message, Some(saved), true).unwrap().partner, "22");
        assert_eq!(resolve_trade_link("no link", Some(saved), false).unwrap().partner, "22");
        assert!(resolve_trade_link("no link", None, false).is_none());
    }

    #[test]
    fn only_known_orderless_market_rejections_can_release_an_attempt() {
        assert!(is_definitive_rejection(MarketBuyForErrorKind::NotEnoughFunds, None));
        assert!(is_definitive_rejection(MarketBuyForErrorKind::PriceOrChanceDeviation, None));
        assert!(is_definitive_rejection(MarketBuyForErrorKind::InvalidTradeLink, None));
        assert!(!is_definitive_rejection(MarketBuyForErrorKind::Unknown, None));
        assert!(!is_definitive_rejection(MarketBuyForErrorKind::Other, None));
        assert!(!is_definitive_rejection(MarketBuyForErrorKind::NotEnoughFunds, Some("possibly-created")));
    }
}
