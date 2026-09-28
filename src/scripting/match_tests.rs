use super::matches;
use crate::cs2::{
    Normalizer,
    model::{EventKind, ResetReason, Source},
};
use serde_json::{Value, json};
use uuid::Uuid;

fn source(seq: i64) -> Source {
    Source {
        device_id: Uuid::from_u128(1),
        session_id: Uuid::from_u128(2),
        channel_id: "round-regression".into(),
        source_seq: seq,
        timestamp: chrono::DateTime::from_timestamp(1790596000 + seq, 0).unwrap(),
    }
}
fn payload(record: &Value, seq: i64) -> Value {
    json!({"provider":{"appid":730,"version":1,"steamid":"76561198000000001","timestamp":1790596000+seq},
        "map":{"name":"de_anubis","mode":"competitive","phase":record["phase"],"round":record["round"],"team_ct":{"score":record["ct"]},"team_t":{"score":record["t"]}},
        "round":{"phase":record["round_phase"]},
        "player":{"steamid":if record["spectator"]==true {"76561198000000002"}else{"76561198000000001"},"team":"CT","activity":"playing","state":{"health":100,"round_kills":0,"round_killhs":0}}})
}
#[test]
fn eight_continuous_rounds_survive_spectating_and_gameover_without_false_round_nine() {
    let records: Vec<Value> = serde_json::from_str(include_str!(
        "../cs2/fixtures/competitive-eight-rounds.json"
    ))
    .unwrap();
    let mut normalizer = Normalizer::default();
    let mut data = Value::Null;
    let mut ends = 0;
    for (i, record) in records.iter().enumerate() {
        let source = source(i as i64 + 1);
        let t = normalizer.apply(source.clone(), &payload(record, source.source_seq));
        if i == 1 {
            assert!(!t.resets.contains(&ResetReason::MatchRestarted));
        }
        ends += t
            .events
            .iter()
            .filter(|e| matches!(e.event, EventKind::RoundEnded))
            .count();
        data = matches::advance(data, &source, &t);
    }
    assert_eq!(ends, 8);
    assert_eq!(
        data["rounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
    assert!(data["current_round"].is_null());
    assert_ne!(data["partial"], true);
    assert!(data["rounds"][3]["player"]["kills"].is_null());
    assert_eq!(data["end_reason"], "game_over");
}
#[test]
fn last_live_round_can_end_in_the_same_payload_as_gameover() {
    let mut n = Normalizer::default();
    let first = json!({"round":7,"phase":"live","round_phase":"live","ct":3,"t":4});
    let s = source(1);
    let t = n.apply(s.clone(), &payload(&first, 1));
    let data = matches::advance(Value::Null, &s, &t);
    let last = json!({"round":8,"phase":"gameover","round_phase":"over","ct":3,"t":5});
    let s = source(2);
    let t = n.apply(s.clone(), &payload(&last, 2));
    assert!(
        t.events
            .iter()
            .any(|e| matches!(e.event, EventKind::RoundEnded))
    );
    let data = matches::advance(data, &s, &t);
    assert_eq!(data["rounds"][0]["index"], 7);
    assert!(data["current_round"].is_null());
}
#[test]
fn jumps_are_diagnosed_and_never_filled_with_invented_rounds() {
    let mut n = Normalizer::default();
    let first = json!({"round":2,"phase":"live","round_phase":"live","ct":1,"t":1});
    n.apply(source(1), &payload(&first, 1));
    let jump = json!({"round":8,"phase":"gameover","round_phase":"freezetime","ct":3,"t":5});
    let t = n.apply(source(2), &payload(&jump, 2));
    assert_eq!(
        crate::cs2::transition_issues(&t),
        [
            "completed_rounds_jump",
            "completed_rounds_without_round_end"
        ]
    );
    let data = matches::advance(Value::Null, &source(2), &t);
    assert!(data["rounds"].as_array().unwrap().is_empty());
    assert!(data["current_round"].is_null());
}

#[test]
fn score_update_after_round_end_refreshes_the_completed_round_without_duplicating_it() {
    let mut n = Normalizer::default();
    let mut data = Value::Null;
    for (i, record) in [
        json!({"round":0,"phase":"live","round_phase":"live","ct":0,"t":0}),
        json!({"round":0,"phase":"live","round_phase":"over","ct":0,"t":0}),
        json!({"round":1,"phase":"live","round_phase":"over","ct":1,"t":0}),
    ]
    .iter()
    .enumerate()
    {
        let s = source(i as i64 + 1);
        let t = n.apply(s.clone(), &payload(record, s.source_seq));
        data = matches::advance(data, &s, &t);
    }
    assert_eq!(data["rounds"].as_array().unwrap().len(), 1);
    assert_eq!(data["rounds"][0]["index"], 0);
    assert_eq!(data["rounds"][0]["score_after"]["ct"], 1);
    assert!(
        data["rounds"][0]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"]["kind"] == "score_changed")
    );
}
#[test]
fn audit_messages_identify_real_actions_without_generic_editor_noise() {
    let message = super::service::audit_message(
        "script.project_published",
        &json!({"project_name":"Round rewards","user_login":"owner","revision":3}),
    );
    assert_eq!(
        message,
        "@owner published script project \"Round rewards\" (revision 3)"
    );
}

#[test]
fn late_final_score_updates_the_last_completed_round_without_a_new_current_round() {
    let mut n = Normalizer::default();
    let mut data = Value::Null;
    for (i, record) in [
        json!({"round":0,"phase":"live","round_phase":"live","ct":0,"t":0}),
        json!({"round":1,"phase":"gameover","round_phase":"over","ct":0,"t":0}),
        json!({"round":1,"phase":"gameover","round_phase":"over","ct":1,"t":0}),
    ]
    .iter()
    .enumerate()
    {
        let s = source(i as i64 + 1);
        let t = n.apply(s.clone(), &payload(record, s.source_seq));
        data = matches::advance(data, &s, &t);
    }
    assert_eq!(data["rounds"].as_array().unwrap().len(), 1);
    assert_eq!(data["rounds"][0]["score_after"]["ct"], 1);
    assert!(data["current_round"].is_null());
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn persisted_eight_round_match_and_script_contexts_are_sequential_without_warmup_record() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pool = db.pool();
    let channel = format!("eight-round-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let mut records: Vec<Value> = serde_json::from_str(include_str!(
        "../cs2/fixtures/competitive-eight-rounds.json"
    ))
    .unwrap();
    // The score arrives one payload after game-over; the existing match must be refreshed.
    let final_score = records.last().unwrap().clone();
    let len = records.len();
    records[len - 3]["t"] = json!(4);
    records[len - 2]["t"] = json!(4);
    records[len - 1] = final_score;
    let mut normalizer = Normalizer::default();
    let device = Uuid::new_v4();
    let session = Uuid::new_v4();
    for (i, record) in records.iter().enumerate() {
        let mut s = source(i as i64 + 1);
        s.channel_id = channel.clone();
        s.device_id = device;
        s.session_id = session;
        let transition = normalizer.apply(s.clone(), &payload(record, s.source_seq));
        matches::persist(pool, &s, &transition).await.unwrap();
        if i == 0 {
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM script_matches WHERE channel_id=$1")
                    .bind(&channel)
                    .fetch_one(pool)
                    .await
                    .unwrap();
            assert_eq!(count, 0);
        }
    }
    let matches: Vec<(Value, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT data,completed_at FROM script_matches WHERE channel_id=$1")
            .bind(&channel)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].1.is_some());
    assert_eq!(matches[0].0["rounds"].as_array().unwrap().len(), 8);
    assert_eq!(matches[0].0["rounds"][7]["score_after"]["t"], 5);
    assert_eq!(matches[0].0["state"]["match"]["score"]["t"], 5);
    assert!(matches[0].0["current_round"].is_null());
    let contexts: Vec<Value> =
        sqlx::query_scalar("SELECT context FROM script_snapshots WHERE channel_id=$1 ORDER BY seq")
            .bind(&channel)
            .fetch_all(pool)
            .await
            .unwrap();
    let end_contexts: Vec<_> = contexts
        .iter()
        .filter(|c| {
            c["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "round_ended")
        })
        .collect();
    assert_eq!(end_contexts.len(), 8);
    for (i, c) in end_contexts.iter().enumerate() {
        assert_eq!(
            c["current_match"]["rounds"].as_array().unwrap().len(),
            i + 1
        );
        assert_eq!(c["current_match"]["rounds"][i]["index"], i);
        assert!(c["current_match"]["rounds"][i].get("events").is_none());
    }
    sqlx::query("DELETE FROM script_snapshots WHERE channel_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM script_matches WHERE channel_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM broadcasters WHERE channel_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE twitch_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
}

async fn owner_command(
    state: &std::sync::Arc<crate::state::AppState>,
    channel: &str,
    body: Value,
) -> Result<Value, crate::api::error::ApiError> {
    super::api::command(
        axum::extract::State(state.clone()),
        crate::api::extractor::authorized_channel::AuthorizedChannel {
            user_id: channel.into(),
            user_login: "owner".into(),
            channel_id: channel.into(),
            role: crate::db::channel_permissions::ChannelRole::Owner,
        },
        crate::api::extractor::json::JsonArg(serde_json::from_value(body).unwrap()),
    )
    .await
    .map(|result| result.0)
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn published_file_inspection_is_owner_scoped_and_only_real_lifecycle_actions_are_audited() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = crate::state::AppState::from_env(db.clone()).await.unwrap();
    let pool = db.pool();
    let channel = format!("audit-review-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let project = owner_command(
        &state,
        &channel,
        json!({"action":"create","name":"Round rewards"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let files = json!({"main.rhai":"fn on_timer(ctx) { log.info(ctx.timer.key); }"});
    owner_command(
        &state,
        &channel,
        json!({"action":"save","project_id":project,"version":1,"files":files}),
    )
    .await
    .unwrap();
    owner_command(
        &state,
        &channel,
        json!({"action":"publish","project_id":project,"version":2}),
    )
    .await
    .unwrap();
    assert_eq!(
        owner_command(
            &state,
            &channel,
            json!({"action":"revision","project_id":project,"revision":1})
        )
        .await
        .unwrap()["files"],
        files
    );
    assert!(
        owner_command(
            &state,
            "another-owner",
            json!({"action":"revision","project_id":project,"revision":1})
        )
        .await
        .is_err()
    );
    let overview = super::api::list(
        axum::extract::State(state.clone()),
        crate::api::extractor::authorized_channel::AuthorizedChannel {
            user_id: channel.clone(),
            user_login: "owner".into(),
            channel_id: channel.clone(),
            role: crate::db::channel_permissions::ChannelRole::Owner,
        },
    )
    .await
    .unwrap()
    .0;
    assert_eq!(overview["projects"][0]["live_files"], files);
    let attr = super::service::Attribution {
        project_id: Uuid::parse_str(&project).unwrap(),
        revision: 1,
        execution_id: Uuid::new_v4(),
        channel_id: channel.clone(),
    };
    let job = super::service::schedule(pool, &attr, "round-cleanup", 3600, json!({}))
        .await
        .unwrap();
    for _ in 0..2 {
        owner_command(
            &state,
            &channel,
            json!({"action":"cancel_job","project_id":project,"job_id":job["id"]}),
        )
        .await
        .unwrap();
    }
    state.shutdown_token.cancel();
    let logs: Vec<(String,String,Value)> = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let logs: Vec<(String,String,Value)> = sqlx::query_as("SELECT event_type,message,details FROM channel_logs WHERE broadcaster_id=$1 ORDER BY id").bind(&channel).fetch_all(pool).await.unwrap();
            if logs.len() >= 3 { break logs; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    assert_eq!(logs.len(), 3);
    assert_eq!(
        logs.iter().map(|log| log.0.as_str()).collect::<Vec<_>>(),
        [
            "script.project_created",
            "script.project_published",
            "script.job_cancelled"
        ]
    );
    assert!(
        logs.iter()
            .all(|log| log.1.contains("@owner") && log.1.contains("Round rewards"))
    );
    assert_eq!(logs[1].2["revision"], 1);
    assert_eq!(logs[2].2["job_key"], "round-cleanup");
    sqlx::query("UPDATE script_projects SET active_revision=NULL WHERE id=$1")
        .bind(attr.project_id)
        .execute(pool)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM script_editor_reports WHERE project_id=$1",
        "DELETE FROM script_jobs WHERE project_id=$1",
        "DELETE FROM script_revisions WHERE project_id=$1",
        "DELETE FROM script_projects WHERE id=$1",
    ] {
        sqlx::query(statement)
            .bind(attr.project_id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM broadcasters WHERE channel_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE twitch_id=$1")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
}
