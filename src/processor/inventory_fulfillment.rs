use std::sync::Arc;
use tracing::{error, warn};
use uuid::Uuid;

use crate::messages::{
    MSG_MARKET_ERR_UNKNOWN,
    MSG_ORDERS_CREATED, MSG_ORDERS_INSUFFICIENT_FUNDS, MSG_ORDERS_RECONCILIATION_REQUIRED,
    MSG_ORDERS_TRADE_LINK_REQUIRED, MSG_ORDERS_UNAVAILABLE,
    MSG_ORDERS_REFUNDED, MSG_TRADES_ACCEPTED, MSG_TRADES_CREATED,
    MSG_TRADES_FAILED_BUYER, MSG_TRADES_FAILED_SELLER,
    MSG_TRADES_REVERTED_BUYER, MSG_TRADES_REVERTED_SELLER,
};
use crate::state::AppState;
use crate::steam::market::errors::{classify_market_buy_for_error, MarketBuyForErrorKind};
use crate::steam::trade_link::TradeLink;

/// All Market effects use an inventory identity. Only the source adapter may
/// interact with Twitch; a manual context has no reward, redemption or viewer.
pub(crate) struct DeliveryContext {
    pub inventory: (Uuid, String, i64, String, String, bool),
    pub channel_id: String,
    pub api_key: String,
    pub chance_to_transfer: i16,
    pub redemption: Option<crate::db::redemptions::Redemption>,
}

impl DeliveryContext {
    pub async fn load(state: &Arc<AppState>, inventory_id: Uuid) -> Result<Self, String> {
        let inventory = state.db.get_delivery_core(inventory_id).await.map_err(|e| e.to_string())?.ok_or("Inventory item not found")?;
        let (channel_id, redemption_id, _) = state.db.delivery_source(inventory_id).await.map_err(|e| e.to_string())?;
        let settings = state.db.get_broadcaster_setting(&channel_id).await.map_err(|e| e.to_string())?.ok_or("Channel settings not found")?;
        let redemption = match redemption_id {
            Some(id) => Some(state.db.get_redemption(id).await.map_err(|e| e.to_string())?.ok_or("Fulfillment source not found")?),
            None => None,
        };
        Ok(Self { inventory, channel_id, api_key: settings.market_api_key,
            chance_to_transfer: settings.market_chance_to_transfer, redemption })
    }
}

async fn send_delivery_notice(state: &Arc<AppState>, context: &DeliveryContext, template: &str, item: &str, extra: &[(&str, &str)]) {
    if let Some(redemption) = context.redemption.as_ref() {
        send_fulfillment_chat(state, &redemption.origin, Some(redemption.fulfillment_id), &context.channel_id,
            template, &redemption.user_login, item, extra).await;
    }
}

async fn send_delivery_attempt_notice(state: &Arc<AppState>, context: &DeliveryContext, custom: &str, template: &str, item: &str, extra: &[(&str, &str)]) {
    if let Some(redemption) = context.redemption.as_ref() {
        send_attempt_chat(state, redemption, custom, &context.channel_id, template, item, extra).await;
    }
}

fn safe_market_detail(detail: &str, api_key: &str, token: &str) -> String {
    let mut safe = detail.to_owned();
    for value in [api_key, token] {
        if !value.is_empty() { safe = safe.replace(value, "[redacted]"); }
    }
    safe.chars().take(2000).collect()
}

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

fn attempt_result(status: &str) -> &'static str {
    match status {
        "DELIVERED" => "DELIVERED",
        "SELLER_FAILED" | "BUYER_FAILED" => "RETRY_AVAILABLE",
        "TERMINAL_UNCLASSIFIED" => "OPERATOR_REVIEW",
        "TRADE_WAITING" => "TRADE_WAITING",
        "TRADE_ACCEPTED" => "TRADE_ACCEPTED",
        "ORDER_CREATED" => "ORDER_CREATED",
        "REJECTED" => "RETRY_AVAILABLE",
        _ => "RECONCILIATION_REQUIRED",
    }
}

/// Chat is informational. A Twitch chat failure must not undo a persisted
/// inventory or Market transition, and callers only invoke this on a new event.
pub async fn send_inventory_chat(
    state: &Arc<AppState>, channel_id: &str, template: &str, buyer: &str, item: &str,
    extra: &[(&str, &str)],
) {
    send_fulfillment_chat(state, "TWITCH", None, channel_id, template, buyer, item, extra).await;
}

