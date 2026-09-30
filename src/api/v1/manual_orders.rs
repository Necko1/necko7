use crate::{
    api::{
        error::ApiError,
        extractor::{
            authorized_channel::AuthorizedChannel, json::JsonArg, path::PathArg, query::QueryArg,
        },
    },
    db::{
        broadcaster_settings::BroadcasterSetting,
        manual_orders::{AttemptParameters, ManualAuditEvent, ManualOrder, fingerprint},
    },
    processor::manual_orders::start_attempt,
    state::AppState,
    steam::{market, trade_link::TradeLink},
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct OrderPath {
    pub id: Uuid,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateManualOrder {
    pub item_name: String,
    pub currency: String,
    #[serde(flatten)]
    pub parameters: AttemptParameters,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataBody {
    pub description: String,
    pub tags: Vec<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseBody {
    pub reason: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PreviewBody {
    pub item_name: String,
    pub currency: Option<String>,
    pub max_price: Option<i64>,
    pub chance_to_transfer: Option<i16>,
    pub trade_link: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PreviewResponse {
    pub item_name: String,
    pub currency: String,
    pub min_price: i64,
    pub max_price: i64,
    pub chance_to_transfer: i16,
    pub trade_link: Option<String>,
    pub steam_partner: Option<String>,
}

#[derive(Deserialize, IntoParams)]
pub struct CatalogQuery {
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
#[derive(Serialize, ToSchema)]
pub struct CatalogItem {
    pub market_hash_name: String,
    pub price: i64,
    pub volume: i64,
}
#[derive(Serialize, ToSchema)]
pub struct CatalogResponse {
    pub items: Vec<CatalogItem>,
    pub currency: String,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
#[derive(Deserialize, IntoParams)]
pub struct OrderQuery {
    pub search: Option<String>,
    pub status: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
#[derive(Serialize, ToSchema)]
pub struct OrderList {
    pub items: Vec<ManualOrder>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

fn invalid(message: &str, param: &str) -> ApiError {
    ApiError::UnprocessableEntity {
        message: message.into(),
        param: param.into(),
    }
}

pub fn normalize_parameters(parameters: &mut AttemptParameters) -> Result<String, ApiError> {
    if !(1..=i32::MAX as i64).contains(&parameters.max_price) {
        return Err(invalid("invalid_price", "max_price"));
    }
    if !(0..=100).contains(&parameters.chance_to_transfer) {
        return Err(invalid("invalid_chance", "chance_to_transfer"));
    }
    let trade = TradeLink::parse_single_url(&parameters.trade_link)
        .ok_or_else(|| invalid("invalid_trade_link", "trade_link"))?;
    parameters.trade_link = format!(
        "https://steamcommunity.com/tradeoffer/new/?partner={}&token={}",
        trade.partner, trade.token
    );
    Ok(trade.partner)
}

fn normalize_metadata(description: &mut String, tags: &mut Vec<String>) -> Result<(), ApiError> {
    *description = description.trim().into();
    if description.chars().count() > 4000 {
        return Err(invalid("description_too_long", "description"));
    }
    *tags = tags
        .iter()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    if tags.len() > 20 || tags.iter().any(|t| t.chars().count() > 64) {
        return Err(invalid("invalid_tags", "tags"));
    }
    Ok(())
}

async fn account(
    state: &Arc<AppState>,
    channel: &str,
) -> Result<(BroadcasterSetting, String), ApiError> {
    let settings = state
        .db
        .get_broadcaster_setting(channel)
        .await?
        .ok_or_else(|| invalid("market_not_configured", "channel_id"))?;
    if settings.market_api_key.trim().is_empty() {
        return Err(invalid("market_not_configured", "channel_id"));
    }
    let balance = state
        .market_client
        .get_money(&settings.market_api_key)
        .await
        .map_err(|_| invalid("market_unavailable", "channel_id"))?;
    if !balance.success {
        return Err(invalid("market_unavailable", "channel_id"));
    }
    let currency = balance
        .currency
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| invalid("currency_unknown", "currency"))?
        .to_uppercase();
    if !["RUB", "USD", "EUR"].contains(&currency.as_str()) {
        return Err(invalid("currency_unknown", "currency"));
    }
    Ok((settings, currency))
}

async fn quote(
    state: &Arc<AppState>,
    channel: &str,
    body: PreviewBody,
) -> Result<PreviewResponse, ApiError> {
    let (settings, currency) = account(state, channel).await?;
    if body
        .currency
        .as_ref()
        .is_some_and(|c| !c.eq_ignore_ascii_case(&currency))
    {
        return Err(invalid("account_currency_changed", "currency"));
    }
    let item_name = body.item_name.trim();
    if item_name.is_empty() || item_name.chars().count() > 512 {
        return Err(invalid("invalid_item", "item_name"));
    }
    let found = state
        .market_client
        .search_item(&settings.market_api_key, item_name)
        .await
        .map_err(|_| invalid("market_unavailable", "item_name"))?;
    if !found.success {
        return Err(invalid("market_unavailable", "item_name"));
    }
    if found
        .currency
        .as_ref()
        .is_some_and(|c| !c.eq_ignore_ascii_case(&currency))
    {
        return Err(invalid("account_currency_changed", "currency"));
    }
    let min_price = found
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.market_hash_name == item_name && i.price > 0 && i.count > 0)
        .map(|i| i.price)
        .min()
        .ok_or_else(|| invalid("item_unavailable", "item_name"))?;
    let chance = body
        .chance_to_transfer
        .unwrap_or(settings.market_chance_to_transfer);
    let max = body.max_price.unwrap_or(min_price);
    if !(1..=i32::MAX as i64).contains(&max) {
        return Err(invalid("invalid_price", "max_price"));
    }
    if !(0..=100).contains(&chance) {
        return Err(invalid("invalid_chance", "chance_to_transfer"));
    }
    let (trade_link, steam_partner) = match body.trade_link {
        Some(link) => {
            let mut params = AttemptParameters {
                request_id: Uuid::new_v4(),
                max_price: max,
                chance_to_transfer: chance,
                trade_link: link,
            };
            let partner = normalize_parameters(&mut params)?;
            (Some(params.trade_link), Some(partner))
        }
        None => (None, None),
    };
    Ok(PreviewResponse {
        item_name: item_name.into(),
        currency,
        min_price,
        max_price: max,
        chance_to_transfer: chance,
        trade_link,
        steam_partner,
    })
}

async fn require_order(
    state: &Arc<AppState>,
    auth: &AuthorizedChannel,
    id: Uuid,
) -> Result<ManualOrder, ApiError> {
    auth.require_editor()?;
    state
        .db
        .get_manual_order(&auth.channel_id, id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            message: "order_not_found".into(),
        })
}

#[utoipa::path(get, path="/api/v1/broadcasters/{channel_id}/manual-orders/catalog", tag="Manual Orders",
    params(("channel_id"=String,Path),CatalogQuery), responses((status=200,body=CatalogResponse)), security(("session_id"=[])))]
pub async fn catalog(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<CatalogQuery>,
) -> Result<Json<CatalogResponse>, ApiError> {
    auth.require_editor()?;
    let (_, currency) = account(&state, &auth.channel_id).await?;
    let prices = state
        .get_cached_or_fetch_prices(&currency)
        .await
        .map_err(|_| invalid("market_unavailable", "search"))?;
    let words: Vec<String> = query
        .search
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut items: Vec<_> = prices
        .iter()
        .filter(|i| {
            i.price.is_finite()
                && i.price > 0.0
                && i.volume > 0
                && words
                    .iter()
                    .all(|word| i.market_hash_name.to_lowercase().contains(word))
        })
        .map(|i| CatalogItem {
            market_hash_name: i.market_hash_name.clone(),
            price: market::major_to_minor(i.price, &currency),
            volume: i.volume,
        })
        .collect();
    items.sort_by(|a, b| a.market_hash_name.cmp(&b.market_hash_name));
    let total = items.len() as i64;
    let limit = query.limit.unwrap_or(24).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    Ok(Json(CatalogResponse {
        items: items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect(),
        currency,
        total,
        limit,
        offset,
    }))
}

#[utoipa::path(post, path="/api/v1/broadcasters/{channel_id}/manual-orders/preview", tag="Manual Orders", request_body=PreviewBody,
    params(("channel_id"=String,Path)), responses((status=200,body=PreviewResponse)), security(("session_id"=[])))]
pub async fn preview(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<PreviewBody>,
) -> Result<Json<PreviewResponse>, ApiError> {
    auth.require_editor()?;
    Ok(Json(quote(&state, &auth.channel_id, body).await?))
}

#[utoipa::path(get, path="/api/v1/broadcasters/{channel_id}/manual-orders", tag="Manual Orders", params(("channel_id"=String,Path),OrderQuery),
    responses((status=200,body=OrderList)), security(("session_id"=[])))]
pub async fn list(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    QueryArg(query): QueryArg<OrderQuery>,
) -> Result<Json<OrderList>, ApiError> {
    auth.require_editor()?;
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let (items, total) = state
        .db
        .list_manual_orders(
            &auth.channel_id,
            &query.search.unwrap_or_default(),
            query.status.as_deref().filter(|s| !s.is_empty()),
            query.tag.as_deref().filter(|s| !s.is_empty()),
            limit,
            offset,
        )
        .await?;
    Ok(Json(OrderList {
        items,
        total,
        limit,
        offset,
    }))
}

#[utoipa::path(get, path="/api/v1/broadcasters/{channel_id}/manual-orders/{id}", tag="Manual Orders",
    params(("channel_id"=String,Path),("id"=Uuid,Path)), responses((status=200,body=ManualOrder)), security(("session_id"=[])))]
