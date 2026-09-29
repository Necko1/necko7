use super::*;
use crate::{
    api::extractor::{authorized_channel::AuthorizedChannel, json::JsonArg},
    db::channel_permissions::ChannelRole,
    state::AppState,
};
use axum::extract::State;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

async fn cmd(
    state: &Arc<AppState>,
    channel: &str,
    body: Value,
) -> Result<Value, crate::api::error::ApiError> {
    api::command(
        State(state.clone()),
        AuthorizedChannel {
            user_id: channel.into(),
            user_login: "owner".into(),
            channel_id: channel.into(),
            role: ChannelRole::Owner,
        },
        JsonArg(serde_json::from_value(body).unwrap()),
    )
    .await
    .map(|v| v.0)
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn project_scheduler_storage_and_fulfillment_contracts() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::from_env(db.clone()).await.unwrap();
    let pool = db.pool();
    let channel = format!("script-test-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let p = cmd(&state, &channel, json!({"action":"create","name":"First"}))
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let q = cmd(
        &state,
        &channel,
        json!({"action":"create","name":"Independent"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let project = Uuid::parse_str(&p).unwrap();
    let other = Uuid::parse_str(&q).unwrap();
    assert!(
        cmd(
            &state,
            "another-channel",
            json!({"action":"enable","project_id":p,"enabled":true})
        )
        .await
        .is_err()
    );
    let files = json!({"main.rhai":"fn on_event(ctx) { storage.increment(\"events\", 1); } fn on_timer(ctx) { storage.increment(\"timers\", 1); log.info(ctx.timer.key); }"});
    cmd(
        &state,
        &channel,
        json!({"action":"save","project_id":p,"version":1,"files":files}),
    )
    .await
    .unwrap();
    cmd(
        &state,
        &channel,
        json!({"action":"publish","project_id":p,"version":2}),
    )
    .await
    .unwrap();
    // Failed publication leaves the active revision unchanged; stale saves cannot overwrite another tab.
    assert!(
        cmd(
            &state,
            &channel,
            json!({"action":"save","project_id":p,"version":1,"files":files})
        )
        .await
        .is_err()
    );
    cmd(
        &state,
        &channel,
        json!({"action":"save","project_id":p,"version":2,"files":{"main.rhai":"fn broken("}}),
    )
    .await
    .unwrap();
    assert!(
        cmd(
            &state,
            &channel,
            json!({"action":"publish","project_id":p,"version":3})
        )
        .await
        .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT active_revision FROM script_projects WHERE id=$1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    let attr = service::Attribution {
        project_id: project,
        revision: 1,
        execution_id: Uuid::new_v4(),
        channel_id: channel.clone(),
    };
    let job = service::schedule(pool, &attr, "due", 1, json!({"value":1}))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE script_jobs SET scheduled_for=now()-interval '1 second' WHERE project_id=$1",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    // Re-enable between polling ticks still latches overdue disabled jobs.
    cmd(
        &state,
        &channel,
        json!({"action":"enable","project_id":p,"enabled":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM script_jobs WHERE project_id=$1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        "blocked"
    );
    assert!(!worker::step(state.clone()).await.unwrap());
    cmd(
        &state,
        &channel,
        json!({"action":"enable","project_id":p,"enabled":false}),
    )
    .await
    .unwrap();
    assert!(
        cmd(
            &state,
            &channel,
            json!({"action":"run_job","project_id":p,"job_id":job["id"],"confirmation":""})
        )
        .await
        .is_err()
    );
    cmd(
        &state,
        &channel,
        json!({"action":"run_job","project_id":p,"job_id":job["id"],"confirmation":"RUN"}),
    )
    .await
    .unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM script_jobs WHERE project_id=$1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        "completed"
    );
    assert!(!worker::step(state.clone()).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT value FROM script_storage WHERE project_id=$1 AND key='timers'"
        )
        .bind(project)
        .fetch_one(pool)
        .await
        .unwrap(),
        json!(1)
    );
    // Publish unrelated timer logic; already-created jobs keep revision 1.
    let pinned = service::schedule(pool, &attr, "pinned", 3600, json!({}))
        .await
        .unwrap();
    cmd(&state,&channel,json!({"action":"save","project_id":p,"version":3,"files":{"main.rhai":"fn on_timer(ctx) { throw \"new revision\"; }"}})).await.unwrap();
    cmd(
        &state,
        &channel,
        json!({"action":"publish","project_id":p,"version":4}),
    )
    .await
    .unwrap();
    cmd(
        &state,
        &channel,
        json!({"action":"run_job","project_id":p,"job_id":pinned["id"],"confirmation":"RUN"}),
    )
    .await
    .unwrap();
    worker::step(state.clone()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT value FROM script_storage WHERE project_id=$1 AND key='timers'"
        )
        .bind(project)
        .fetch_one(pool)
        .await
        .unwrap(),
        json!(2)
    );
    cmd(
        &state,
        &channel,
        json!({"action":"rollback","project_id":p,"revision":1}),
    )
    .await
    .unwrap();
    let cancelled = service::schedule(pool, &attr, "cancel", 3600, json!({}))
        .await
        .unwrap();
    cmd(
        &state,
        &channel,
        json!({"action":"cancel_job","project_id":p,"job_id":cancelled["id"]}),
    )
    .await
    .unwrap();
    assert!(
        cmd(
            &state,
            &channel,
            json!({"action":"run_job","project_id":p,"job_id":cancelled["id"],"confirmation":"RUN"})
        )
        .await
        .is_err()
    );
    // Atomic independent counters, JSON limits and dry-run isolation.
    let (a, b) = tokio::join!(
        service::storage_write(pool, project, "counter", Some(json!(1)), true),
        service::storage_write(pool, project, "counter", Some(json!(1)), true)
    );
    a.unwrap();
    b.unwrap();
    service::storage_write(pool, other, "counter", Some(json!(99)), false)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT value FROM script_storage WHERE project_id=$1 AND key='counter'"
        )
        .bind(project)
        .fetch_one(pool)
        .await
        .unwrap(),
        json!(2)
    );
    assert!(
        service::storage_write(
            pool,
            project,
            "large",
            Some(json!("x".repeat(65537))),
            false
        )
        .await
        .is_err()
    );
    let dry=service::run(state.clone(),BTreeMap::from([("main.rhai".into(),"fn on_event(ctx) { storage.increment(\"counter\", 1); if storage.get(\"counter\") != 3 { throw \"bad overlay\"; } log.info(\"dry\"); }".into())]),"on_event".into(),json!({}),attr.clone(),true).await;
    assert!(dry.error.is_none(), "{:?}", dry.error);
    assert_eq!(dry.logs.len(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT value FROM script_storage WHERE project_id=$1 AND key='counter'"
        )
        .bind(project)
        .fetch_one(pool)
        .await
        .unwrap(),
        json!(2)
    );
    // Real SCRIPT-origin fulfillment: no network with operator fulfillment, no Twitch limits.
    db.get_or_create_broadcaster_setting(&channel)
        .await
        .unwrap();
    sqlx::query("UPDATE broadcaster_settings SET is_active=true WHERE channel_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    let reward = Uuid::new_v4();
    sqlx::query("INSERT INTO rewards(twitch_id,streamer_id,is_paused,script_alias,market_item_name,twitch_title,twitch_description,current_market_price,permissible_market_price_deviation,twitch_price_markup_percentage,global_cooldown_seconds,max_redemptions_per_stream,max_redemptions_per_user_per_stream,market_autobuy,created_at,updated_at) VALUES($1,$2,false,'skin','Test item','Test','',100,0,0,99999,1,1,false,now(),now())").bind(reward).bind(&channel).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO chat_messages(message_id,broadcaster_id,chatter_user_id,chatter_user_login,message_text,char_count,sent_at) VALUES($1,$2,'viewer','viewer','hello',5,now())").bind(Uuid::new_v4().to_string()).bind(&channel).execute(pool).await.unwrap();
    sqlx::query("UPDATE rewards SET is_visible=false WHERE twitch_id=$1")
        .bind(reward)
        .execute(pool)
        .await
        .unwrap();
    assert!(
        db.get_active_rewards_by_streamer_id(&channel)
            .await
            .unwrap()
            .iter()
            .any(|r| r.twitch_id == reward)
    );
    let one = service::trigger(&state, &attr, "skin", "viewer", false, &[])
        .await
        .unwrap();
    assert_eq!(one["ok"], true, "{one}");
    let two = service::trigger(&state, &attr, "skin", "viewer", false, &[])
        .await
        .unwrap();
    assert_eq!(two["ok"], true, "{two}");
    // A known account need not have chatted when no chat requirement is configured.
    let known = service::trigger(&state, &attr, "skin", &channel, true, &[])
        .await
        .unwrap();
    assert_eq!(known["ok"], true);
    let stats=service::run(state.clone(),BTreeMap::from([("main.rhai".into(),"fn on_event(ctx) { let s=chat.user_stats(\"viewer\",Duration::from_mins(30)); if s.messages != 1 || s.redemptions.script != 2 { throw \"bad stats\"; } storage.set(\"null-test\",()); if storage.get(\"null-test\",42) != () { throw \"lost null\"; } storage.delete(\"null-test\"); if storage.get(\"null-test\",42) != 42 { throw \"missing delete\"; } }".into())]),"on_event".into(),json!({}),attr.clone(),true).await;
    assert!(stats.error.is_none(), "{:?}", stats.error);
    // The caller can replace one actionable pre-order notice without changing
    // the inventory status or suppressing later trade tracking.
    db.save_viewer_settings("viewer", true, None).await.unwrap();
    sqlx::query("UPDATE rewards SET market_autobuy=true WHERE twitch_id=$1")
        .bind(reward).execute(pool).await.unwrap();
    let suppressed = service::trigger(&state, &attr, "skin", "viewer", false,
        &[crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED.to_owned()]).await.unwrap();
    assert_eq!(suppressed["code"], "trade_link_required", "{suppressed}");
    assert_eq!(suppressed["inventory_status"], "TRADE_LINK_REQUIRED");
    let suppressed_id = Uuid::parse_str(suppressed["fulfillment_id"].as_str().unwrap()).unwrap();
    let saved_keys: Vec<String> = sqlx::query_scalar("SELECT script_suppressed_chat_keys FROM redemptions WHERE fulfillment_id=$1")
        .bind(suppressed_id).fetch_one(pool).await.unwrap();
    assert_eq!(saved_keys, [crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED]);
    let should_send = crate::processor::inventory_fulfillment::should_send_fulfillment_chat;
    assert!(!should_send(&state, "SCRIPT", Some(suppressed_id), crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED).await);
    assert!(should_send(&state, "SCRIPT", Some(suppressed_id), crate::messages::MSG_TRADES_CREATED).await);
    assert!(should_send(&state, "SCRIPT", Some(suppressed_id), crate::messages::MSG_ORDERS_RECONCILIATION_REQUIRED).await);
    sqlx::query("UPDATE redemptions SET script_suppressed_chat_keys=array_append(script_suppressed_chat_keys, $2) WHERE fulfillment_id=$1")
        .bind(suppressed_id).bind(crate::messages::MSG_TRADES_CREATED).execute(pool).await.unwrap();
    assert!(should_send(&state, "SCRIPT", Some(suppressed_id), crate::messages::MSG_TRADES_CREATED).await);
    assert!(should_send(&state, "TWITCH", Some(suppressed_id), crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED).await);
    assert!(should_send(&state, "SCRIPT", None, crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED).await);
    let normal_id = Uuid::parse_str(two["fulfillment_id"].as_str().unwrap()).unwrap();
    assert!(should_send(&state, "SCRIPT", Some(normal_id), crate::messages::MSG_ORDERS_TRADE_LINK_REQUIRED).await);
    let fulfillment = Uuid::parse_str(one["fulfillment_id"].as_str().unwrap()).unwrap();
    let row = db.get_redemption(fulfillment).await.unwrap().unwrap();
    assert_eq!(row.origin, "SCRIPT");
    assert!(row.twitch_redemption_id.is_none());
    assert_eq!(row.twitch_points_cost, 0);
    let item = Uuid::parse_str(one["inventory_item_id"].as_str().unwrap()).unwrap();
    assert!(
        !db.discard_script_item(item, "another-viewer")
            .await
            .unwrap()
    );
    assert!(db.discard_script_item(item, "viewer").await.unwrap());
    assert!(!db.discard_script_item(item, "viewer").await.unwrap());
    assert!(db.get_redemption(fulfillment).await.unwrap().is_some());
    sqlx::query("UPDATE rewards SET chat_min_messages=999 WHERE twitch_id=$1")
        .bind(reward)
        .execute(pool)
        .await
        .unwrap();
    let rejected = service::trigger(&state, &attr, "skin", "viewer", false, &[])
        .await
        .unwrap();
    assert_eq!(
        rejected["code"], "activity_requirement_failed",
        "{rejected}"
    );
    let simulated = service::trigger(&state, &attr, "skin", "viewer", true, &[])
        .await
        .unwrap();
    assert_eq!(simulated["code"], "activity_requirement_failed");
    sqlx::query("UPDATE rewards SET chat_min_messages=NULL,purchase_limits=$2 WHERE twitch_id=$1")
        .bind(reward)
        .bind(json!({"global":[],"user":[{"max_redemptions":1,"window_hours":null}]}))
        .execute(pool)
        .await
        .unwrap();
    let rejected = service::trigger(&state, &attr, "skin", "viewer", false, &[])
        .await
        .unwrap();
    assert_eq!(rejected["code"], "purchase_limit_reached", "{rejected}");
    let second = Uuid::parse_str(two["fulfillment_id"].as_str().unwrap()).unwrap();
    db.begin_inventory_attempt(second, "test-link", false)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !db.claim_inventory_attempt_chat(&second.to_string(), crate::messages::MSG_ORDERS_CREATED)
            .await
            .unwrap()
    );
    assert!(db.claim_inventory_attempt_chat(&second.to_string(), crate::messages::MSG_TRADES_CREATED)
        .await.unwrap());
    assert!(!db.claim_inventory_attempt_chat(&second.to_string(), crate::messages::MSG_TRADES_CREATED)
        .await.unwrap());
    let inherited:Value=sqlx::query_scalar("SELECT details FROM fulfillment_audit_events WHERE redemption_id=$1 AND event_type='inventory_created'").bind(second).fetch_one(pool).await.unwrap();
    assert_eq!(
        inherited["script_execution_id"],
        attr.execution_id.to_string()
    );
    let second_item = Uuid::parse_str(two["inventory_item_id"].as_str().unwrap()).unwrap();
    assert!(!db.discard_script_item(second_item, "viewer").await.unwrap());
    let audit:Value=sqlx::query_scalar("SELECT details FROM fulfillment_audit_events WHERE redemption_id=$1 AND event_type='reward_redeemed'").bind(fulfillment).fetch_one(pool).await.unwrap();
    assert_eq!(audit["script_execution_id"], attr.execution_id.to_string());
    let operational: Vec<Value> = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
                let logs: Vec<Value> = sqlx::query_scalar("SELECT details FROM channel_logs WHERE broadcaster_id=$1 AND event_type='reward.script_trigger' AND details->>'fulfillment_id' IN ($2,$3)")
                .bind(&channel).bind(fulfillment.to_string()).bind(second.to_string()).fetch_all(pool).await.unwrap();
            if logs.len() == 2 { break logs; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    assert!(operational.iter().all(|l| l["actor_type"] == "script"
        && l["script"]["execution_id"] == attr.execution_id.to_string()));
    state.shutdown_token.cancel();
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn semantic_dispatch_isolated_projects_and_match_retention() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::from_env(db.clone()).await.unwrap();
    let pool = db.pool();
    let channel = format!("dispatch-test-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let mut ids = Vec::new();
    for (name, code, enabled) in [
        (
            "ordered",
            "fn on_event(ctx) { let seen=storage.get(\"seen\",[]); seen.push(ctx.event.kind); storage.set(\"seen\",seen); }",
            true,
        ),
        ("broken", "fn on_event(ctx) { loop {} }", true),
        ("disabled", "fn on_event(ctx) {}", false),
        ("timer only", "fn on_timer(ctx) {}", true),
    ] {
        let id = cmd(&state, &channel, json!({"action":"create","name":name}))
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        cmd(
            &state,
            &channel,
            json!({"action":"save","project_id":id,"version":1,"files":{"main.rhai":code}}),
        )
        .await
        .unwrap();
        cmd(
            &state,
            &channel,
            json!({"action":"publish","project_id":id,"version":2}),
        )
        .await
        .unwrap();
        cmd(
            &state,
            &channel,
            json!({"action":"enable","project_id":id,"enabled":enabled}),
        )
        .await
        .unwrap();
        ids.push(Uuid::parse_str(&id).unwrap());
    }
    let source = crate::cs2::model::Source {
        device_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        channel_id: channel.clone(),
        source_seq: 1,
        timestamp: chrono::Utc::now(),
    };
    let mut normalizer = crate::cs2::Normalizer::default();
    let mut payload = json!({"provider":{"appid":730,"steamid":"76561198000000001","timestamp":100},"map":{"name":"de_test","mode":"competitive","phase":"live","round":0,"team_ct":{"score":0},"team_t":{"score":0}},"round":{"phase":"live"},"player":{"steamid":"76561198000000001","team":"CT","activity":"playing","state":{"health":100,"armor":100,"round_kills":0},"match_stats":{"kills":0,"deaths":0,"assists":0,"score":0}}});
    let first = normalizer.apply(source.clone(), &payload);
    matches::persist(pool, &source, &first).await.unwrap();
    payload["player"]["state"]["health"] = json!(90);
    payload["player"]["state"]["round_kills"] = json!(1);
    payload["player"]["match_stats"]["kills"] = json!(1);
    payload["provider"]["timestamp"] = json!(101);
    let second_source = crate::cs2::model::Source {
        source_seq: 2,
        timestamp: source.timestamp + chrono::Duration::seconds(1),
        ..source.clone()
    };
    let second = normalizer.apply(second_source.clone(), &payload);
    assert!(second.events.len() >= 2);
    matches::persist(pool, &second_source, &second)
        .await
        .unwrap();
    let snapshot: Value =
        sqlx::query_scalar("SELECT context FROM script_snapshots WHERE channel_id=$1")
            .bind(&channel)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(snapshot["previous"]["player"]["health"], 100);
    assert!(snapshot["current_match"].is_object());
    assert!(snapshot.get("match").is_none());
    assert_eq!(snapshot["state"]["player"]["health"], 90);
    let snapshot_id: i64 =
        sqlx::query_scalar("SELECT id FROM script_snapshots WHERE channel_id=$1")
            .bind(&channel)
            .fetch_one(pool)
            .await
            .unwrap();
    let recorded = cmd(
        &state,
        &channel,
        json!({"action":"snapshot","project_id":ids[0],"snapshot_id":snapshot_id,"event_index":0}),
    )
    .await
    .unwrap();
    assert_eq!(recorded["previous"]["player"]["health"], 100);
    assert!(recorded.get("events").is_none());

    let counts:Vec<(Uuid,i64)>=sqlx::query_as("SELECT project_id,count(*) FROM script_executions WHERE project_id=ANY($1) GROUP BY project_id").bind(&ids).fetch_all(pool).await.unwrap();
    assert_eq!(counts.len(), 2);
    assert!(
        counts
            .iter()
            .all(|(p, n)| (*p == ids[0] || *p == ids[1]) && *n == second.events.len() as i64)
    );
    // One blocked project cannot stop a different project from taking work.
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM script_projects WHERE id=$1 FOR NO KEY UPDATE")
        .bind(ids[0])
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM script_executions WHERE project_id=$1 AND status='failed'"
        )
        .bind(ids[1])
        .fetch_one(pool)
        .await
        .unwrap(),
        1
    );
    lock.rollback().await.unwrap();
    for _ in 0..second.events.len() * 2 {
        worker::step(state.clone()).await.unwrap();
    }
    let seen: Value =
        sqlx::query_scalar("SELECT value FROM script_storage WHERE project_id=$1 AND key='seen'")
            .bind(ids[0])
            .fetch_one(pool)
            .await
            .unwrap();
    let expected: Vec<Value> = second
        .events
        .iter()
        .map(|e| json!(e.event)["kind"].clone())
        .collect();
    assert_eq!(seen, json!(expected));
    // Capture a reliably observed round ending using the same semantic normalizer.
    payload["round"]["phase"] = json!("over");
    payload["round"]["win_team"] = json!("CT");
    payload["map"]["round"] = json!(1);
    payload["map"]["team_ct"]["score"] = json!(1);
    payload["provider"]["timestamp"] = json!(102);
    let end_source = crate::cs2::model::Source {
        source_seq: 3,
        timestamp: source.timestamp + chrono::Duration::seconds(2),
        ..source.clone()
    };
    let end = normalizer.apply(end_source.clone(), &payload);
    matches::persist(pool, &end_source, &end).await.unwrap();
    let data: Value = sqlx::query_scalar(
        "SELECT data FROM script_matches WHERE channel_id=$1 AND completed_at IS NULL",
    )
    .bind(&channel)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(data["rounds"][0]["player"]["kills"], 1);
    assert_eq!(data["rounds"][0]["winner"], "ct");
    for n in 0..32 {
        sqlx::query("INSERT INTO script_matches(id,channel_id,device_id,session_id,map,data,completed_at) VALUES($1,$2,$3,$4,'old','{}',now()-$5*interval '1 hour')").bind(Uuid::new_v4()).bind(&channel).bind(source.device_id).bind(source.session_id).bind(n as f64).execute(pool).await.unwrap();
    }
    let tail_source = crate::cs2::model::Source {
        source_seq: 4,
        ..end_source
    };
    let tail = normalizer.apply(tail_source.clone(), &payload);
    matches::persist(pool, &tail_source, &tail).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM script_matches WHERE channel_id=$1 AND completed_at IS NOT NULL",
    )
    .bind(&channel)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count, 30);
    // Leave no executable queue behind for subsequent worker tests.
    sqlx::query("UPDATE script_executions SET status='skipped',finished_at=now() WHERE project_id=ANY($1) AND status='queued'").bind(ids).execute(pool).await.unwrap();
    state.shutdown_token.cancel();
}
