use super::{api, matches, worker};
use crate::cs2::{
    Normalizer,
    model::{EventKind, Source},
};
use serde_json::{Value, json};
use uuid::Uuid;

fn records() -> Vec<Value> {
    serde_json::from_str(include_str!("../cs2/fixtures/live-vertigo-20260928.json")).unwrap()
}
fn source(record: &Value) -> Source {
    Source {
        device_id: Uuid::parse_str("3c8e2b50-4832-4848-9556-1133c5efbeea").unwrap(),
        session_id: Uuid::parse_str("98763c39-464b-4d8e-9b57-36df3500542d").unwrap(),
        channel_id: "847261392".into(),
        source_seq: record["seq"].as_i64().unwrap(),
        timestamp: record["received_at"].as_str().unwrap().parse().unwrap(),
    }
}
fn replay(records: &[Value]) -> Value {
    let mut normalizer = Normalizer::default();
    let mut data = Value::Null;
    for record in records {
        let s = source(record);
        let t = normalizer.apply(s.clone(), &record["payload"]);
        if t.current.r#match.is_some() {
            data = matches::advance(data, &s, &t);
        }
    }
    data
}
fn assert_match(data: &Value) {
    let rounds = data["rounds"].as_array().unwrap();
    assert_eq!(rounds.len(), 15);
    let kills = [2, 0, 0, 1, 0, 0, 0, 2, 1, 0, 1, 0, 1, 0, 0];
    let hs = [2, 0, 0, 0, 0, 0, 0, 2, 1, 0, 1, 0, 0, 0, 0];
    let before = [
        (0, 0),
        (1, 0),
        (1, 1),
        (1, 2),
        (1, 3),
        (1, 4),
        (1, 5),
        (1, 6),
        (6, 2),
        (6, 3),
        (6, 4),
        (6, 5),
        (7, 5),
        (7, 6),
        (8, 6),
    ];
    let after = [
        (1, 0),
        (1, 1),
        (1, 2),
        (1, 3),
        (1, 4),
        (1, 5),
        (1, 6),
        (2, 6),
        (6, 3),
        (6, 4),
        (6, 5),
        (7, 5),
        (7, 6),
        (8, 6),
        (9, 6),
    ];
    for (i, round) in rounds.iter().enumerate() {
        assert_eq!(round["index"], i);
        assert_eq!(round["player"]["kills"], kills[i], "round {}", i + 1);
        assert_eq!(round["player"]["headshot_kills"], hs[i], "round {}", i + 1);
        assert_eq!(
            round["score_before"],
            json!({"ct":before[i].0,"t":before[i].1})
        );
        assert_eq!(
            round["score_after"],
            json!({"ct":after[i].0,"t":after[i].1})
        );
    }
    assert_eq!(rounds[8]["side_swap_before"]["seq"], 231);
    assert!(data["current_round"].is_null());
    assert_ne!(data["partial"], true);
    assert_eq!(
        data["local_summary"]["stats"],
        json!({"kills":8,"assists":2,"deaths":11,"mvps":5,"score":26})
    );
    assert_eq!(data["local_summary"]["final"], true);
}

#[test]
fn live_continuous_delivery_retains_fifteen_boundaries_eight_kills_and_eleven_deaths() {
    let records = records();
    assert_eq!(records.len(), 454);
    let mut n = Normalizer::default();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut kills = Vec::new();
    let mut deaths = Vec::new();
    for (i, record) in records.iter().enumerate() {
        let s = source(record);
        assert_eq!(s.source_seq, 8 + i as i64);
        let t = n.apply(s.clone(), &record["payload"]);
        if record["payload"]["player"]["steamid"] != record["payload"]["provider"]["steamid"] {
            assert!(t.current.player.is_none());
        }
        assert!(crate::cs2::transition_issues(&t).is_empty());
        for e in &t.events {
            match e.event {
                EventKind::RoundStarted => starts.push(s.source_seq),
                EventKind::RoundEnded => ends.push(s.source_seq),
                EventKind::PlayerKill { count, .. } => {
                    assert_eq!(count, 1);
                    kills.push(s.source_seq);
                }
                EventKind::PlayerDied { .. } => deaths.push(s.source_seq),
                _ => {}
            }
        }
    }
    assert_eq!(
        starts,
        [
            22, 54, 74, 97, 145, 157, 183, 213, 235, 286, 308, 348, 395, 428, 441
        ]
    );
    assert_eq!(
        ends,
        [
            48, 64, 85, 133, 151, 179, 202, 226, 275, 299, 333, 385, 417, 435, 458
        ]
    );
    assert_eq!(kills, [41, 48, 118, 224, 226, 275, 321, 403]);
    assert_eq!(
        deaths,
        [63, 85, 133, 150, 179, 202, 334, 356, 410, 435, 458]
    );
    assert_match(&replay(&records));
}