pub async fn detail(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<OrderPath>,
) -> Result<Json<ManualOrder>, ApiError> {
    Ok(Json(require_order(&state, &auth, path.id).await?))
}

#[utoipa::path(get, path="/api/v1/broadcasters/{channel_id}/manual-orders/{id}/audit", tag="Manual Orders",
    params(("channel_id"=String,Path),("id"=Uuid,Path)), responses((status=200,body=Vec<ManualAuditEvent>)), security(("session_id"=[])))]
pub async fn audit(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<OrderPath>,
) -> Result<Json<Vec<ManualAuditEvent>>, ApiError> {
    require_order(&state, &auth, path.id).await?;
    Ok(Json(
        state.db.manual_audit(&auth.channel_id, path.id).await?,
    ))
}

#[utoipa::path(post, path="/api/v1/broadcasters/{channel_id}/manual-orders", tag="Manual Orders", request_body=CreateManualOrder,
    params(("channel_id"=String,Path)), responses((status=200,body=ManualOrder),(status=409,description="Request identity conflict")), security(("session_id"=[])))]
pub async fn create(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(mut body): JsonArg<CreateManualOrder>,
) -> Result<Json<ManualOrder>, ApiError> {
    auth.require_editor()?;
    let partner = normalize_parameters(&mut body.parameters)?;
    normalize_metadata(&mut body.description, &mut body.tags)?;
    body.item_name = body.item_name.trim().into();
    body.currency = body.currency.to_uppercase();
    let hash = fingerprint(&body);
    if let Some((id, existing_hash)) = state
        .db
        .manual_order_by_request(&auth.channel_id, body.parameters.request_id)
        .await?
    {
        if hash != existing_hash {
            return Err(ApiError::Conflict {
                message: "request_conflict".into(),
            });
        }
        return Ok(Json(require_order(&state, &auth, id).await?));
    }
    quote(
        &state,
        &auth.channel_id,
        PreviewBody {
            item_name: body.item_name.clone(),
            currency: Some(body.currency.clone()),
            max_price: Some(body.parameters.max_price),
            chance_to_transfer: Some(body.parameters.chance_to_transfer),
            trade_link: Some(body.parameters.trade_link.clone()),
        },
    )
    .await?;
    let (id, new, matches) = state
        .db
        .create_manual_order(
            &auth.channel_id,
            &body.item_name,
            &body.currency,
            &body.parameters,
            &partner,
            &body.description,
            &body.tags,
            &auth.user_id,
            &hash,
        )
        .await?;
    if !matches {
        return Err(ApiError::Conflict {
            message: "request_conflict".into(),
        });
    }
    if new {
        let worker = state.clone();
        let channel = auth.channel_id.clone();
        let actor = auth.user_id.clone();
        state.spawn_task(async move {
            let _ = start_attempt(&worker, &channel, id, body.parameters, &actor, true).await;
        });
    }
    Ok(Json(require_order(&state, &auth, id).await?))
}

