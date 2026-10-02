use super::{api, limits, runtime::Files, service, worker};
use crate::{api::{extractor::{authorized_channel::AuthorizedChannel, json::JsonArg}, error::ApiError}, db::{Db, channel_permissions::ChannelRole}, state::AppState};
use axum::extract::State;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

async fn command(state: &Arc<AppState>, channel: &str, role: ChannelRole, body: Value) -> Result<Value, ApiError> {
    api::command(State(state.clone()), AuthorizedChannel {
        user_id: channel.into(), user_login: "limits-editor".into(), channel_id: channel.into(), role,
    }, JsonArg(serde_json::from_value(body).unwrap())).await.map(|v| v.0)
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn postgres_limits_persist_guard_permissions_and_apply_to_dry_run_event_and_pinned_timer() {
    let db = Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
    let state = AppState::from_env(db.clone()).await.unwrap();
    let pool = db.pool();
    let channel = format!("limits-test-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)").bind(&channel).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())")
        .bind(&channel).execute(pool).await.unwrap();
    let created = command(&state, &channel, ChannelRole::Owner, json!({"action":"create","name":"Time budgets"})).await.unwrap();
    let project = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let defaults = limits::load(pool, project, &channel).await.unwrap();
    assert_eq!(defaults.execution_timeout_secs, 30);
    assert_eq!(defaults.host_timeout_secs, 10);
    let settings = json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":5,"host_timeout_secs":4});
    command(&state, &channel, ChannelRole::Editor, settings.clone()).await.unwrap();
    assert!(command(&state, &channel, ChannelRole::Viewer, settings.clone()).await.is_err());
    assert!(command(&state, "other-channel", ChannelRole::Editor, settings).await.is_err());
    for (total, host) in [(0, 1), (121, 1), (30, 0), (120, 61), (3, 4)] {
        assert!(command(&state, &channel, ChannelRole::Owner, json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":total,"host_timeout_secs":host})).await.is_err());
    }
    assert!(serde_json::from_value::<api::Command>(json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":1.5,"host_timeout_secs":1})).is_err());
    assert!(sqlx::query("UPDATE script_projects SET host_timeout_secs=6 WHERE id=$1").bind(project).execute(pool).await.is_err());
    let files = Files::from([("main.rhai".into(), "fn on_event(ctx) { log.info(ctx.meta.execution_limits.execution_timeout_secs.to_string()); } fn on_timer(ctx) { log.info(ctx.meta.execution_limits.execution_timeout_secs.to_string()); }".into())]);
    command(&state, &channel, ChannelRole::Editor, json!({"action":"save","project_id":project,"version":1,"files":files})).await.unwrap();
    command(&state, &channel, ChannelRole::Editor, json!({"action":"publish","project_id":project,"version":2})).await.unwrap();
    command(&state, &channel, ChannelRole::Editor, json!({"action":"enable","project_id":project,"enabled":true})).await.unwrap();
    let dry = command(&state, &channel, ChannelRole::Editor, json!({"action":"test","project_id":project,"entry":"on_event","context":{}})).await.unwrap();
    assert!(dry["error"].is_null(), "{dry}");
    assert_eq!(dry["meta"]["execution_limits"]["host_timeout_secs"], 4);
    assert_eq!(dry["logs"][0]["message"], "5");
    let attr = service::Attribution { project_id: project, revision: 1, execution_id: Uuid::new_v4(), channel_id: channel.clone() };

    // Real database-backed host work exceeds both former fixed deadlines.
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,7))").bind(project.to_string()).execute(&mut *blocker).await.unwrap();
    let release = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(3200)).await;
        blocker.commit().await.unwrap();
    });
    let report = service::run(state.clone(), Files::from([("main.rhai".into(), "fn on_event(ctx) { storage.set(\"slow-call\", 42); log.info(\"finished\"); }".into())]),
        "on_event".into(), json!({}), attr.clone(), false).await;
    release.await.unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert!(report.duration_ms >= 3000, "{}", report.duration_ms);
    assert_eq!(report.meta["execution_limits"]["execution_timeout_secs"], 5);
    let stored: Value = sqlx::query_scalar("SELECT value FROM script_storage WHERE project_id=$1 AND key='slow-call'").bind(project).fetch_one(pool).await.unwrap();
    assert_eq!(stored, json!(42));

    command(&state, &channel, ChannelRole::Owner, json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":2,"host_timeout_secs":1})).await.unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,7))").bind(project.to_string()).execute(&mut *blocker).await.unwrap();
    let report = service::run(state.clone(), Files::from([("main.rhai".into(), "fn on_event(ctx) { storage.set(\"timed-out\", 1); }".into())]),
        "on_event".into(), json!({}), attr.clone(), false).await;
    assert!(report.error.as_ref().unwrap().contains("host_timeout"), "{:?}", report.error);
    blocker.commit().await.unwrap();
    assert!(!sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM script_storage WHERE project_id=$1 AND key='timed-out')").bind(project).fetch_one(pool).await.unwrap());

    let job = service::schedule(pool, &attr, "pinned-limits", 3600, json!({})).await.unwrap();
    command(&state, &channel, ChannelRole::Editor, json!({"action":"run_job","project_id":project,"job_id":job["id"],"confirmation":"RUN"})).await.unwrap();
    // Existing queued work uses limits at start, while keeping its immutable source revision.
    command(&state, &channel, ChannelRole::Editor, json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":7,"host_timeout_secs":3})).await.unwrap();
    let event_id = Uuid::new_v4();
    let snapshot: i64 = sqlx::query_scalar("INSERT INTO script_snapshots(channel_id,device_id,session_id,seq,context) VALUES($1,$2,$3,1,$4) RETURNING id")
        .bind(&channel).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(json!({"events":[{"kind":"score_changed"}]})).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,snapshot_id,event_index) VALUES($1,$2,1,'cs2',$3,0)")
        .bind(event_id).bind(project).bind(snapshot).execute(pool).await.unwrap();
    for _ in 0..20 {
        worker::step(state.clone()).await.unwrap();
        let done: bool = sqlx::query_scalar("SELECT status='success' FROM script_executions WHERE id=$1").bind(event_id).fetch_one(pool).await.unwrap();
        if done { break; }
    }
    let reports: Vec<Value> = sqlx::query_scalar("SELECT report FROM script_executions WHERE project_id=$1 ORDER BY sequence").bind(project).fetch_all(pool).await.unwrap();
    assert_eq!(reports.len(), 2);
    for report in &reports {
        assert!(report["error"].is_null(), "{report}");
        assert_eq!(report["meta"]["execution_limits"]["execution_timeout_secs"], 7);
        assert_eq!(report["meta"]["execution_limits"]["host_timeout_secs"], 3);
        assert_eq!(report["logs"][0]["message"], "7");
    }
    command(&state, &channel, ChannelRole::Editor, json!({"action":"rollback","project_id":project,"revision":1})).await.unwrap();
    assert_eq!(limits::load(pool, project, &channel).await.unwrap().execution_timeout_secs, 7);
    command(&state, &channel, ChannelRole::Editor, json!({"action":"execution_limits","project_id":project,"execution_timeout_secs":30,"host_timeout_secs":10})).await.unwrap();
    let persisted: Value = sqlx::query_scalar("SELECT report FROM script_executions WHERE id=$1").bind(event_id).fetch_one(pool).await.unwrap();
    assert_eq!(persisted["meta"]["execution_limits"]["execution_timeout_secs"], 7);
}
