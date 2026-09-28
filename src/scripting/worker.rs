use super::{
    runtime::Files,
    service::{self, Attribution},
};
use crate::state::AppState;
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

pub fn start(state: Arc<AppState>) {
    let cleanup = state.clone();
    state.spawn_task(async move {
        let mut tick=tokio::time::interval(std::time::Duration::from_secs(3600));
        loop { tokio::select! {
            _=cleanup.shutdown_token.cancelled()=>break,
            _=tick.tick()=> {
                if let Err(error)=prune(cleanup.db.pool()).await {tracing::error!(%error,"Script history cleanup failed");}
            }
        } }
    });
    for _ in 0..2 {
        let worker = state.clone();
        state.spawn_task(async move {
            loop {
                if worker.shutdown_token.is_cancelled() { break; }
                match step(worker.clone()).await {
                    Ok(true)=>continue,
                    Err(error)=>tracing::error!(%error,"Scripting worker failed"),
                    _=>{}
                }
                tokio::select! { _=worker.shutdown_token.cancelled()=>break, _=tokio::time::sleep(std::time::Duration::from_millis(500))=>{} }
            }
        });
    }
}
async fn prune(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM script_editor_reports WHERE id IN (SELECT id FROM script_editor_reports WHERE created_at<now()-interval '30 days' ORDER BY created_at LIMIT 1000)").execute(pool).await?;
    // Small batches avoid long-running deletion transactions during gameplay.
    sqlx::query("DELETE FROM script_executions WHERE id IN (SELECT id FROM script_executions WHERE status NOT IN ('queued','running') AND finished_at<now()-interval '30 days' ORDER BY finished_at LIMIT 1000)").execute(pool).await?;
    sqlx::query("DELETE FROM script_jobs WHERE id IN (SELECT j.id FROM script_jobs j WHERE j.status IN ('completed','cancelled','failed') AND j.completed_at<now()-interval '30 days' AND NOT EXISTS(SELECT 1 FROM script_executions e WHERE e.job_id=j.id) ORDER BY j.completed_at LIMIT 1000)").execute(pool).await?;
    sqlx::query("DELETE FROM script_snapshots WHERE id IN (SELECT s.id FROM script_snapshots s WHERE s.created_at<now()-interval '7 days' AND NOT EXISTS(SELECT 1 FROM script_executions e WHERE e.snapshot_id=s.id) ORDER BY s.id LIMIT 1000)").execute(pool).await?;
    Ok(())
}
pub async fn block_expired(tx: &mut sqlx::PgConnection, project: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE script_jobs SET status='blocked',reason='project_disabled' WHERE project_id=$1 AND status='scheduled' AND scheduled_for<=now()")
        .bind(project).execute(tx).await?;
    Ok(())
}

