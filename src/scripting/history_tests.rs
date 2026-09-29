use super::{
    history::{self, Filter},
    service::{self, Attribution},
    worker,
};
use crate::{
    api::extractor::{authorized_channel::AuthorizedChannel, json::JsonArg},
    db::channel_permissions::ChannelRole,
    state::AppState,
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

async fn setup() -> (Arc<AppState>, String, Uuid) {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let channel = format!("history-{}", Uuid::new_v4());
    let pool = db.pool();
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(pool).await.unwrap();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO script_projects(id,channel_id,name,enabled) VALUES($1,$2,'History fixture',true)").bind(project).bind(&channel).execute(pool).await.unwrap();
    for (revision, code) in [
        (1i64, "fn on_timer(ctx) { log.info(ctx.timer.key); }"),
        (2, "fn on_timer(ctx) { throw \"expected timer failure\"; }"),
    ] {
        sqlx::query("INSERT INTO script_revisions(project_id,revision,files,has_on_event,has_on_timer) VALUES($1,$2,$3,false,true)").bind(project).bind(revision).bind(json!({"main.rhai":code})).execute(pool).await.unwrap();
    }
    (AppState::from_env(db).await.unwrap(), channel, project)
}
async fn execution(
    pool: &sqlx::PgPool,
    p: Uuid,
    source: &str,
    status: &str,
    report: Value,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,status,report,created_at,finished_at) VALUES($1,$2,1,$3,$4,$5,now()-interval '10 minutes',now())")
        .bind(id).bind(p).bind(source).bind(status).bind(report).execute(pool).await.unwrap();
    id
}
async fn noise(pool: &sqlx::PgPool, p: Uuid, count: i32) {
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,status,report,finished_at) SELECT gen_random_uuid(),$1,1,'cs2','success','{\"logs\":[],\"actions\":[],\"error\":null}',now() FROM generate_series(1,$2)")
        .bind(p).bind(count).execute(pool).await.unwrap();
}
async fn fetch(state: &AppState, channel: &str, f: Filter) -> Value {
    f.validate().unwrap();
    history::page(state.db.pool(), channel, &f).await.unwrap()
}
fn all() -> Filter {
    Filter {
        mode: "all".into(),
        ..Default::default()
    }
}
fn ids(page: &Value) -> Vec<String> {
    page["executions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn history_filter_validation_is_bounded_and_explicit() {
    assert!(
        serde_json::from_value::<Filter>(json!({}))
            .unwrap()
            .validate()
            .is_ok()
    );
    for value in [
        json!({"mode":"invalid"}),
        json!({"level":"critical"}),
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"cursor":"broken"}),
        json!({"search":"x".repeat(129)}),
    ] {
        assert!(
            serde_json::from_value::<Filter>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn retained_history_filters_precede_limit_and_default_excludes_cs2_noise() {
    let (state, channel, p) = setup().await;
    let pool = state.db.pool();
    let warn = execution(pool,p,"cs2","success",json!({"logs":[{"level":"warn","message":"player_kill warning"}],"actions":[],"error":null})).await;
    let fail = execution(
        pool,
        p,
        "timer",
        "failed",
        json!({"logs":[],"actions":[],"error":"old timer failure"}),
    )
    .await;
    let host = execution(pool,p,"cs2","success",json!({"actions":[{"method":"rewards.trigger","result":{"ok":false,"code":"reward_paused"}}]})).await;
    let read = execution(
        pool,
        p,
        "cs2",
        "success",
        json!({"actions":[{"method":"storage.get","error":null}]}),
    )
    .await;
    noise(pool, p, 350).await;
    let f = Filter {
        level: "warn".into(),
        ..all()
    };
    assert_eq!(ids(&fetch(&state, &channel, f).await), [warn.to_string()]);
    let f = Filter {
        project_id: Some(p),
        status: "failed".into(),
        source: "timer".into(),
        level: "error".into(),
        ..all()
    };
    assert_eq!(ids(&fetch(&state, &channel, f).await), [fail.to_string()]);
    assert_eq!(
        ids(&fetch(
            &state,
            &channel,
            Filter {
                search: "player_kill".into(),
                ..all()
            }
        )
        .await),
        [warn.to_string()]
    );
    let output = fetch(
        &state,
        &channel,
        Filter {
            mode: "noteworthy".into(),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(output["total"], 3);
    assert!(ids(&output).contains(&host.to_string()));
    assert!(!ids(&output).contains(&read.to_string()));
    let full = fetch(&state, &channel, all()).await;
    assert_eq!(full["total"], 354);
    assert_eq!(full["executions"].as_array().unwrap().len(), 50);
    let other = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO script_projects(id,channel_id,name) VALUES($1,$2,'Other history project')",
    )
    .bind(other)
    .bind(&channel)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO script_revisions(project_id,revision,files,has_on_event,has_on_timer) VALUES($1,1,'{}',false,false)").bind(other).execute(pool).await.unwrap();
    noise(pool, other, 350).await;
    let project_only = fetch(
        &state,
        &channel,
        Filter {
            project_id: Some(p),
            ..all()
        },
    )
    .await;
    assert_eq!(project_only["total"], 354);
    assert_eq!(project_only["executions"].as_array().unwrap().len(), 50);
    assert!(
        project_only["executions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["project_id"] == p.to_string())
    );
    assert_eq!(
        ids(&fetch(
            &state,
            &channel,
            Filter {
                status: "failed".into(),
                ..all()
            }
        )
        .await),
        [fail.to_string()]
    );
    assert_eq!(
        ids(&fetch(
            &state,
            &channel,
            Filter {
                source: "timer".into(),
                ..all()
            }
        )
        .await),
        [fail.to_string()]
    );
    assert_eq!(fetch(&state, "unrelated", all()).await["total"], 0);
    assert_eq!(
        fetch(
            &state,
            "unrelated",
            Filter {
                job_id: Some(fail),
                ..all()
            }
        )
        .await["total"],
        0
    );
    assert_eq!(
        fetch(
            &state,
            &channel,
            Filter {
                search: "' OR true --".into(),
                ..all()
            }
        )
        .await["total"],
        0
    );
    state.shutdown_token.cancel();
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn union_cursor_history_is_stable_across_ties_and_new_insertions() {
    let (state, channel, p) = setup().await;
    let pool = state.db.pool();
    let time = chrono::Utc::now() - chrono::Duration::seconds(1);
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,status,report,created_at,finished_at) SELECT gen_random_uuid(),$1,1,'cs2','success','{}',$2,now() FROM generate_series(1,230)")
        .bind(p).bind(time).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO script_editor_reports(id,project_id,source,status,report,created_at) SELECT gen_random_uuid(),$1,'validate','success','{}',$2 FROM generate_series(1,30)")
        .bind(p).bind(time).execute(pool).await.unwrap();
    let mut f = Filter {
        limit: Some(37),
        ..all()
    };
    let mut found = HashSet::new();
    let first = fetch(&state, &channel, f.clone()).await;
    let mut result = first.clone();
    noise(pool, p, 1).await;
    let anchored_first = fetch(
        &state,
        &channel,
        Filter {
            cursor: Some(first["start_cursor"].as_str().unwrap().into()),
            ..f.clone()
        },
    )
    .await;
    assert_eq!(ids(&anchored_first), ids(&first));
    assert_eq!(anchored_first["total"], 260);
    loop {
        assert_eq!(result["total"], 260);
        for id in ids(&result) {
            assert!(found.insert(id), "duplicate cursor row");
        }
        match result["next_cursor"].as_str() {
            Some(cursor) => {
                f.cursor = Some(cursor.into());
                result = fetch(&state, &channel, f.clone()).await;
            }
            None => break,
        }
    }
    assert_eq!(found.len(), 260);
    assert_eq!(fetch(&state, &channel, all()).await["total"], 261);
    f.level = "warn".into();
    assert!(
        f.validate().is_err(),
        "cursor must not silently reuse different filters"
    );
    state.shutdown_token.cancel();
}

fn attr(p: Uuid, channel: &str, revision: i64) -> Attribution {
    Attribution {
        project_id: p,
        channel_id: channel.into(),
        revision,
        execution_id: Uuid::new_v4(),
    }
}
async fn schedule(state: &AppState, channel: &str, p: Uuid, revision: i64) -> Uuid {
    let job = service::schedule(
        state.db.pool(),
        &attr(p, channel, revision),
        "ace_giveaway",
        1,
        json!({}),
    )
    .await
    .unwrap();
    let id = Uuid::parse_str(job["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE script_jobs SET scheduled_for=now()-interval '1 second' WHERE id=$1")
        .bind(id)
        .execute(state.db.pool())
        .await
        .unwrap();
    id
}
async fn result(state: &AppState, channel: &str, id: Uuid) -> Value {
    history::jobs(state.db.pool(), channel)
        .await
        .unwrap()
        .into_iter()
        .find(|j| j["id"] == id.to_string())
        .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn same_key_jobs_keep_exact_success_failure_revision_and_terminal_history_after_noise() {
    let (state, channel, p) = setup().await;
    let mut expected = Vec::new();
    for revision in [1, 1, 2, 1, 2, 1] {
        let job = schedule(&state, &channel, p, revision).await;
        assert!(worker::step(state.clone()).await.unwrap());
        let status = if revision == 1 { "success" } else { "failed" };
        expected.push((job, revision, status));
        noise(state.db.pool(), p, 250).await;
        for (id, rev, status) in &expected {
            let row = result(&state, &channel, *id).await;
            assert_eq!(row["last_execution"]["status"], *status);
            assert_eq!(row["revision"], *rev);
            let history = fetch(
                &state,
                &channel,
                Filter {
                    job_id: Some(*id),
                    ..all()
                },
            )
            .await;
            assert_eq!(history["total"], 1);
            assert_eq!(history["executions"][0]["job_id"], id.to_string());
            assert_eq!(history["executions"][0]["status"], *status);
        }
    }
    let cancelled = schedule(&state, &channel, p, 1).await;
    sqlx::query("UPDATE script_jobs SET status='cancelled',completed_at=now() WHERE id=$1")
        .bind(cancelled)
        .execute(state.db.pool())
        .await
        .unwrap();
    let blocked = schedule(&state, &channel, p, 2).await;
    sqlx::query("UPDATE script_jobs SET status='blocked',reason='project_disabled' WHERE id=$1")
        .bind(blocked)
        .execute(state.db.pool())
        .await
        .unwrap();
    assert!(result(&state, &channel, cancelled).await["last_execution"].is_null());
    assert!(result(&state, &channel, blocked).await["last_execution"].is_null());
    for (id, _, status) in expected {
        assert_eq!(
            result(&state, &channel, id).await["last_execution"]["status"],
            status
        );
    }
    state.shutdown_token.cancel();
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn skipped_admission_keeps_job_id_and_manual_run_is_not_a_replay() {
    let (state, channel, p) = setup().await;
    let job = schedule(&state, &channel, p, 1).await;
    sqlx::query("UPDATE script_jobs SET status='queued' WHERE id=$1")
        .bind(job)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE script_projects SET enabled=false WHERE id=$1")
        .bind(p)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,job_id) VALUES(gen_random_uuid(),$1,1,'timer',$2)").bind(p).bind(job).execute(state.db.pool()).await.unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    assert_eq!(
        result(&state, &channel, job).await["last_execution"]["status"],
        "skipped"
    );
    let run = || {
        super::api::command(
            axum::extract::State(state.clone()),
            AuthorizedChannel {
                user_id: channel.clone(),
                user_login: "owner".into(),
                channel_id: channel.clone(),
                role: ChannelRole::Owner,
            },
            JsonArg(
                serde_json::from_value(
                    json!({"action":"run_job","project_id":p,"job_id":job,"confirmation":"RUN"}),
                )
                .unwrap(),
            ),
        )
    };
    let _ = run().await.unwrap();
    assert!(worker::step(state.clone()).await.unwrap());
    assert_eq!(
        result(&state, &channel, job).await["last_execution"]["status"],
        "success"
    );
    let history = fetch(
        &state,
        &channel,
        Filter {
            job_id: Some(job),
            ..all()
        },
    )
    .await;
    assert_eq!(history["total"], 2);
    assert_eq!(history["executions"][0]["status"], "success");
    assert_eq!(history["executions"][1]["status"], "skipped");
    assert!(
        run().await.is_err(),
        "completed job cannot be manually replayed"
    );
    state.shutdown_token.cancel();
}