pub(crate) async fn should_send_fulfillment_chat(
    state: &Arc<AppState>, origin: &str, redemption_id: Option<Uuid>, template: &str,
) -> bool {
    if !crate::messages::inventory_chat_allowed(origin, template) { return false; }
    if origin == "SCRIPT" && crate::messages::script_suppressible_chat_key(template).is_some() {
        let Some(redemption_id) = redemption_id else {
            warn!(%template, "Script fulfillment notice has no fulfillment ID");
            return true;
        };
        let suppressed: Option<bool> = match sqlx::query_scalar(
            "SELECT $2 = ANY(script_suppressed_chat_keys) FROM redemptions WHERE fulfillment_id=$1 AND origin='SCRIPT'"
        ).bind(redemption_id).bind(template).fetch_optional(state.db.pool()).await {
            Ok(value) => value,
            Err(error) => {
                warn!(%error, %redemption_id, %template, "Could not check script fulfillment chat preference");
                return true;
            }
        };
        match suppressed {
            Some(true) => {
                tracing::debug!(%redemption_id, %template, "Script suppressed a fulfillment chat notice");
                return false;
            }
            Some(false) => {}
            None => {
                warn!(%redemption_id, %template, "Script fulfillment missing before chat notice");
                return true;
            }
        }
    }
    true
}

async fn send_fulfillment_chat(
    state: &Arc<AppState>, origin: &str, redemption_id: Option<Uuid>, channel_id: &str, template: &str, buyer: &str, item: &str,
    extra: &[(&str, &str)],
) {
    if !should_send_fulfillment_chat(state, origin, redemption_id, template).await { return; }
    let mut vars = vec![("buyer", buyer), ("item", item)];
    vars.extend_from_slice(extra);
    let message = state.render_chat_message(channel_id, template, &vars);
    if let Err(e) = state.send_chat_message(channel_id, &message, None).await {
        warn!(error = %e, %channel_id, %template, "Could not send inventory status to channel chat");
    }
}

async fn send_attempt_chat(state: &Arc<AppState>, redemption: &crate::db::redemptions::Redemption,
    custom_id: &str, channel_id: &str, template: &str, item: &str, extra: &[(&str, &str)]) {
    match state.db.claim_inventory_attempt_chat(custom_id, template).await {
        Ok(true) => send_fulfillment_chat(state, &redemption.origin, Some(redemption.fulfillment_id), channel_id, template,
            &redemption.user_login, item, extra).await,
        Ok(false) => {},
        Err(e) => warn!(error = %e, %custom_id, %template, "Cannot claim attempt chat event"),
    }
}

pub fn terminal_trade_template(kind: &str) -> Option<&'static str> {
    match kind {
        "buyer_not_accepted" => Some(MSG_TRADES_FAILED_BUYER),
        "seller_not_sent" | "seller_cancelled" => Some(MSG_TRADES_FAILED_SELLER),
        "buyer_reverted" => Some(MSG_TRADES_REVERTED_BUYER),
        "seller_reverted" => Some(MSG_TRADES_REVERTED_SELLER),
        _ => None,
    }
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
pub async fn purchase(state: &Arc<AppState>, redemption_id: Uuid, viewer_action: bool, use_saved_link: bool,
    actor_user_id: Option<&str>) -> Result<&'static str, String> {
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
        if matches!(previous.status.as_str(), "CALLING" | "ORDER_CREATED" | "TRADE_WAITING" | "TRADE_ACCEPTED" | "RECONCILIATION_REQUIRED") {
            if !reconcile(state, redemption_id, &previous.custom_id).await? { return Ok("RECONCILIATION_REQUIRED"); }
        }
    }

    let viewer_settings = state.db.get_viewer_settings(&redemption.user_id).await.map_err(|e| e.to_string())?;
    let parsed = resolve_trade_link(&redemption.user_trade_link, viewer_settings.trade_link.as_deref(), use_saved_link);
    let Some(trade_link) = parsed else {
        if state.db.require_inventory_trade_link(redemption_id).await.map_err(|e| e.to_string())? {
            send_fulfillment_chat(state, &redemption.origin, Some(redemption_id), &reward.streamer_id, MSG_ORDERS_TRADE_LINK_REQUIRED,
                &redemption.user_login, &inventory.1, &[]).await;
        }
        return Ok("TRADE_LINK_REQUIRED");
    };
    let trade_link_text = format!("https://steamcommunity.com/tradeoffer/new/?partner={}&token={}", trade_link.partner, trade_link.token);
    let initiator = match (viewer_action, actor_user_id) {
        (true, Some(_)) => "viewer",
        (false, Some(_)) => "operator",
        _ => "system",
    };
    let Some(custom_id) = state.db.begin_inventory_attempt_with_chance(redemption_id, &trade_link_text, viewer_action,
        initiator, actor_user_id, setting.market_chance_to_transfer).await.map_err(|e| e.to_string())? else {
        return Ok("BLOCKED");
    };

    let context = DeliveryContext { inventory, channel_id: reward.streamer_id,
        api_key: setting.market_api_key, chance_to_transfer: setting.market_chance_to_transfer,
        redemption: Some(redemption) };
    submit_delivery_attempt(state, context, custom_id, trade_link).await
}

