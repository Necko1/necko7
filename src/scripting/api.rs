use super::{
    runtime::{self, Files},
    service::{self, Attribution},
    worker,
};
use crate::{
    api::{
        error::ApiError,
        extractor::{authorized_channel::AuthorizedChannel, json::JsonArg},
    },
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock};
use tokio::sync::Semaphore;
use uuid::Uuid;

static EDITOR_WORK: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));
fn bad(message: impl Into<String>) -> ApiError {
    ApiError::BadRequest {
        message: message.into(),
        param: "script".into(),
    }
}
fn db(error: sqlx::Error) -> ApiError {
    crate::db::error::DbError::from(error).into()
}
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/broadcasters/{channel_id}/scripts",
            get(list).post(command),
        )
        .layer(DefaultBodyLimit::max(512 * 1024))
}
#[derive(Default, Deserialize)]
pub struct ExecutionSearch {
    #[serde(default)]
    pub execution_search: String,
}
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthorizedChannel,
    Query(filter): Query<ExecutionSearch>,
) -> Result<Json<Value>, ApiError> {
    auth.require_owner()?;
    let search = filter.execution_search.trim();
    if search.chars().count() > 128 {
        return Err(bad("Execution search is limited to 128 characters"));
    }
    let pool = state.db.pool();
    let projects:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(p) || jsonb_build_object('live_files',r.files) FROM script_projects p LEFT JOIN script_revisions r ON r.project_id=p.id AND r.revision=p.active_revision WHERE channel_id=$1 AND deleted_at IS NULL ORDER BY p.created_at").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    let revisions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('project_id',r.project_id,'revision',revision,'has_on_event',has_on_event,'has_on_timer',has_on_timer,'created_at',r.created_at) FROM script_revisions r JOIN script_projects p ON p.id=r.project_id WHERE p.channel_id=$1 ORDER BY r.created_at DESC LIMIT 500").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    let jobs:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(j) FROM script_jobs j JOIN script_projects p ON p.id=j.project_id WHERE p.channel_id=$1 ORDER BY j.created_at DESC LIMIT 500").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    let storage:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM script_storage s JOIN script_projects p ON p.id=s.project_id WHERE p.channel_id=$1 ORDER BY s.key LIMIT 2000").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    // Search retained history before limiting it, rather than searching only the latest 200.
    let mut executions:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) || jsonb_build_object('event',s.context->'events'->e.event_index,'source_meta',s.context->'source') FROM script_executions e JOIN script_projects p ON p.id=e.project_id LEFT JOIN script_snapshots s ON s.id=e.snapshot_id WHERE p.channel_id=$1 AND ($2='' OR strpos(lower((to_jsonb(e) || jsonb_build_object('event',s.context->'events'->e.event_index,'source_meta',s.context->'source'))::text || p.name),lower($2))>0) ORDER BY e.sequence DESC LIMIT 200").bind(&auth.channel_id).bind(search).fetch_all(pool).await.map_err(db)?;
    let reports:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM script_editor_reports e JOIN script_projects p ON p.id=e.project_id WHERE p.channel_id=$1 AND ($2='' OR strpos(lower(to_jsonb(e)::text || p.name),lower($2))>0) ORDER BY e.created_at DESC LIMIT 100").bind(&auth.channel_id).bind(search).fetch_all(pool).await.map_err(db)?;
    executions.extend(reports);
    executions.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    executions.truncate(200);
    let matches:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(m) FROM script_matches m WHERE channel_id=$1 ORDER BY m.created_at DESC LIMIT 31").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    let snapshots:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'created_at',created_at,'events',context->'events') FROM script_snapshots WHERE channel_id=$1 ORDER BY id DESC LIMIT 30").bind(&auth.channel_id).fetch_all(pool).await.map_err(db)?;
    Ok(Json(
        json!({"projects":projects,"revisions":revisions,"jobs":jobs,"storage":storage,"executions":executions,"matches":matches,"snapshots":snapshots}),
    ))
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Create {
        name: String,
    },
    Rename {
        project_id: Uuid,
        name: String,
    },
    Enable {
        project_id: Uuid,
        enabled: bool,
    },
    Delete {
        project_id: Uuid,
        confirmation: String,
    },
    Save {
        project_id: Uuid,
        version: i64,
        files: Files,
    },
    Validate {
        project_id: Uuid,
    },
    Publish {
        project_id: Uuid,
        version: i64,
    },
    Rollback {
        project_id: Uuid,
        revision: i64,
    },
    Test {
        project_id: Uuid,
        entry: String,
        context: Value,
    },
    Snapshot {
        project_id: Uuid,
        snapshot_id: i64,
        event_index: usize,
    },
    Revision {
        project_id: Uuid,
        revision: i64,
    },
    StorageSet {
        project_id: Uuid,
        key: String,
        value: Value,
    },
    StorageDelete {
        project_id: Uuid,
        key: String,
    },
    StorageClear {
        project_id: Uuid,
        confirmation: String,
    },
    RunJob {
        project_id: Uuid,
        job_id: Uuid,
        confirmation: String,
    },
    CancelJob {
        project_id: Uuid,
        job_id: Uuid,
    },
}
impl Command {
    fn project(&self) -> Option<Uuid> {
        match self {
            Self::Create { .. } => None,
            Self::Rename { project_id, .. }
            | Self::Enable { project_id, .. }
            | Self::Delete { project_id, .. }
            | Self::Save { project_id, .. }
            | Self::Validate { project_id }
            | Self::Publish { project_id, .. }
            | Self::Rollback { project_id, .. }
            | Self::Test { project_id, .. }
            | Self::Snapshot { project_id, .. }
            | Self::Revision { project_id, .. }
            | Self::StorageSet { project_id, .. }
            | Self::StorageDelete { project_id, .. }
            | Self::StorageClear { project_id, .. }
            | Self::RunJob { project_id, .. }
            | Self::CancelJob { project_id, .. } => Some(*project_id),
        }
    }
}
pub async fn command(
    State(state): State<Arc<AppState>>,
    auth: AuthorizedChannel,
    JsonArg(cmd): JsonArg<Command>,
) -> Result<Json<Value>, ApiError> {
    auth.require_owner()?;
    let pool = state.db.pool();
    let project = cmd.project();
    if let Some(id) = project {
        let owned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM script_projects WHERE id=$1 AND channel_id=$2 AND deleted_at IS NULL)").bind(id).bind(&auth.channel_id).fetch_one(pool).await.map_err(db)?;
        if !owned {
            return Err(bad("Project not found"));
        }
    }
    let mut audit_action = match &cmd {
        Command::Create { .. } => Some("script.project_created"),
        Command::Rename { .. } => Some("script.project_renamed"),
        Command::Enable { enabled, .. } => Some(if *enabled {
            "script.project_enabled"
        } else {
            "script.project_disabled"
        }),
        Command::Delete { .. } => Some("script.project_deleted"),
        Command::Publish { .. } => Some("script.project_published"),
        Command::Rollback { .. } => Some("script.project_rollback"),
        Command::RunJob { .. } => Some("script.job_run"),
        Command::CancelJob { .. } => Some("script.job_cancelled"),
        _ => None,
    };
    let project_name: Option<String> = if let Some(id) = project.filter(|_| audit_action.is_some())
    {
        sqlx::query_scalar("SELECT name FROM script_projects WHERE id=$1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db)?
    } else {
        None
    };
    let mut audit_details = json!({"actor_type":"user","user_id":auth.user_id,"user_login":auth.user_login,"project_id":project,"project_name":project_name});
    match &cmd {
        Command::Create { name } => audit_details["project_name"] = json!(name.trim()),
        Command::Rename { name, .. } => {
            audit_details["previous_name"] = audit_details["project_name"].clone();
            audit_details["project_name"] = json!(name.trim());
        }
        Command::Rollback { revision, .. } => audit_details["revision"] = json!(revision),
        Command::RunJob { job_id, .. } | Command::CancelJob { job_id, .. } => {
            audit_details["job_id"] = json!(job_id);
            if let Some((key, revision)) = sqlx::query_as::<_, (String, i64)>(
                "SELECT job_key,revision FROM script_jobs WHERE id=$1 AND project_id=$2",
            )
            .bind(job_id)
            .bind(project)
            .fetch_optional(pool)
            .await
            .map_err(db)?
            {
                audit_details["job_key"] = json!(key);
                audit_details["revision"] = json!(revision);
            }
        }
        _ => {}
    }
    let result = match cmd {
        Command::Revision {
            project_id,
            revision,
        } => {
            let files: Value = sqlx::query_scalar(
                "SELECT files FROM script_revisions WHERE project_id=$1 AND revision=$2",
            )
            .bind(project_id)
            .bind(revision)
            .fetch_optional(pool)
            .await
            .map_err(db)?
            .ok_or_else(|| bad("Published version not found"))?;
            json!({"files":files,"revision":revision})
        }
        Command::Create { name } => {
            if name.trim().is_empty() || name.len() > 80 {
                return Err(bad("Name must be 1–80 characters"));
            }
            let mut tx = pool.begin().await.map_err(db)?;
            sqlx::query("SELECT channel_id FROM broadcasters WHERE channel_id=$1 FOR UPDATE")
                .bind(&auth.channel_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM script_projects WHERE channel_id=$1 AND deleted_at IS NULL",
            )
            .bind(&auth.channel_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if count >= 20 {
                return Err(bad("Channel project limit (20) reached"));
            }
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO script_projects(id,channel_id,name) VALUES($1,$2,$3)")
                .bind(id)
                .bind(&auth.channel_id)
                .bind(name.trim())
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            tx.commit().await.map_err(db)?;
            json!({"id":id})
        }
        Command::Snapshot {
            snapshot_id,
            event_index,
            ..
        } => {
            let mut ctx: Value = sqlx::query_scalar(
                "SELECT context FROM script_snapshots WHERE id=$1 AND channel_id=$2",
            )
            .bind(snapshot_id)
            .bind(&auth.channel_id)
            .fetch_optional(pool)
            .await
            .map_err(db)?
            .ok_or_else(|| bad("Recorded context not found"))?;
            let event = ctx["events"]
                .as_array()
                .and_then(|events| events.get(event_index))
                .cloned()
                .ok_or_else(|| bad("Event not found"))?;
            ctx["event"] = event;
            ctx.as_object_mut().unwrap().remove("events");
            ctx
        }
        Command::Validate { project_id } | Command::Test { project_id, .. } => {
            let _permit = EDITOR_WORK
                .try_acquire()
                .map_err(|_| bad("Editor workers busy; retry shortly"))?;
            let (files, revision): (Value, Option<i64>) =
                sqlx::query_as("SELECT draft,active_revision FROM script_projects WHERE id=$1")
                    .bind(project_id)
                    .fetch_one(pool)
                    .await
                    .map_err(db)?;
            let files: Files = serde_json::from_value(files).map_err(|_| bad("Invalid draft"))?;
            if let Command::Test { entry, context, .. } = cmd {
                if !["on_event", "on_timer"].contains(&entry.as_str()) || !context.is_object() {
                    return Err(bad("Invalid test entrypoint or context"));
                }
                let report = service::run(
                    state.clone(),
                    files,
                    entry,
                    context,
                    Attribution {
                        project_id,
                        revision: revision.unwrap_or(0),
                        execution_id: Uuid::new_v4(),
                        channel_id: auth.channel_id.clone(),
                    },
                    true,
                )
                .await;
                editor_report(
                    pool,
                    project_id,
                    "dry_run",
                    report.error.is_none(),
                    json!(report),
                )
                .await?;
                json!(report)
            } else {
                let validated = tokio::task::spawn_blocking(move || runtime::validate(&files))
                    .await
                    .map_err(|_| bad("Compiler failed"))?;
                editor_report(
                    pool,
                    project_id,
                    "validate",
                    validated.is_ok(),
                    json!({"error":validated.as_ref().err()}),
                )
                .await?;
                json!(validated.map_err(bad)?)
            }
        }
        Command::StorageSet {
            project_id,
            key,
            value,
        } => service::storage_write(pool, project_id, &key, Some(value), false)
            .await
            .map_err(bad)?,
        Command::StorageDelete { project_id, key } => {
            service::storage_write(pool, project_id, &key, None, false)
                .await
                .map_err(bad)?
        }
        cmd => {
            let id = project.unwrap();
            let mut tx = pool.begin().await.map_err(db)?;
            let (files,version,enabled):(Value,i64,bool)=sqlx::query_as("SELECT draft,draft_version,enabled FROM script_projects WHERE id=$1 AND deleted_at IS NULL FOR NO KEY UPDATE").bind(id).fetch_one(&mut *tx).await.map_err(db)?;
            let mut result = json!({"ok":true});
            match cmd {
                Command::Rename { name, .. } => {
                    if name.trim().is_empty() || name.len() > 80 {
                        return Err(bad("Name must be 1–80 characters"));
                    }
                    sqlx::query("UPDATE script_projects SET name=$2 WHERE id=$1")
                        .bind(id)
                        .bind(name.trim())
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                }
                Command::Enable {
                    enabled: target, ..
                } => {
                    if !enabled {
                        worker::block_expired(&mut tx, id).await.map_err(db)?;
                    }
                    sqlx::query("UPDATE script_projects SET enabled=$2 WHERE id=$1")
                        .bind(id)
                        .bind(target)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                }
                Command::Delete { confirmation, .. } => {
                    if confirmation != "DELETE" {
                        return Err(bad("Type DELETE to confirm"));
                    }
                    sqlx::query(
                        "UPDATE script_projects SET enabled=false,deleted_at=now() WHERE id=$1",
                    )
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                    sqlx::query("UPDATE script_jobs SET status='cancelled',reason='project_deleted',completed_at=now() WHERE project_id=$1 AND status IN ('scheduled','blocked','queued')").bind(id).execute(&mut *tx).await.map_err(db)?;
                    sqlx::query("UPDATE script_executions SET status='skipped',finished_at=now() WHERE project_id=$1 AND status='queued'").bind(id).execute(&mut *tx).await.map_err(db)?;
                }
                Command::Save {
                    version: expected,
                    files,
                    ..
                } => {
                    if version != expected {
                        return Err(bad("Draft changed in another editor; reload before saving"));
                    }
                    runtime::validate_files(&files).map_err(bad)?;
                    sqlx::query("UPDATE script_projects SET draft=$2,draft_version=draft_version+1 WHERE id=$1").bind(id).bind(json!(files)).execute(&mut *tx).await.map_err(db)?;
                    result = json!({"version":version+1});
                }
                Command::Publish {
                    version: expected, ..
                } => {
                    if version != expected {
                        return Err(bad("Draft changed; reload before publishing"));
                    }
                    let _permit = EDITOR_WORK
                        .try_acquire()
                        .map_err(|_| bad("Compiler busy"))?;
                    let f: Files =
                        serde_json::from_value(files.clone()).map_err(|_| bad("Invalid files"))?;
                    let validated = tokio::task::spawn_blocking(move || runtime::validate(&f))
                        .await
                        .map_err(|_| bad("Compiler failed"))?;
                    editor_report(
                        pool,
                        id,
                        "publish",
                        validated.is_ok(),
                        json!({"error":validated.as_ref().err()}),
                    )
                    .await?;
                    let handlers = validated.map_err(bad)?;
                    let revision:i64=sqlx::query_scalar("SELECT COALESCE(max(revision),0)+1 FROM script_revisions WHERE project_id=$1").bind(id).fetch_one(&mut *tx).await.map_err(db)?;
                    if revision > 200 {
                        return Err(bad(
                            "Project revision limit (200) reached; create a new project to preserve pinned history",
                        ));
                    }
                    sqlx::query("INSERT INTO script_revisions(project_id,revision,files,has_on_event,has_on_timer) VALUES($1,$2,$3,$4,$5)").bind(id).bind(revision).bind(files).bind(handlers.has_on_event).bind(handlers.has_on_timer).execute(&mut *tx).await.map_err(db)?;
                    sqlx::query("UPDATE script_projects SET active_revision=$2 WHERE id=$1")
                        .bind(id)
                        .bind(revision)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                    result = json!({"revision":revision});
                }
                Command::Rollback { revision, .. } => {
                    sqlx::query("UPDATE script_projects SET active_revision=$2 WHERE id=$1")
                        .bind(id)
                        .bind(revision)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                }
                Command::StorageClear { confirmation, .. } => {
                    if confirmation != "CLEAR" {
                        return Err(bad("Type CLEAR to confirm"));
                    }
                    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,7))")
                        .bind(id.to_string())
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                    sqlx::query("DELETE FROM script_storage WHERE project_id=$1")
                        .bind(id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                }
                Command::RunJob {
                    job_id,
                    confirmation,
                    ..
                } => {
                    if confirmation != "RUN" {
                        return Err(bad(
                            "Confirm RUN: pinned code may perform real side effects, even when disabled",
                        ));
                    }
                    let revision:Option<i64>=sqlx::query_scalar("UPDATE script_jobs SET status='queued',reason=NULL WHERE id=$1 AND project_id=$2 AND status IN ('scheduled','blocked') RETURNING revision").bind(job_id).bind(id).fetch_optional(&mut *tx).await.map_err(db)?;
                    let revision = revision
                        .ok_or_else(|| bad("Job is already queued, completed or cancelled"))?;
                    let timer:bool=sqlx::query_scalar("SELECT r.has_on_timer OR EXISTS(SELECT 1 FROM script_jobs j WHERE j.id=$3 AND j.host_action IS NOT NULL) FROM script_revisions r WHERE r.project_id=$1 AND r.revision=$2").bind(id).bind(revision).bind(job_id).fetch_one(&mut *tx).await.map_err(db)?;
                    if !timer {
                        return Err(bad("Pinned revision has no on_timer"));
                    }
                    sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,job_id,actor_type,actor_user_id) VALUES($1,$2,$3,'timer',$4,'user',$5)").bind(Uuid::new_v4()).bind(id).bind(revision).bind(job_id).bind(&auth.user_id).execute(&mut *tx).await.map_err(db)?;
                }
                Command::CancelJob { job_id, .. } => {
                    let changed = sqlx::query("UPDATE script_jobs SET status='cancelled',completed_at=now() WHERE id=$1 AND project_id=$2 AND status IN ('scheduled','blocked','queued')").bind(job_id).bind(id).execute(&mut *tx).await.map_err(db)?.rows_affected();
                    if changed == 0 {
                        audit_action = None;
                    }
                    sqlx::query("UPDATE script_executions SET status='skipped',finished_at=now() WHERE project_id=$1 AND job_id=$2 AND status='queued'").bind(id).bind(job_id).execute(&mut *tx).await.map_err(db)?;
                }
                _ => unreachable!(),
            }
            tx.commit().await.map_err(db)?;
            result
        }
    };
    if let Some(action) = audit_action {
        if let Some(id) = result.get("id") {
            audit_details["project_id"] = id.clone();
        }
        if let Some(revision) = result.get("revision") {
            audit_details["revision"] = revision.clone();
        }
        service::audit(&state, &auth.channel_id, action, audit_details);
    }
    Ok(Json(result))
}

async fn editor_report(
    pool: &sqlx::PgPool,
    project: Uuid,
    source: &str,
    success: bool,
    report: Value,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO script_editor_reports(id,project_id,source,status,report) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(project).bind(source).bind(if success {"success"}else{"failed"}).bind(report).execute(pool).await.map_err(db)?;
    Ok(())
}
