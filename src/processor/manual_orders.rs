use super::inventory_fulfillment::{DeliveryContext, submit_delivery_attempt};
use crate::{
    db::manual_orders::{AttemptClaim, AttemptParameters},
    state::AppState,
    steam::trade_link::TradeLink,
};
use std::sync::Arc;
use uuid::Uuid;

/// Claiming commits all parameters before any buy-for call. A duplicate request
/// returns the existing attempt and never schedules a second external purchase.
pub async fn start_attempt(
    state: &Arc<AppState>,
    channel: &str,
    order_id: Uuid,
    parameters: AttemptParameters,
    actor: &str,
    initial: bool,
) -> Result<&'static str, String> {
    let order = state
        .db
        .get_manual_order(channel, order_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("order_not_found")?;
    let mut context = DeliveryContext::load(state, order.inventory_id).await?;
    if context.api_key.trim().is_empty() {
        return Err("market_not_configured".into());
    }
    let trade = TradeLink::parse_single_url(&parameters.trade_link).ok_or("invalid_trade_link")?;
    let money = state
        .market_client
        .get_money(&context.api_key)
        .await
        .map_err(|_| "market_unavailable")?;
    if !money.success {
        return Err("market_unavailable".into());
    }
    let currency = money
        .currency
        .filter(|c| !c.trim().is_empty())
        .ok_or("currency_unknown")?;
    if !currency.eq_ignore_ascii_case(&order.currency) {
        return Err("account_currency_changed".into());
    }
    context.inventory.2 = parameters.max_price;
    context.chance_to_transfer = parameters.chance_to_transfer;
    let claim = state
        .db
        .begin_manual_attempt(
            order.inventory_id,
            &parameters,
            &trade.partner,
            actor,
            initial,
        )
        .await
        .map_err(|e| e.to_string())?;
    match claim {
        AttemptClaim::New(custom) => {
            let worker = state.clone();
            state.spawn_task(async move {
                if let Err(_error) = submit_delivery_attempt(&worker, context, custom, trade).await
                {
                    // The durable CALLING attempt remains recoverable. Error
                    // strings can contain external URLs, so only log identity.
                    tracing::warn!(%order_id, "Manual attempt awaits durable reconciliation");
                }
            });
            Ok("started")
        }
        AttemptClaim::Existing(_) => Ok("existing"),
        AttemptClaim::Conflict => Err("request_conflict".into()),
        AttemptClaim::Blocked => Err("delivery_blocked".into()),
    }
}

pub async fn recover_initial_orders(state: &Arc<AppState>) {
    let Ok(queued) = state.db.queued_manual_orders().await else {
        return;
    };
    let mut workers = tokio::task::JoinSet::new();
    for (channel, id) in queued {
        if state.shutdown_token.is_cancelled() {
            return;
        }
        let state = state.clone();
        workers.spawn(async move {
            let Ok(parameters) = state.db.manual_initial_parameters(id).await else {
                return;
            };
            let Ok(Some(order)) = state.db.get_manual_order(&channel, id).await else {
                return;
            };
            let _ = start_attempt(&state, &channel, id, parameters, &order.created_by, true).await;
        });
        if workers.len() >= 8 {
            let _ = workers.join_next().await;
        }
    }
    while workers.join_next().await.is_some() {}
}