pub(crate) async fn submit_delivery_attempt(state: &Arc<AppState>, context: DeliveryContext,
    custom_id: String, trade_link: TradeLink) -> Result<&'static str, String> {
    let inventory = &context.inventory;
    let inventory_id = inventory.0;
    let max_price = i32::try_from(inventory.2).map_err(|_| "Price exceeds Market request range")?;
    let market_result = state.market_client.buy_for(
        &context.api_key, &inventory.1, max_price, context.chance_to_transfer,
        trade_link.clone(), &custom_id,
    ).await;
    match market_result {
        Ok(response) if response.success => {
            if let Err(e) = state.db.attach_delivery_order(inventory_id, &custom_id, response.id.as_deref(), &inventory.1).await {
                error!(error = %e, %inventory_id, "Market order may exist but local attachment failed");
                return Err(e.to_string());
            }
            if let Some(redemption) = context.redemption.as_ref() {
                let id = redemption.fulfillment_id;
                let retry_count = custom_id.strip_prefix(&format!("{id}-")).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
                state.db.set_redemption_order_created(id, response.price.unwrap_or(inventory.2), Some(&inventory.1), retry_count).await.map_err(|e| e.to_string())?;
                crate::processor::redemption::check_and_pause_if_global_limit_reached(state, &context.channel_id, redemption.twitch_reward_id).await;
            }
            sqlx::query("UPDATE inventory_order_attempts SET paid_price=COALESCE(paid_price,$2) WHERE custom_id=$1")
                .bind(&custom_id).bind(response.price).execute(state.db.pool()).await.map_err(|e| e.to_string())?;
            let current = state.db.latest_delivery_attempt(inventory_id).await.map_err(|e| e.to_string())?
                .ok_or("Market attempt disappeared after order creation")?;
            if current.custom_id != custom_id { return Ok("RECONCILIATION_REQUIRED"); }
            if matches!(current.status.as_str(), "ORDER_CREATED" | "TRADE_WAITING" | "TRADE_ACCEPTED") {
                send_delivery_attempt_notice(state, &context, &custom_id, MSG_ORDERS_CREATED,
                    &inventory.1, &[]).await;
            }
            Ok(attempt_result(&current.status))
        }
        Ok(response) => {
            let raw_detail = response.error.unwrap_or_else(|| "Market rejected purchase".to_string());
            let detail = safe_market_detail(&raw_detail, &context.api_key, &trade_link.token);
            let kind = classify_market_buy_for_error(response.code.unwrap_or(0), &raw_detail);
            if !is_definitive_rejection(kind, response.id.as_deref()) {
                state.db.mark_delivery_ambiguous(inventory_id, &custom_id, &detail).await.map_err(|e| e.to_string())?;
                let current = state.db.latest_delivery_attempt(inventory_id).await.map_err(|e| e.to_string())?;
                if let Some(ref current) = current {
                    if current.custom_id == custom_id && current.status != "RECONCILIATION_REQUIRED" {
                        return Ok(attempt_result(&current.status));
                    }
                }
                let template = if response.id.is_none() && kind == MarketBuyForErrorKind::Unknown {
                    MSG_MARKET_ERR_UNKNOWN
                } else {
                    MSG_ORDERS_RECONCILIATION_REQUIRED
                };
                send_delivery_notice(state, &context, template,
                    &inventory.1, &[]).await;
                return Ok("RECONCILIATION_REQUIRED");
            }
            let label = match kind {
                MarketBuyForErrorKind::NotEnoughFunds => "no_money",
                MarketBuyForErrorKind::PriceOrChanceDeviation => "item_unavailable",
                k if k.is_buyer_terminal_error() => "trade_link",
                _ => "market_rejected",
            };
            let changed = state.db.mark_delivery_rejected(inventory_id, &custom_id, label, &detail).await.map_err(|e| e.to_string())?;
            if !changed {
                if let Some(current) = state.db.latest_delivery_attempt(inventory_id).await.map_err(|e| e.to_string())? {
                    if current.status == "REJECTED" {
                        return Ok(match current.outcome_kind.as_deref() {
                            Some("no_money") => "INSUFFICIENT_FUNDS",
                            Some("trade_link") => "TRADE_LINK_REQUIRED",
                            _ => "RETRY_AVAILABLE",
                        });
                    }
                    return Ok(attempt_result(&current.status));
                }
                return Ok("RECONCILIATION_REQUIRED");
            }
            if changed {
                if let Some(template) = rejected_order_template(kind) {
                    let price = format_inventory_price(inventory.2, &inventory.3);
                    send_delivery_notice(state, &context, template,
                        &inventory.1, &[("price", &price)]).await;
                } else {
                    warn!(?kind, %inventory_id, "No chat template for definitive Market rejection");
                }
            }
            Ok(match label { "no_money" => "INSUFFICIENT_FUNDS", "trade_link" => "TRADE_LINK_REQUIRED", _ => "RETRY_AVAILABLE" })
        }
        Err(_e) => {
            warn!(%inventory_id, "Market buy-for outcome is unknown; no automatic retry");
            state.db.mark_delivery_ambiguous(inventory_id, &custom_id, "Market transport or response error; result is unknown").await.map_err(|e| e.to_string())?;
            if let Some(current) = state.db.latest_delivery_attempt(inventory_id).await.map_err(|e| e.to_string())? {
                if current.custom_id == custom_id && current.status != "RECONCILIATION_REQUIRED" {
                    return Ok(attempt_result(&current.status));
                }
            }
            send_delivery_notice(state, &context, MSG_ORDERS_RECONCILIATION_REQUIRED,
                &inventory.1, &[]).await;
            Ok("RECONCILIATION_REQUIRED")
        }
    }
}

