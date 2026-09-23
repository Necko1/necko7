use std::sync::Arc;
use tracing::{error, warn};
use uuid::Uuid;

use crate::processor::order_watcher::{OrderWatcher, WatcherRedemptionData};
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
        state.db.require_inventory_trade_link(redemption_id).await.map_err(|e| e.to_string())?;
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
            spawn_watcher(state, redemption_id, redemption.twitch_reward_id, &reward.streamer_id, &redemption.user_login, &setting.market_api_key, custom_id);
            Ok("ORDER_CREATED")
        }
        Ok(response) => {
            let detail = response.error.unwrap_or_else(|| "Market rejected purchase".to_string());
            let kind = classify_market_buy_for_error(response.code.unwrap_or(0), &detail);
            if !is_definitive_rejection(kind, response.id.as_deref()) {
                state.db.mark_attempt_ambiguous(redemption_id, &custom_id, &detail).await.map_err(|e| e.to_string())?;
                return Ok("RECONCILIATION_REQUIRED");
            }
            let label = match kind {
                MarketBuyForErrorKind::NotEnoughFunds => "no_money",
                MarketBuyForErrorKind::PriceOrChanceDeviation => "item_unavailable",
                k if k.is_buyer_terminal_error() => "trade_link",
                _ => "market_rejected",
            };
            state.db.mark_attempt_rejected(redemption_id, &custom_id, label, &detail).await.map_err(|e| e.to_string())?;
            Ok(match label { "no_money" => "INSUFFICIENT_FUNDS", "trade_link" => "TRADE_LINK_REQUIRED", _ => "RETRY_AVAILABLE" })
        }
        Err(e) => {
            warn!(error = %e, %redemption_id, "Market buy-for outcome is unknown; no automatic retry");
            state.db.mark_attempt_ambiguous(redemption_id, &custom_id, &e.to_string()).await.map_err(|e| e.to_string())?;
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
        state.db.mark_inventory_delivered(redemption_id, custom_id).await.map_err(|e| e.to_string())?;
        if let Err(e) = fulfill_delivered_twitch(state, redemption_id).await {
            error!(error = %e, %redemption_id, "Twitch fulfillment remains pending for recovery");
        }
        return Ok(false);
    }
    if data.stage == "5" {
        let buyer = data.causer.as_deref() == Some("buyer");
        state.db.attach_inventory_order(redemption_id, custom_id, Some(&data.item_id), &data.market_hash_name).await.map_err(|e| e.to_string())?;
        state.db.set_terminal_trade_failure(redemption_id, custom_id, buyer, data.causer.as_deref(), data.cancellation_reason.as_deref()).await.map_err(|e| e.to_string())?;
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
        spawn_watcher(state, redemption_id, redemption.twitch_reward_id, &reward.streamer_id, &redemption.user_login, &setting.market_api_key, custom_id.to_string());
    }
    if data.has_active_trade() {
        state.db.set_trade_waiting(redemption_id, custom_id, data.trade_id.as_deref(), data.send_until, data.receive_until).await.map_err(|e| e.to_string())?;
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
    match result { Ok(_) => Ok("REFUNDED"), Err(e) => Err(format!("Twitch refund result must be reconciled: {e}")) }
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
    use super::{is_definitive_rejection, resolve_trade_link};
    use crate::steam::market::errors::MarketBuyForErrorKind;

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
