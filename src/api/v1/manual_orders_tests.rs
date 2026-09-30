//! Integration tests use a disposable TEST_DATABASE_URL and a localhost Market
//! server. The mock client cannot issue real purchases.
use super::manual_orders::*;
use crate::{db::manual_orders::AttemptParameters, state::AppState, steam::market};
use axum::{Json, Router, extract::State, routing::get};
use axum::{
    body::Body,
    extract::Query,
    http::{Request, StatusCode},
    response::IntoResponse,
};
use parking_lot::{Mutex, RwLock};
use serde_json::{Value, json};
use std::sync::Arc;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU16, AtomicU64, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct MockMarket {
    calls: Arc<Mutex<Vec<HashMap<String, String>>>>,
    buy: Arc<RwLock<Value>>,
    info: Arc<RwLock<Value>>,
    currency: Arc<RwLock<Option<String>>>,
    buy_status: Arc<AtomicU16>,
    money_delay_ms: Arc<AtomicU64>,
    search_delay_ms: Arc<AtomicU64>,
}

async fn response(
    router: &Router,
    method: &str,
    path: &str,
    cookie: Option<Uuid>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("Content-Type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("Cookie", format!("session_id={cookie}"));
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn observation(stage: &str, trade: bool, accepted: bool, causer: Option<&str>) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({"success":true,"data":{"item_id":"456","market_hash_name":"Glock-18 | Vogue (Field-Tested)",
        "classid":"1","instance":"0","time":"0","stage":stage,"paid":25.0,"currency":"RUB",
        "trade_id":if trade {json!("789")} else {Value::Null},"send_until":now+600,
        "receive_until":if trade {json!(now+300)} else {Value::Null},
        "settlement":if accepted {json!(now)} else {Value::Null},"causer":causer,"cancellation_reason":null}})
}

async fn wait_status(
    state: &Arc<AppState>,
    channel: &str,
    id: Uuid,
    expected: &str,
) -> crate::db::manual_orders::ManualOrder {
    for _ in 0..150 {
        let order = state
            .db
            .get_manual_order(channel, id)
            .await
            .unwrap()
            .unwrap();
        if order.status == expected
            && (expected != "ORDER_PENDING"
                || order
                    .attempts
                    .last()
                    .is_some_and(|a| a.status == "ORDER_CREATED"))
        {
            return order;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!(
        "expected {expected}, got {:?}",
        state
            .db
            .get_manual_order(channel, id)
            .await
            .unwrap()
            .map(|o| o.status)
    );
}

#[test]
fn manual_validation_and_openapi_contract() {
    let mut parameters = AttemptParameters {
        request_id: Uuid::new_v4(),
        max_price: 2750,
        chance_to_transfer: 80,
        trade_link: "steamcommunity.com/tradeoffer/new/?partner=123&token=example".into(),
    };
    assert_eq!(normalize_parameters(&mut parameters).unwrap(), "123");
    assert!(parameters.trade_link.starts_with("https://"));
    for (price, chance) in [
        (0, 80),
        (-1, 80),
        (i32::MAX as i64 + 1, 80),
        (2750, -1),
        (2750, 101),
    ] {
        let mut invalid_parameters = parameters.clone();
        invalid_parameters.max_price = price;
        invalid_parameters.chance_to_transfer = chance;
        assert!(normalize_parameters(&mut invalid_parameters).is_err());
    }
    parameters.trade_link = "https://evil.example/tradeoffer/new/?partner=123&token=example".into();
    assert!(normalize_parameters(&mut parameters).is_err());
    use utoipa::OpenApi;
    let doc = super::ApiDoc::openapi();
    let value = serde_json::to_value(doc).unwrap();
    assert!(
        value["paths"]["/api/v1/broadcasters/{channel_id}/manual-orders/{id}"]["patch"].is_object()
    );
    assert!(
        value["components"]["schemas"]["ManualOrder"]["properties"]
            .get("twitch_reward_id")
            .is_none()
    );
    assert!(
        value["components"]["schemas"]["AttemptParameters"]["properties"]["request_id"].is_object()
    );
    assert!(
        value["components"]["schemas"]["CreateManualOrder"]
            .to_string()
            .contains("AttemptParameters")
    );
    assert!(crate::messages::inventory_chat_allowed("MANUAL", "trades.created") == false);
    assert_eq!(
        crate::db::channel_logs::ChannelLogCategory::from_str_case_insensitive("manual"),
        Some(crate::db::channel_logs::ChannelLogCategory::Manual)
    );
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn manual_orders_database_api_and_mocked_delivery() {
    let mock = MockMarket {
        calls: Arc::default(),
        buy: Arc::new(RwLock::new(
            json!({"success":false,"code":0,"error":"Not enough funds"}),
        )),
        info: Arc::new(RwLock::new(json!({"success":false,"data":false}))),
        currency: Arc::new(RwLock::new(Some("RUB".into()))),
        buy_status: Arc::new(AtomicU16::new(200)),
        money_delay_ms: Arc::default(),
        search_delay_ms: Arc::default(),
    };
    let mock_router = Router::new()
        .route("/get-money",get(|State(mock):State<MockMarket>| async move {
            tokio::time::sleep(std::time::Duration::from_millis(mock.money_delay_ms.load(Ordering::SeqCst))).await;
            Json(json!({"success":true,"money":1000,"money_settlement":0,"currency":mock.currency.read().clone()})) }))
        .route("/search-item-by-hash-name",get(|State(mock):State<MockMarket>,Query(query):Query<HashMap<String,String>>| async move {
            tokio::time::sleep(std::time::Duration::from_millis(mock.search_delay_ms.load(Ordering::SeqCst))).await;
            Json(json!({"success":true,"currency":mock.currency.read().clone(),"data":[
                {"market_hash_name":query["hash_name"],"price":3000,"class":1,"instance":0,"count":1},
                {"market_hash_name":query["hash_name"],"price":2500,"class":2,"instance":0,"count":1}]}))
        }))
        .route("/prices/RUB.json",get(|| async { Json(json!({"success":true,"items":(0..75).map(|i|json!({"market_hash_name":format!("Glock-18 | Vogue variant {i:02}"),"price":25.0,"volume":1})).collect::<Vec<_>>()})) }))
        .route("/buy-for",get(|State(mock):State<MockMarket>,Query(query):Query<HashMap<String,String>>| async move {
            mock.calls.lock().push(query);
            (StatusCode::from_u16(mock.buy_status.load(Ordering::SeqCst)).unwrap(),Json(mock.buy.read().clone())).into_response()
        }))
        .route("/get-buy-info-by-custom-id",get(|State(mock):State<MockMarket>| async move {Json(mock.info.read().clone())}))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut state = AppState::from_env(db.clone()).await.unwrap();
    Arc::get_mut(&mut state).unwrap().market_client =
        market::MarketClient::for_test(format!("http://{address}"));
    let channel = format!("manual-qa-{}", Uuid::new_v4());
    let other_channel = format!("manual-other-{}", Uuid::new_v4());
    let owner = Uuid::new_v4();
    let editor = Uuid::new_v4();
    let viewer = Uuid::new_v4();
    for (user, role, cookie) in [
        (&channel, "OWNER", owner),
        (&format!("editor-{channel}"), "EDITOR", editor),
        (&format!("viewer-{channel}"), "VIEWER", viewer),
    ] {
        sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
            .bind(user)
            .execute(db.pool())
            .await
            .unwrap();
        if role == "OWNER" {
            sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',NOW(),NOW())")
                .bind(&channel).execute(db.pool()).await.unwrap();
        }
        if role != "VIEWER" {
            sqlx::query("INSERT INTO channel_permissions(channel_id,user_id,role,granted_by) VALUES($1,$2,$3,$1)")
                .bind(&channel).bind(user).bind(role).execute(db.pool()).await.unwrap();
        }
        sqlx::query("INSERT INTO sessions(session_id,user_id,expires_at) VALUES($1,$2,NOW()+INTERVAL '1 hour')")
            .bind(cookie).bind(user).execute(db.pool()).await.unwrap();
    }
    // Switching off Twitch automation must not disable manual spending.
    db.get_or_create_broadcaster_setting(&channel)
        .await
        .unwrap();
    sqlx::query("UPDATE broadcaster_settings SET market_api_key='fake-market-key',market_chance_to_transfer=80,is_active=FALSE WHERE channel_id=$1")
        .bind(&channel).execute(db.pool()).await.unwrap();
    let router = router().with_state(state.clone());
    let base = format!("/broadcasters/{channel}/manual-orders");
    assert_eq!(
        response(&router, "GET", &base, None, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        response(&router, "GET", &base, Some(viewer), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let catalog = response(
        &router,
        "GET",
        &format!("{base}/catalog?search=Glock-18%20Vogue&offset=48&limit=24"),
        Some(editor),
        Value::Null,
    )
    .await;
    assert_eq!(catalog.0, StatusCode::OK);
    assert_eq!(catalog.1["total"], 75);
    assert_eq!(catalog.1["items"].as_array().unwrap().len(), 24);
    let preview = response(
        &router,
        "POST",
        &format!("{base}/preview"),
        Some(editor),
        json!({"item_name":"Glock-18 | Vogue (Field-Tested)"}),
    )
    .await;
    assert_eq!(preview.0, StatusCode::OK);
    assert_eq!(preview.1["min_price"], 2500);
    assert_eq!(preview.1["chance_to_transfer"], 80);
    // Read-only latency reproduction: this localhost mock delays responses;
    // no buy request has been issued, and production latency is not inferred.
    mock.money_delay_ms.store(4000, Ordering::SeqCst);
    mock.search_delay_ms.store(4500, Ordering::SeqCst);
    let timed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{base}/preview"))
                .header("Content-Type", "application/json")
                .header("Cookie", format!("session_id={editor}"))
                .body(Body::from(
                    json!({"item_name":"Glock-18 | Vogue (Field-Tested)"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(timed.status(), StatusCode::OK);
    let header = timed.headers()["Server-Timing"].to_str().unwrap();
    println!("MOCK_PREVIEW_SERVER_TIMING: {header}");
    let measurements: HashMap<&str, f64> = header
        .split(", ")
        .map(|metric| {
            let (name, duration) = metric.split_once(";dur=").unwrap();
            (name, duration.parse().unwrap())
        })
        .collect();
    assert!(measurements["money_http"] >= 3900.0);
    assert!(measurements["search_http"] >= 4400.0);
    assert!(measurements["total"] >= 8400.0);
    assert!(
        measurements["auth_extract"] + measurements["settings_db"] + measurements["local_other"]
            < 1000.0
    );
    assert!(measurements["money_parse"] + measurements["search_parse"] < 100.0);
    assert!(Uuid::parse_str(timed.headers()["X-Preview-Request-Id"].to_str().unwrap()).is_ok());
    assert!(mock.calls.lock().is_empty());
    mock.money_delay_ms.store(0, Ordering::SeqCst);
    mock.search_delay_ms.store(0, Ordering::SeqCst);
    *mock.currency.write() = None;
    assert_eq!(
        response(
            &router,
            "GET",
            &format!("{base}/catalog"),
            Some(owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    *mock.currency.write() = Some("RUB".into());
    let body = json!({"request_id":Uuid::new_v4(),"item_name":"Glock-18 | Vogue (Field-Tested)","currency":"RUB",
        "max_price":2750,"chance_to_transfer":81,"trade_link":"https://steamcommunity.com/tradeoffer/new/?partner=123&token=FAKE-TOKEN",
        "description":"Telegram giveaway","tags":["telegram","giveaway"]});
    let (a, b) = tokio::join!(
        response(&router, "POST", &base, Some(owner), body.clone()),
        response(&router, "POST", &base, Some(editor), body.clone())
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert_eq!(a.1["id"], b.1["id"]);
    let id = Uuid::parse_str(a.1["id"].as_str().unwrap()).unwrap();
    let path = format!("{base}/{id}");
    let first = wait_status(&state, &channel, id, "INSUFFICIENT_FUNDS").await;
    assert_eq!(mock.calls.lock().len(), 1);
    assert_eq!(first.attempts[0].max_price, 2750);
    assert_eq!(first.attempts[0].chance_to_transfer, 81);
    assert!(first.can_retry && first.can_close);
    assert!(
        sqlx::query(
            "UPDATE manual_orders SET closed_at=NOW(),closed_by='qa',close_reason=NULL WHERE id=$1"
        )
        .bind(id)
        .execute(db.pool())
        .await
        .is_err()
    );
    let fields: (Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT redemption_id,viewer_id FROM inventory_items WHERE manual_order_id=$1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(fields, (None, None));
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM redemptions r JOIN rewards rw ON rw.twitch_id=r.twitch_reward_id WHERE rw.streamer_id=$1")
        .bind(&channel).fetch_one(db.pool()).await.unwrap();
    assert_eq!(count, 0);
    assert!(
        db.get_viewer_inventory("123", None, None, None, 100, 0)
            .await
            .unwrap()
            .iter()
            .all(|i| i.id != first.inventory_id)
    );
    assert!(
        sqlx::query("UPDATE inventory_items SET viewer_id='invented' WHERE id=$1")
            .bind(first.inventory_id)
            .execute(db.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE inventory_order_attempts SET chance_to_transfer=99 WHERE inventory_id=$1"
        )
        .bind(first.inventory_id)
        .execute(db.pool())
        .await
        .is_err()
    );
    let mut altered = body.clone();
    altered["max_price"] = json!(9999);
    assert_eq!(
        response(&router, "POST", &base, Some(owner), altered)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        response(&router, "POST", &base, Some(owner), body.clone())
            .await
            .1["id"],
        json!(id)
    );
    assert_eq!(mock.calls.lock().len(), 1);
    let list = response(
        &router,
        "GET",
        &format!("{base}?tag=telegram&status=INSUFFICIENT_FUNDS&search=123"),
        Some(editor),
        Value::Null,
    )
    .await;
    assert_eq!(list.1["total"], 1);
    // All methods are role- and channel-scoped, even when an order ID is known.
    for (method, suffix, data) in [
        ("GET", "", Value::Null),
        ("GET", "/audit", Value::Null),
        (
            "POST",
            "/retry",
            json!({"request_id":Uuid::new_v4(),"max_price":3000,"chance_to_transfer":70,"trade_link":body["trade_link"]}),
        ),
        ("PATCH", "", json!({"description":"x","tags":[]})),
        ("POST", "/close", json!({"reason":"x"})),
    ] {
        assert_eq!(
            response(
                &router,
                method,
                &format!("{path}{suffix}"),
                Some(viewer),
                data
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&other_channel)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',NOW(),NOW())").bind(&other_channel).execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO channel_permissions(channel_id,user_id,role,granted_by) VALUES($1,$2,'OWNER',$2)").bind(&other_channel).bind(&channel).execute(db.pool()).await.unwrap();
    assert_eq!(
        response(
            &router,
            "GET",
            &format!("/broadcasters/{other_channel}/manual-orders/{id}"),
            Some(owner),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Different retry requests race under the same inventory lock.
    *mock.buy.write() = json!({"success":true,"id":"456","price":2900});
    let retry = json!({"request_id":Uuid::new_v4(),"max_price":3200,"chance_to_transfer":65,"trade_link":"https://steamcommunity.com/tradeoffer/new/?partner=456&token=NEW-TOKEN"});
    let mut competing = retry.clone();
    competing["request_id"] = json!(Uuid::new_v4());
    let retry_path = format!("{path}/retry");
    let (a, b) = tokio::join!(
        response(&router, "POST", &retry_path, Some(owner), retry.clone()),
        response(
            &router,
            "POST",
            &retry_path,
            Some(editor),
            competing.clone()
        )
    );
    assert!(a.0 == StatusCode::OK || b.0 == StatusCode::OK);
    assert!(a.0 == StatusCode::CONFLICT || b.0 == StatusCode::CONFLICT);
    let winning = if a.0 == StatusCode::OK {
        retry
    } else {
        competing
    };
    let second = wait_status(&state, &channel, id, "ORDER_PENDING").await;
    assert_eq!(mock.calls.lock().len(), 2);
    assert_eq!(second.attempts.len(), 2);
    assert_eq!(second.attempts[0].max_price, 2750);
    assert_eq!(second.attempts[1].max_price, 3200);
    assert_eq!(mock.calls.lock()[1]["chance_to_transfer"], "65");
    assert_eq!(mock.calls.lock()[1]["partner"], "456");
    assert_eq!(
        response(
            &router,
            "POST",
            &format!("{path}/retry"),
            Some(owner),
            winning.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(mock.calls.lock().len(), 2);
    assert!(!second.can_retry && !second.can_close);
    assert_eq!(
        response(
            &router,
            "POST",
            &format!("{path}/close"),
            Some(owner),
            json!({"reason":"stop"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        response(
            &router,
            "PATCH",
            &path,
            Some(editor),
            json!({"description":"Updated during delivery","tags":["updated"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let custom = second.attempts.last().unwrap().custom_id.clone();
    *mock.info.write() = observation("1", true, false, None);
    crate::processor::inventory_fulfillment::reconcile_delivery(
        &state,
        second.inventory_id,
        &custom,
    )
    .await
    .unwrap();
    assert_eq!(
        db.get_manual_order(&channel, id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "TRADE_WAITING"
    );
    *mock.info.write() = observation("1", false, true, None);
    crate::processor::inventory_fulfillment::reconcile_delivery(
        &state,
        second.inventory_id,
        &custom,
    )
    .await
    .unwrap();
    assert_eq!(
        db.get_manual_order(&channel, id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "TRADE_ACCEPTED"
    );
    *mock.info.write() = observation("2", false, false, None);
    crate::processor::inventory_fulfillment::reconcile_delivery(
        &state,
        second.inventory_id,
        &custom,
    )
    .await
    .unwrap();
    let delivered = db.get_manual_order(&channel, id).await.unwrap().unwrap();
    assert_eq!(delivered.status, "DELIVERED");
    assert!(delivered.attempts[1].settlement.is_some());
    *mock.info.write() = observation("1", true, false, None);
    crate::processor::inventory_fulfillment::reconcile_delivery(
        &state,
        second.inventory_id,
        &custom,
    )
    .await
    .unwrap();
    assert_eq!(
        db.get_manual_order(&channel, id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "DELIVERED"
    );
    assert!(
        !db.close_manual_order(&channel, id, "stop", &channel)
            .await
            .unwrap()
    );
    assert_eq!(
        response(
            &router,
            "PATCH",
            &path,
            Some(owner),
            json!({"description":"Completed giveaway","tags":["done"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let audit = db.manual_audit(&channel, id).await.unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|e| e.event_type == "market_stage_2_delivered")
            .count(),
        1
    );
    assert!(audit.iter().all(|e| !e.event_type.starts_with("twitch_")));
    assert!(
        !serde_json::to_string(&audit)
            .unwrap()
            .contains("FAKE-TOKEN")
    );
    assert!(
        sqlx::query("DELETE FROM fulfillment_audit_events WHERE manual_order_id=$1")
            .bind(id)
            .execute(db.pool())
            .await
            .is_err()
    );
    let logs: Vec<(String, Value)> =
        sqlx::query_as("SELECT category,details FROM channel_logs WHERE broadcaster_id=$1")
            .bind(&channel)
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert!(logs.iter().all(|(category, _)| category == "MANUAL"));
    assert!(!serde_json::to_string(&logs).unwrap().contains("NEW-TOKEN"));
    // Recovery starts only the untouched first attempt. HTTP failure never frees
    // a CALLING attempt, even after reconnecting to the database.
    let params = AttemptParameters {
        request_id: Uuid::new_v4(),
        max_price: 2750,
        chance_to_transfer: 80,
        trade_link: body["trade_link"].as_str().unwrap().into(),
    };
    let (uncertain, _, _) = db
        .create_manual_order(
            &channel,
            "Glock-18 | Vogue (Field-Tested)",
            "RUB",
            &params,
            "123",
            "crash recovery",
            &[],
            &channel,
            "test-hash",
        )
        .await
        .unwrap();
    mock.buy_status.store(500, Ordering::SeqCst);
    *mock.info.write() = json!({"success":false,"data":false});
    crate::processor::manual_orders::recover_initial_orders(&state).await;
    let unknown = wait_status(&state, &channel, uncertain, "RECONCILIATION_REQUIRED").await;
    assert!(!unknown.can_retry && !unknown.can_close);
    assert!(
        !db.close_manual_order(&channel, uncertain, "stop", &channel)
            .await
            .unwrap()
    );
    let reconnect = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(
        !reconnect
            .get_manual_order(&channel, uncertain)
            .await
            .unwrap()
            .unwrap()
            .can_close
    );
    let before = mock.calls.lock().len();
    crate::processor::manual_orders::recover_initial_orders(&state).await;
    assert_eq!(mock.calls.lock().len(), before);
    let custom = unknown.attempts[0].custom_id.clone();
    *mock.info.write() = observation("5", false, false, Some("seller"));
    crate::processor::inventory_fulfillment::reconcile_delivery(
        &state,
        unknown.inventory_id,
        &custom,
    )
    .await
    .unwrap();
    let failed = db
        .get_manual_order(&channel, uncertain)
        .await
        .unwrap()
        .unwrap();
    assert!(failed.can_retry && failed.can_close);
    assert!(
        !db.close_manual_order(&channel, uncertain, " ", &channel)
            .await
            .unwrap()
    );
    assert!(
        db.close_manual_order(&channel, uncertain, "Winner declined", &channel)
            .await
            .unwrap()
    );
    let closed = db
        .get_manual_order(&channel, uncertain)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(closed.status, "CANCELLED");
    assert_eq!(closed.close_reason.as_deref(), Some("Winner declined"));
    assert_eq!(closed.attempts.len(), 1);
    assert!(!closed.can_retry && !closed.can_close);
    assert!(
        db.update_manual_metadata(
            &channel,
            uncertain,
            "Retained",
            &["closed".into()],
            &channel
        )
        .await
        .unwrap()
    );
    // A saved CALLING row after a crash must never be purchased again.
    let mut parameters = params.clone();
    parameters.request_id = Uuid::new_v4();
    let (crashed, _, _) = db
        .create_manual_order(
            &channel,
            "Glock-18 | Vogue (Field-Tested)",
            "RUB",
            &parameters,
            "123",
            "",
            &[],
            &channel,
            "crashed",
        )
        .await
        .unwrap();
    let crashed_order = db
        .get_manual_order(&channel, crashed)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        db.begin_manual_attempt(
            crashed_order.inventory_id,
            &parameters,
            "123",
            &channel,
            true
        )
        .await
        .unwrap(),
        crate::db::manual_orders::AttemptClaim::New(_)
    ));
    assert!(
        !db.get_manual_order(&channel, crashed)
            .await
            .unwrap()
            .unwrap()
            .can_close
    );
    crate::processor::manual_orders::recover_initial_orders(&state).await;
    assert_eq!(mock.calls.lock().len(), before);
    // Refusals release an attempt, including an invalid Steam recipient. Echoed
    // secrets are removed before the outcome is retained or exposed.
    mock.buy_status.store(200, Ordering::SeqCst);
    for (code, error, status, outcome) in [
        (
            0,
            "No item at this price or below",
            "RETRY_AVAILABLE",
            "item_unavailable",
        ),
        (
            12,
            "Invalid trade link FAKE-TOKEN fake-market-key",
            "TRADE_LINK_REQUIRED",
            "trade_link",
        ),
    ] {
        *mock.buy.write() = json!({"success":false,"code":code,"error":error});
        let mut parameters = params.clone();
        parameters.request_id = Uuid::new_v4();
        let (refused, _, _) = db
            .create_manual_order(
                &channel,
                "Glock-18 | Vogue (Field-Tested)",
                "RUB",
                &parameters,
                "123",
                "",
                &[],
                &channel,
                "refusal",
            )
            .await
            .unwrap();
        crate::processor::manual_orders::start_attempt(
            &state, &channel, refused, parameters, &channel, true,
        )
        .await
        .unwrap();
        let refused = wait_status(&state, &channel, refused, status).await;
        assert!(refused.can_retry && refused.can_close);
        assert_eq!(refused.attempts[0].outcome_kind.as_deref(), Some(outcome));
        let detail = refused.attempts[0].outcome_detail.as_deref().unwrap();
        assert!(!detail.contains("FAKE-TOKEN") && !detail.contains("fake-market-key"));
    }
    // Terminal buyer failures allow manual recovery regardless of viewer reward
    // policies. Unclassified stage 5 permits closing but never another buy.
    for (causer, accepted, outcome) in [
        (Some("buyer"), false, "buyer_not_accepted"),
        (Some("buyer"), true, "buyer_reverted"),
        (None, false, "terminal_unclassified"),
    ] {
        *mock.buy.write() = json!({"success":true,"id":"456","price":2500});
        let mut parameters = params.clone();
        parameters.request_id = Uuid::new_v4();
        let (terminal, _, _) = db
            .create_manual_order(
                &channel,
                "Glock-18 | Vogue (Field-Tested)",
                "RUB",
                &parameters,
                "123",
                "",
                &[],
                &channel,
                "terminal",
            )
            .await
            .unwrap();
        crate::processor::manual_orders::start_attempt(
            &state, &channel, terminal, parameters, &channel, true,
        )
        .await
        .unwrap();
        let started = wait_status(&state, &channel, terminal, "ORDER_PENDING").await;
        let custom = &started.attempts[0].custom_id;
        *mock.info.write() = observation("1", true, accepted, None);
        crate::processor::inventory_fulfillment::reconcile_delivery(
            &state,
            started.inventory_id,
            custom,
        )
        .await
        .unwrap();
        *mock.info.write() = observation("5", false, false, causer);
        crate::processor::inventory_fulfillment::reconcile_delivery(
            &state,
            started.inventory_id,
            custom,
        )
        .await
        .unwrap();
        let terminal = db
            .get_manual_order(&channel, terminal)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(terminal.attempts[0].outcome_kind.as_deref(), Some(outcome));
        assert!(terminal.can_close);
        assert_eq!(terminal.can_retry, causer.is_some());
        let (a, b) = tokio::join!(
            db.close_manual_order(&channel, terminal.id, "One administrator", &channel),
            db.close_manual_order(&channel, terminal.id, "Another administrator", &channel)
        );
        assert_ne!(a.unwrap(), b.unwrap());
        assert_eq!(
            db.manual_audit(&channel, terminal.id)
                .await
                .unwrap()
                .iter()
                .filter(|e| e.event_type == "manual_order_closed")
                .count(),
            1
        );
    }
    state.shutdown_token.cancel();
    state.tasks.close();
    state.tasks.wait().await;
    server.abort();
}