#[utoipa::path(post, path="/api/v1/broadcasters/{channel_id}/manual-orders/{id}/retry", tag="Manual Orders", request_body=AttemptParameters,
    params(("channel_id"=String,Path),("id"=Uuid,Path)), responses((status=200,body=ManualOrder),(status=409,description="Unsafe action or request conflict")), security(("session_id"=[])))]
pub async fn retry(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<OrderPath>,
    JsonArg(mut body): JsonArg<AttemptParameters>,
) -> Result<Json<ManualOrder>, ApiError> {
    let mut order = require_order(&state, &auth, path.id).await?;
    normalize_parameters(&mut body)?;
    // Replay remains available even if Market is down or the attempt completed.
    if let Some(prior) = order
        .attempts
        .iter()
        .find(|a| a.request_id == body.request_id)
    {
        if fingerprint(&AttemptParameters {
            request_id: prior.request_id,
            max_price: prior.max_price,
            chance_to_transfer: prior.chance_to_transfer,
            trade_link: prior.trade_link.clone(),
        }) != fingerprint(&body)
        {
            return Err(ApiError::Conflict {
                message: "request_conflict".into(),
            });
        }
        return Ok(Json(order));
    }
    if !order.can_retry
        && let Some(last) = order.attempts.last()
    {
        crate::processor::inventory_fulfillment::reconcile_delivery(
            &state,
            order.inventory_id,
            &last.custom_id,
        )
        .await
        .map_err(|_| invalid("market_unavailable", "id"))?;
        order = require_order(&state, &auth, path.id).await?;
    }
    if !order.can_retry {
        return Err(ApiError::Conflict {
            message: order
                .action_block_reason
                .unwrap_or_else(|| "delivery_blocked".into()),
        });
    }
    start_attempt(
        &state,
        &auth.channel_id,
        path.id,
        body,
        &auth.user_id,
        false,
    )
    .await
    .map_err(|error| {
        if error == "request_conflict" || error == "delivery_blocked" {
            ApiError::Conflict { message: error }
        } else {
            invalid(&error, "id")
        }
    })?;
    Ok(Json(require_order(&state, &auth, path.id).await?))
}