#[test]
fn live_final_freezetime_completion_is_exactly_once_and_late_updates_stay_on_round_fifteen() {
    let records = records();
    let mut data = replay(&records[..records.iter().position(|r| r["seq"] == 459).unwrap()]);
    assert_match(&data);
    let mut n = Normalizer::default();
    for record in records
        .iter()
        .filter(|r| r["seq"].as_i64().unwrap() >= 457 && r["seq"].as_i64().unwrap() <= 459)
    {
        let s = source(record);
        let t = n.apply(s.clone(), &record["payload"]);
        if record["seq"] == 459 {
            assert!(
                !t.events
                    .iter()
                    .any(|e| matches!(e.event, EventKind::RoundEnded))
            );
            data = matches::advance(data, &s, &t);
        }
    }
    assert_match(&data);
}

#[test]
fn totals_are_last_observed_when_gameover_views_a_teammate() {
    let mut records = records();
    for r in records
        .iter_mut()
        .filter(|r| r["payload"]["map"]["phase"] == "gameover")
    {
        r["payload"]["player"]["steamid"] = json!("76561199407039520");
        r["payload"]["player"]["match_stats"]["kills"] = json!(99);
    }
    let data = replay(&records);
    assert_eq!(data["local_summary"]["stats"]["kills"], 8);
    assert_eq!(data["local_summary"]["stats"]["deaths"], 10);
    assert_eq!(data["local_summary"]["seq"], 457);
    assert_eq!(data["local_summary"]["final"], false);
}

#[test]
fn sparse_local_and_spectator_payloads_keep_known_values_but_new_unobserved_round_stays_unknown() {
    let base = records().into_iter().find(|r| r["seq"] == 403).unwrap();
    let mut n = Normalizer::default();
    let mut data = Value::Null;
    for (i, mut r) in (0..6).map(|i| (i, base.clone())) {
        r["seq"] = json!(403 + i);
        r["payload"]["provider"]["timestamp"] = json!(1790625450 + i);
        if i == 1 {
            r["payload"]["player"]["state"] = json!({});
        }
        if i >= 2 {
            r["payload"]["player"]["steamid"] = json!("76561199407039520");
            r["payload"]["player"]["state"] = json!({"round_kills":99,"round_killhs":99});
        }
        if i == 2 {
            r["payload"]["map"]["round"] = json!(13);
            r["payload"]["round"]["phase"] = json!("over");
        }
        if i == 3 {
            r["payload"]["map"]["round"] = json!(13);
            r["payload"]["round"]["phase"] = json!("freezetime");
        }
        if i == 4 {
            r["payload"]["map"]["round"] = json!(13);
        }
        if i == 5 {
            r["payload"]["map"]["round"] = json!(14);
            r["payload"]["round"]["phase"] = json!("over");
        }
        let s = source(&r);
        let t = n.apply(s.clone(), &r["payload"]);
        data = matches::advance(data, &s, &t);
    }
    assert_eq!(data["rounds"][0]["player"]["kills"], 1);
    assert_eq!(data["rounds"][0]["player"]["headshot_kills"], 0);
    assert!(data["rounds"][1]["player"]["kills"].is_null());
    assert!(data["rounds"][1]["player"]["headshot_kills"].is_null());
}

#[test]
fn final_freeze_reset_cannot_erase_known_round_stats_or_invent_a_missing_baseline() {
    let mut observed = records();
    for r in observed
        .iter_mut()
        .filter(|r| r["seq"].as_i64().unwrap() > 435 && r["seq"].as_i64().unwrap() < 458)
    {
        r["payload"]["player"]["state"]["round_kills"] = json!(1);
        r["payload"]["player"]["state"]["round_killhs"] = json!(1);
    }
    let data = replay(&observed);
    assert_eq!(data["rounds"][14]["player"]["kills"], 1);
    assert_eq!(data["rounds"][14]["player"]["headshot_kills"], 1);
    let mut unobserved = records();
    for r in unobserved
        .iter_mut()
        .filter(|r| r["seq"].as_i64().unwrap() > 435 && r["seq"].as_i64().unwrap() < 458)
    {
        r["payload"]["player"]["steamid"] = json!("76561199407039520");
    }
    let data = replay(&unobserved);
    assert!(data["rounds"][14]["player"]["kills"].is_null());
    assert!(data["rounds"][14]["player"]["headshot_kills"].is_null());
    assert_eq!(data["local_summary"]["final"], true);
}