async fn timers(state: &AppState) -> Result<(), sqlx::Error> {
    let mut tx = state.db.pool().begin().await?;
    let p:Option<(Uuid,bool)>=sqlx::query_as("SELECT p.id,p.enabled FROM script_projects p WHERE p.deleted_at IS NULL AND EXISTS(SELECT 1 FROM script_jobs j WHERE j.project_id=p.id AND j.status='scheduled' AND j.scheduled_for<=now()) ORDER BY p.id LIMIT 1 FOR NO KEY UPDATE OF p SKIP LOCKED")
        .fetch_optional(&mut *tx).await?;
    if let Some((project, enabled)) = p {
        if !enabled {
            block_expired(&mut tx, project).await?;
        } else {
            let jobs:Vec<(Uuid,i64,bool)>=sqlx::query_as("SELECT j.id,j.revision,r.has_on_timer OR j.host_action IS NOT NULL FROM script_jobs j JOIN script_revisions r ON r.project_id=j.project_id AND r.revision=j.revision WHERE j.project_id=$1 AND j.status='scheduled' AND j.scheduled_for<=now() ORDER BY j.scheduled_for,j.id LIMIT 16 FOR UPDATE OF j")
                .bind(project).fetch_all(&mut *tx).await?;
            for (job, revision, has_timer) in jobs {
                if !has_timer {
                    sqlx::query("UPDATE script_jobs SET status='blocked',reason='missing_on_timer' WHERE id=$1").bind(job).execute(&mut *tx).await?;
                    continue;
                }
                sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,job_id,actor_type) VALUES($1,$2,$3,'timer',$4,'scheduler') ON CONFLICT(job_id) DO NOTHING")
                    .bind(Uuid::new_v4()).bind(project).bind(revision).bind(job).execute(&mut *tx).await?;
                sqlx::query("UPDATE script_jobs SET status='queued' WHERE id=$1")
                    .bind(job)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await
}

pub(super) async fn step(state: Arc<AppState>) -> Result<bool, sqlx::Error> {
    timers(&state).await?;
    let mut tx = state.db.pool().begin().await?;
    let project:Option<(Uuid,String,bool)>=sqlx::query_as("SELECT p.id,p.channel_id,p.enabled AND p.deleted_at IS NULL FROM script_projects p WHERE EXISTS(SELECT 1 FROM script_executions e WHERE e.project_id=p.id AND e.status IN ('queued','running')) ORDER BY (SELECT min(sequence) FROM script_executions e WHERE e.project_id=p.id AND e.status IN ('queued','running')) LIMIT 1 FOR NO KEY UPDATE OF p SKIP LOCKED")
        .fetch_optional(&mut *tx).await?;
    let Some((project, channel, enabled)) = project else {
        return Ok(false);
    };
    let row=sqlx::query("SELECT id,revision,source,snapshot_id,event_index,job_id,status,actor_type FROM script_executions WHERE project_id=$1 AND status IN ('queued','running') ORDER BY sequence LIMIT 1")
        .bind(project).fetch_one(&mut *tx).await?;
    let id: Uuid = row.get("id");
    let revision: i64 = row.get("revision");
    let source: String = row.get("source");
    let job: Option<Uuid> = row.get("job_id");
    let status: String = row.get("status");
    let actor: String = row.get("actor_type");
    if status == "running" {
        // A previous worker died with ambiguous external effects. Never replay purchases/chat.
        sqlx::query("UPDATE script_executions SET status='interrupted',finished_at=now(),report='{\"error\":\"Worker interrupted; side effects may have occurred. Not replayed.\"}' WHERE id=$1").bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE script_jobs SET status='failed',reason='execution_interrupted',completed_at=now() WHERE id=$1").bind(job).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(true);
    }
    if !enabled && actor != "user" {
        sqlx::query("UPDATE script_executions SET status='skipped',finished_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE script_jobs SET status='blocked',reason='project_disabled' WHERE id=$1",
        )
        .bind(job)
        .execute(&mut *tx)
        .await?;
        // A blocked job has no actual execution yet; permit a new manual execution.
        sqlx::query("UPDATE script_executions SET job_id=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(true);
    }
    let files: Value = sqlx::query_scalar(
        "SELECT files FROM script_revisions WHERE project_id=$1 AND revision=$2",
    )
    .bind(project)
    .bind(revision)
    .fetch_one(&mut *tx)
    .await?;
    let files: Files =
        serde_json::from_value(files).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let mut context = if source == "cs2" {
        let snapshot: Option<i64> = row.get("snapshot_id");
        let mut ctx: Value = sqlx::query_scalar("SELECT context FROM script_snapshots WHERE id=$1")
            .bind(snapshot)
            .fetch_one(&mut *tx)
            .await?;
        let index: Option<i32> = row.get("event_index");
        ctx["event"] = ctx["events"][index.unwrap_or(0) as usize].clone();
        ctx.as_object_mut().map(|m| m.remove("events"));
        ctx
    } else {
        let job:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'key',job_key,'payload',payload,'scheduled_for',scheduled_for,'created_at',created_at,'creating_revision',revision,'host_action',host_action) FROM script_jobs WHERE id=$1").bind(job).fetch_one(&mut *tx).await?;
        json!({"timer":job,"source":"timer"})
    };
    context["actor_type"] = json!(actor);
    // Commit the running marker on a separate connection while retaining project exclusivity.
    sqlx::query("UPDATE script_executions SET status='running' WHERE id=$1")
        .bind(id)
        .execute(state.db.pool())
        .await?;
    let attr = Attribution {
        project_id: project,
        revision,
        execution_id: id,
        channel_id: channel.clone(),
    };
    let report = if context["timer"]["host_action"] == "hide_reward" {
        let reward = context["timer"]["payload"]["reward_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok());
        let result = if let Some(reward) = reward {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                service::mutate_reward(&state, &channel, reward, Some(false), None),
            )
            .await
            .unwrap_or_else(|_| Err("host_timeout".into()))
        } else {
            Err("invalid_reward".into())
        };
        service::Report {
            error: result.err(),
            actions: vec![json!({"method":"rewards.set_visible","value":false,"reward_id":reward})],
            ..Default::default()
        }
    } else {
        service::run(
            state.clone(),
            files,
            if source == "cs2" {
                "on_event"
            } else {
                "on_timer"
            }
            .into(),
            context,
            attr.clone(),
            false,
        )
        .await
    };
    let success = report.error.is_none();
    sqlx::query("UPDATE script_executions SET status=$2,report=$3,finished_at=now() WHERE id=$1")
        .bind(id)
        .bind(if success { "success" } else { "failed" })
        .bind(json!(report))
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE script_jobs SET status=$2,completed_at=now() WHERE id=$1")
        .bind(job)
        .bind(if success { "completed" } else { "failed" })
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    service::audit(
        &state,
        &channel,
        "script.execution",
        json!({"actor_type":actor,"script":attr,"success":success}),
    );
    Ok(true)
}