#[utoipa::path(patch, path="/api/v1/broadcasters/{channel_id}/manual-orders/{id}", tag="Manual Orders", request_body=MetadataBody,
    params(("channel_id"=String,Path),("id"=Uuid,Path)), responses((status=200,body=ManualOrder)), security(("session_id"=[])))]
pub async fn metadata(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<OrderPath>,
    JsonArg(mut body): JsonArg<MetadataBody>,
) -> Result<Json<ManualOrder>, ApiError> {
    require_order(&state, &auth, path.id).await?;
    normalize_metadata(&mut body.description, &mut body.tags)?;
    state
        .db
        .update_manual_metadata(
            &auth.channel_id,
            path.id,
            &body.description,
            &body.tags,
            &auth.user_id,
        )
        .await?;
    Ok(Json(require_order(&state, &auth, path.id).await?))
}

#[utoipa::path(post, path="/api/v1/broadcasters/{channel_id}/manual-orders/{id}/close", tag="Manual Orders", request_body=CloseBody,
    params(("channel_id"=String,Path),("id"=Uuid,Path)), responses((status=200,body=ManualOrder),(status=409,description="Delivery may still proceed")), security(("session_id"=[])))]
pub async fn close(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    PathArg(path): PathArg<OrderPath>,
    JsonArg(body): JsonArg<CloseBody>,
) -> Result<Json<ManualOrder>, ApiError> {
    let order = require_order(&state, &auth, path.id).await?;
    let reason = body.reason.trim();
    if reason.is_empty() || reason.chars().count() > 2000 {
        return Err(invalid("close_reason_required", "reason"));
    }
    if order.status == "CANCELLED" && order.close_reason.as_deref() == Some(reason) {
        return Ok(Json(order));
    }
    if !order.can_close
        && let Some(last) = order.attempts.last()
    {
        crate::processor::inventory_fulfillment::reconcile_delivery(
            &state,
            order.inventory_id,
            &last.custom_id,
        )
        .await
        .map_err(|_| invalid("market_unavailable", "id"))?;
    }
    if !state
        .db
        .close_manual_order(&auth.channel_id, path.id, reason, &auth.user_id)
        .await?
    {
        return Err(ApiError::Conflict {
            message: "delivery_blocked".into(),
        });
    }
    Ok(Json(require_order(&state, &auth, path.id).await?))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/broadcasters/{channel_id}/manual-orders",
            get(list).post(create),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/catalog",
            get(catalog),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/preview",
            post(preview),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/{id}",
            get(detail).patch(metadata),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/{id}/audit",
            get(audit),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/{id}/retry",
            post(retry),
        )
        .route(
            "/broadcasters/{channel_id}/manual-orders/{id}/close",
            post(close),
        )
}