#[test]
fn unresolved_final_counter_jump_does_not_overwrite_an_earlier_completed_round() {
    let mut records = records();
    let pos = records.iter().position(|r| r["seq"] == 458).unwrap();
    records[pos]["payload"]["map"]["round"] = json!(16);
    records[pos]["payload"]["map"]["team_ct"]["score"] = json!(10);
    let data = replay(&records[..=pos]);
    assert_eq!(data["rounds"].as_array().unwrap().len(), 14);
    assert_eq!(data["rounds"][13]["score_after"], json!({"ct":8,"t":6}));
    assert_eq!(data["partial"], true);
}

async fn command(
    state: &std::sync::Arc<crate::state::AppState>,
    channel: &str,
    body: Value,
) -> Value {
    api::command(
        axum::extract::State(state.clone()),
        auth(channel),
        crate::api::extractor::json::JsonArg(serde_json::from_value(body).unwrap()),
    )
    .await
    .unwrap()
    .0
}
fn auth(channel: &str) -> crate::api::extractor::authorized_channel::AuthorizedChannel {
    crate::api::extractor::authorized_channel::AuthorizedChannel {
        user_id: channel.into(),
        user_login: "owner".into(),
        channel_id: channel.into(),
        role: crate::db::channel_permissions::ChannelRole::Owner,
    }
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn live_postgres_match_dispatch_search_and_audit_policy() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = crate::state::AppState::from_env(db.clone()).await.unwrap();
    let pool = db.pool();
    let channel = format!("live-vertigo-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let project = command(
        &state,
        &channel,
        json!({"action":"create","name":"Live evidence"}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let project_id = Uuid::parse_str(&project).unwrap();
    command(&state,&channel,json!({"action":"save","project_id":project,"version":1,"files":{"main.rhai":"fn on_event(ctx) { log.info(ctx.event.kind); if ctx.event.kind == \"player_kill\" { storage.set(\"kills\", storage.get(\"kills\", 0) + ctx.event.count); } }"}})).await;
    command(
        &state,
        &channel,
        json!({"action":"publish","project_id":project,"version":2}),
    )
    .await;
    command(
        &state,
        &channel,
        json!({"action":"enable","project_id":project,"enabled":true}),
    )
    .await;
    let device = Uuid::new_v4();
    let session = Uuid::new_v4();
    let mut n = Normalizer::default();
    for record in records() {
        let mut s = source(&record);
        s.channel_id = channel.clone();
        s.device_id = device;
        s.session_id = session;
        let t = n.apply(s.clone(), &record["payload"]);
        matches::persist(pool, &s, &t).await.unwrap();
    }
    let data: Value = sqlx::query_scalar("SELECT data FROM script_matches WHERE channel_id=$1")
        .bind(&channel)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_match(&data);
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM script_executions WHERE project_id=$1 AND status='queued'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .unwrap();
    for _ in 0..pending {
        assert!(worker::step(state.clone()).await.unwrap());
    }
    let kill_count: Value =
        sqlx::query_scalar("SELECT value FROM script_storage WHERE project_id=$1 AND key='kills'")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(kill_count, 8);
    let successful: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM script_executions WHERE project_id=$1 AND status='success'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(successful, pending);
    // Newer non-kill rows push all eight kills outside the old client-side search window.
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,status,report,finished_at) SELECT gen_random_uuid(),$1,1,'cs2','success','{\"logs\":[{\"level\":\"info\",\"message\":\"ammo_changed\"}]}',now() FROM generate_series(1,300)").bind(project_id).execute(pool).await.unwrap();
    let overview = api::list(
        axum::extract::State(state.clone()),
        auth(&channel),
        axum::extract::Query(api::ExecutionSearch {
            execution_search: "player_kill".into(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(overview["executions"].as_array().unwrap().len(), 8);
    assert!(
        overview["executions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"] == "success" && e["event"]["kind"] == "player_kill")
    );
    candidate_snapshot_race(pool, project_id).await;
    command(&state,&channel,json!({"action":"save","project_id":project,"version":2,"files":{"main.rhai":"fn on_event(ctx) { throw \"expected QA failure\"; }"}})).await;
    command(
        &state,
        &channel,
        json!({"action":"publish","project_id":project,"version":3}),
    )
    .await;
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,snapshot_id,event_index) SELECT gen_random_uuid(),$1,2,'cs2',id,0 FROM script_snapshots WHERE channel_id=$2 ORDER BY id LIMIT 1").bind(project_id).bind(&channel).execute(pool).await.unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,status) VALUES(gen_random_uuid(),$1,1,'cs2','running')").bind(project_id).execute(pool).await.unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    let logs: Vec<Value> = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let logs: Vec<Value> = sqlx::query_scalar(
                "SELECT to_jsonb(l) FROM channel_logs l WHERE broadcaster_id=$1 ORDER BY id DESC",
            )
            .bind(&channel)
            .fetch_all(pool)
            .await
            .unwrap();
            if logs
                .iter()
                .any(|l| l["event_type"] == "script.execution_interrupted")
            {
                break logs;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(!logs.iter().any(|l| l["event_type"] == "script.execution"));
    for event in [
        "script.project_created",
        "script.project_published",
        "script.project_enabled",
    ] {
        assert!(
            logs.iter().any(|l| l["event_type"] == event),
            "missing {event}"
        );
    }
    for event in ["script.execution_failed", "script.execution_interrupted"] {
        let log = logs.iter().find(|l| l["event_type"] == event).unwrap();
        assert_eq!(log["level"], "WARN");
        assert!(!log["details"]["actor_type"].is_null());
    }
    if let Ok(dir) = std::env::var("LIVE_GSI_QA_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("verified-overview.json"),
            serde_json::to_vec_pretty(&overview).unwrap(),
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("verified-operational-logs.json"),
            serde_json::to_vec_pretty(&logs).unwrap(),
        )
        .unwrap();
    }
    state.shutdown_token.cancel();
}

async fn candidate_snapshot_race(pool: &sqlx::PgPool, project: Uuid) {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO script_executions(id,project_id,revision,source) VALUES($1,$2,1,'cs2')",
    )
    .bind(id)
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    let mut drain = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM script_projects WHERE id=$1 FOR NO KEY UPDATE")
        .bind(project)
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    sqlx::query("UPDATE script_executions SET status='success' WHERE id=$1")
        .bind(id)
        .execute(&mut *drain)
        .await
        .unwrap();
    let mut gate = pool.acquire().await.unwrap();
    let key = 190928;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key as i64)
        .execute(&mut *gate)
        .await
        .unwrap();
    // Pause the real candidate statement after its MVCC snapshot, before its project lock.
    let query = worker::PROJECT_CANDIDATE_SQL
        .replacen(
            "WHERE EXISTS",
            &format!("WHERE p.id='{project}' AND EXISTS"),
            1,
        )
        .replacen(
            "ORDER BY ",
            &format!("ORDER BY (SELECT pg_advisory_xact_lock({key}))::text, "),
            1,
        );
    let (pid_tx, pid_rx) = tokio::sync::oneshot::channel();
    let candidate_pool = pool.clone();
    let candidate = tokio::spawn(async move {
        let mut tx = candidate_pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        pid_tx.send(pid).unwrap();
        // Only the fixed production SQL, a typed UUID and a constant lock key enter this query.
        let row: Option<(Uuid, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(query.as_str()))
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
        assert_eq!(row.unwrap().0, project);
        let old = sqlx::query("SELECT id FROM script_executions WHERE project_id=$1 AND status IN ('queued','running') ORDER BY sequence LIMIT 1").bind(project).fetch_one(&mut *tx).await;
        assert!(matches!(old, Err(sqlx::Error::RowNotFound)));
        assert!(
            worker::pending_execution(&mut tx, project)
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    });
    let pid = pid_rx.await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND locktype='advisory' AND NOT granted)").bind(pid).fetch_one(pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    drain.commit().await.unwrap();
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(key as i64)
        .execute(&mut *gate)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), candidate)
        .await
        .unwrap()
        .unwrap();
}