/// Returns true only when the previous attempt is confirmed terminal and cannot
/// still deliver. Missing/unsuccessful lookup is never proof of non-creation.
pub async fn reconcile(state: &Arc<AppState>, redemption_id: Uuid, custom_id: &str) -> Result<bool, String> {
    let core = state.db.get_inventory_core(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Inventory item not found")?;
    reconcile_delivery(state, core.0, custom_id).await
}

pub async fn reconcile_delivery(state: &Arc<AppState>, inventory_id: Uuid, custom_id: &str) -> Result<bool, String> {
    let context = DeliveryContext::load(state, inventory_id).await?;
    let info = match state.market_client.get_buy_info(&context.api_key, custom_id).await {
        Ok(info) if info.success => info,
        _ => return Ok(false),
    };
    let Some(data) = info.data else { return Ok(false); };
    let inventory = &context.inventory;
    if data.market_hash_name != inventory.1 || (context.redemption.is_none() && (!data.currency.eq_ignore_ascii_case(&inventory.3) || !data.paid.is_finite() || data.paid < 0.0)) {
        state.db.require_delivery_reconciliation(inventory_id, custom_id).await.map_err(|e| e.to_string())?;
        return Ok(false);
    }
    if !matches!(data.stage.as_str(), "1" | "2" | "5") {
        state.db.require_delivery_reconciliation(inventory_id, custom_id).await.map_err(|e| e.to_string())?;
        return Ok(false);
    }
    let transition = state.db.observe_delivery_attempt(inventory_id, custom_id, &data).await.map_err(|e| e.to_string())?;
    if transition.chat_eligible && transition.order_created && data.stage == "1" {
        if let Some(redemption) = context.redemption.as_ref() {
            let id = redemption.fulfillment_id;
            let retry_count = custom_id.strip_prefix(&format!("{id}-")).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
            state.db.set_redemption_order_created(id, crate::steam::market::major_to_minor(data.paid, &inventory.3), Some(&inventory.1), retry_count).await.map_err(|e| e.to_string())?;
        }
        send_delivery_attempt_notice(state, &context, custom_id, MSG_ORDERS_CREATED,
            &inventory.1, &[]).await;
    }
    if transition.chat_eligible && transition.trade_created {
        if let Some(trade_id) = data.trade_id.as_deref() {
            use crate::datetime::DateTimeExt;
            let tradeoffer = format!("https://steamcommunity.com/tradeoffer/{trade_id}/");
            let remaining = data.receive_until.map(|at| at.remaining_pretty()).unwrap_or_else(|| "a limited time".to_string());
            send_delivery_attempt_notice(state, &context, custom_id, MSG_TRADES_CREATED,
                &inventory.1,
                &[("tradeoffer", &tradeoffer), ("remaining", &remaining)]).await;
        }
    }
    if transition.chat_eligible && transition.trade_accepted {
        send_delivery_attempt_notice(state, &context, custom_id, MSG_TRADES_ACCEPTED,
            &inventory.1, &[]).await;
    }
    if data.stage == "2" && let Some(redemption) = context.redemption.as_ref() {
        if let Err(e) = fulfill_delivered_twitch(state, redemption.fulfillment_id).await {
            error!(error = %e, %inventory_id, "Twitch fulfillment remains pending for recovery");
        }
    }
    if let Some(kind) = transition.terminal_kind.as_deref().filter(|_| transition.chat_eligible) {
        if let Some(template) = terminal_trade_template(kind) {
            send_delivery_attempt_notice(state, &context, custom_id, template,
                &inventory.1, &[]).await;
        }
    }
    Ok(data.stage == "5")
}

/// Retry only the Twitch status update for an already delivered item. This never
/// creates another Market order or changes the inventory economic snapshot.
pub async fn fulfill_delivered_twitch(state: &Arc<AppState>, redemption_id: Uuid) -> Result<(), String> {
    if !state.db.claim_inventory_twitch_fulfillment(redemption_id).await.map_err(|e| e.to_string())? { return Ok(()); }
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?
        .ok_or("Redemption not found")?;
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?
        .ok_or("Reward not found")?;
    if redemption.origin == "SCRIPT" { return Ok(()); }
    state.with_broadcaster_token(&reward.streamer_id, async |token| {
        state.helix_client.update_redemption_status(&reward.streamer_id,
            &redemption.twitch_reward_id.to_string(), &redemption_id.to_string(), false, &token).await
    }).await.map_err(|e| e.to_string())?;
    state.db.mark_inventory_twitch_fulfilled(redemption_id).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn refund(state: &Arc<AppState>, redemption_id: Uuid, viewer_action: bool,
    actor_user_id: Option<&str>) -> Result<&'static str, String> {
    let redemption = state.db.get_redemption(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Redemption not found")?;
    if redemption.origin == "SCRIPT" { return Ok("SCRIPT_ITEM_REQUIRES_DISCARD"); }
    let reward = state.db.get_reward_by_twitch_id(redemption.twitch_reward_id).await.map_err(|e| e.to_string())?.ok_or("Reward not found")?;
    let inventory = state.db.get_inventory_core(redemption_id).await.map_err(|e| e.to_string())?.ok_or("Inventory item not found")?;
    if let Some(latest) = state.db.latest_inventory_attempt(redemption_id).await.map_err(|e| e.to_string())? {
        if matches!(latest.status.as_str(), "CALLING" | "ORDER_CREATED" | "TRADE_WAITING" | "TRADE_ACCEPTED" | "RECONCILIATION_REQUIRED")
            && !reconcile(state, redemption_id, &latest.custom_id).await? {
            return Ok("RECONCILIATION_REQUIRED");
        }
    }
    if !state.db.reserve_inventory_refund_as(redemption_id, viewer_action, actor_user_id).await.map_err(|e| e.to_string())? {
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
        let accepted = &crate::messages::CategorizedChatMessages::default().trades.accepted;
        assert!(accepted.contains("still confirming"));
        assert!(!accepted.contains("Enjoy your skin"));
        let buyer_revert = &crate::messages::CategorizedChatMessages::default().trades.reverted_buyer;
        assert!(buyer_revert.contains("Contact the channel operator"));
        assert!(!buyer_revert.contains("available actions"));
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
        assert_eq!(terminal_trade_template("buyer_not_accepted"), Some(MSG_TRADES_FAILED_BUYER));
        assert_eq!(terminal_trade_template("seller_cancelled"), Some(MSG_TRADES_FAILED_SELLER));
        assert_eq!(terminal_trade_template("buyer_reverted"), Some(crate::messages::MSG_TRADES_REVERTED_BUYER));
        assert_eq!(terminal_trade_template("seller_reverted"), Some(crate::messages::MSG_TRADES_REVERTED_SELLER));
        assert_eq!(terminal_trade_template("terminal_unclassified"), None);
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
