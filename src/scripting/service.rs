use super::runtime::{self, Files, MessageMode, MessageOperation, UserFilter};
use crate::state::AppState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Attribution {
    pub project_id: Uuid,
    pub revision: i64,
    pub execution_id: Uuid,
    pub channel_id: String,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Report {
    pub meta: Value,
    pub logs: Vec<Value>,
    pub actions: Vec<Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub dry_run: bool,
}
fn preview(value: &Value) -> Value {
    let text = value.to_string();
    if text.len() > 2048 {
        json!({"preview":text.chars().take(1024).collect::<String>(),"truncated":true,"bytes":text.len()})
    } else {
        value.clone()
    }
}

pub fn audit(state: &AppState, channel: &str, action: &str, details: Value) {
    let message = audit_message(action, &details);
    state.channel_logger.log(
        channel,
        crate::db::channel_logs::ChannelLogLevel::Info,
        crate::db::channel_logs::ChannelLogCategory::System,
        action,
        &message,
        Some(details),
        None,
    );
}

pub fn audit_message(action: &str, details: &Value) -> String {
    let project = details["project_name"]
        .as_str()
        .or_else(|| details["script"]["project_id"].as_str())
        .unwrap_or("unknown project");
    let actor = details["user_login"]
        .as_str()
        .map(|login| format!("@{login}"))
        .unwrap_or_else(|| format!("Script \"{project}\""));
    let revision = details
        .get("revision")
        .or_else(|| details["script"].get("revision"));
    let suffix = revision
        .map(|r| format!(" (revision {r})"))
        .unwrap_or_default();
    let reward = details["reward_alias"]
        .as_str()
        .or_else(|| details["reward_id"].as_str())
        .unwrap_or("unknown reward");
    let job = details["job_key"]
        .as_str()
        .or_else(|| details["job_id"].as_str())
        .unwrap_or("unknown job");
    match action {
        "script.project_created" => format!("{actor} created script project \"{project}\""),
        "script.project_renamed" => format!(
            "{actor} renamed script project \"{}\" to \"{project}\"",
            details["previous_name"].as_str().unwrap_or("unknown")
        ),
        "script.project_enabled" => format!("{actor} enabled script project \"{project}\""),
        "script.project_disabled" => format!("{actor} disabled script project \"{project}\""),
        "script.project_deleted" => {
            format!("{actor} deleted script project \"{project}\" and cancelled its pending jobs")
        }
        "script.project_published" => {
            format!("{actor} published script project \"{project}\"{suffix}")
        }
        "script.project_rollback" => {
            format!("{actor} activated an earlier version of \"{project}\"{suffix}")
        }
        "script.job_run" => {
            format!("{actor} requested a manual run of job \"{job}\" for \"{project}\"{suffix}")
        }
        "script.job_cancelled" => {
            format!("{actor} cancelled job \"{job}\" for \"{project}\"{suffix}")
        }
        "rewards.set_visible" => format!(
            "{actor} changed reward \"{reward}\" visibility to {}{suffix}",
            details["value"]
        ),
        "rewards.set_paused" => format!(
            "{actor} changed reward \"{reward}\" pause state to {}{suffix}",
            details["value"]
        ),
        "reward.script_trigger" => format!(
            "{actor} triggered reward \"{reward}\" for user {} (fulfillment {}){suffix}",
            details["user_id"], details["fulfillment_id"]
        ),
        "scheduler.after" => format!("{actor} scheduled one-shot job {}{suffix}", details["key"]),
        "scheduler.cancel" => format!("{actor} cancelled scheduled job {}{suffix}", details["key"]),
        "rewards.enable_for" => {
            format!("{actor} made reward \"{reward}\" temporarily visible{suffix}")
        }
        "chat.send" | "chat.reply" => format!("{actor} sent a Twitch chat message{suffix}"),
        _ => format!("{actor}: {action}{suffix}"),
    }
}

async fn audit_script(state: &AppState, attr: &Attribution, action: &str, mut details: Value) {
    if let Ok(Some(name)) =
        sqlx::query_scalar::<_, String>("SELECT name FROM script_projects WHERE id=$1")
            .bind(attr.project_id)
            .fetch_optional(state.db.pool())
            .await
    {
        details["project_name"] = json!(name);
    }
    audit(state, &attr.channel_id, action, details);
}

pub async fn run(
    state: Arc<AppState>,
    files: Files,
    entry: String,
    mut context: Value,
    attr: Attribution,
    dry: bool,
) -> Report {
    let report = Arc::new(Mutex::new(Report {
        dry_run: dry,
        ..Report::default()
    }));
    let output = report.clone();
    let handle = tokio::runtime::Handle::current();
    let overlay = Arc::new(Mutex::new(BTreeMap::<String, Option<Value>>::new()));
    let meta = json!({"project_id":attr.project_id,"revision":attr.revision,"execution_id":attr.execution_id,
        "channel_id":attr.channel_id,"timestamp":chrono::Utc::now(),"source":context.get("source")});
    context["meta"] = meta;
    report.lock().meta = context["meta"].clone();
    let start = Instant::now();
    let result=tokio::task::spawn_blocking(move || {
        let host:runtime::Host=Arc::new(move |method,args| {
            if method.starts_with("log.") {
                if args[0].as_str().is_some_and(|s|s.len()>2048) {return Err("log_message_limit".into());}
                output.lock().logs.push(json!({"level":method.trim_start_matches("log."),"message":args[0]}));
                return Ok(Value::Null);
            }
            let value=handle.block_on(async {
                let remaining=std::time::Duration::from_secs(3).saturating_sub(start.elapsed()).min(std::time::Duration::from_secs(2));
                tokio::time::timeout(remaining,call(&state,&attr,method,&args,dry,&overlay)).await
                    .map_err(|_|"host_timeout".to_owned())?
            });
            let value=if method=="rewards.trigger" {Ok(value.unwrap_or_else(|code|json!({"ok":false,"code":code})))} else {value};
            output.lock().actions.push(json!({"method":method,"args":preview(&args),"dry_run":dry,"result":value.as_ref().ok().map(preview),"error":value.as_ref().err()}));
            value
        });
        runtime::execute(files,&entry,context,host)
    }).await;
    let mut report = std::mem::take(&mut *report.lock());
    report.duration_ms = start.elapsed().as_millis() as u64;
    report.error = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some("Runtime worker failed".into()),
    };
    report
}

fn db(error: impl std::fmt::Display) -> String {
    tracing::error!(%error,"Script service operation failed");
    "service_unavailable".into()
}
fn key(args: &Value) -> Result<&str, String> {
    args[0]
        .as_str()
        .filter(|k| !k.is_empty() && k.len() <= 128)
        .ok_or_else(|| "invalid_key".into())
}
pub async fn storage_write(
    pool: &sqlx::PgPool,
    project: Uuid,
    key: &str,
    value: Option<Value>,
    increment: bool,
) -> Result<Value, String> {
    if key.is_empty() || key.len() > 128 {
        return Err("invalid_key".into());
    }
    if value.as_ref().is_some_and(|v| v.to_string().len() > 65536) {
        return Err("storage_value_limit".into());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,7))")
        .bind(project.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let result = if let Some(value) = value {
        let value = if increment {
            let old: Option<Value> = sqlx::query_scalar(
                "SELECT value FROM script_storage WHERE project_id=$1 AND key=$2",
            )
            .bind(project)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
            let n = old
                .unwrap_or(json!(0))
                .as_i64()
                .ok_or("storage_not_integer")?;
            json!(
                n.checked_add(value.as_i64().ok_or("storage_not_integer")?)
                    .ok_or("storage_integer_overflow")?
            )
        } else {
            value
        };
        sqlx::query("INSERT INTO script_storage(project_id,key,value) VALUES($1,$2,$3) ON CONFLICT(project_id,key) DO UPDATE SET value=$3,updated_at=now()")
            .bind(project).bind(key).bind(&value).execute(&mut *tx).await.map_err(db)?;
        let (bytes,count):(i64,i64)=sqlx::query_as("SELECT COALESCE(sum(octet_length(value::text)),0)::bigint,count(*) FROM script_storage WHERE project_id=$1").bind(project).fetch_one(&mut *tx).await.map_err(db)?;
        if bytes > 1048576 || count > 1024 {
            return Err("storage_project_limit".into());
        }
        value
    } else {
        sqlx::query("DELETE FROM script_storage WHERE project_id=$1 AND key=$2")
            .bind(project)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        Value::Null
    };
    tx.commit().await.map_err(db)?;
    Ok(result)
}

pub async fn schedule(
    pool: &sqlx::PgPool,
    attr: &Attribution,
    key: &str,
    seconds: i64,
    payload: Value,
) -> Result<Value, String> {
    schedule_inner(pool, attr, key, seconds, payload, None).await
}
async fn schedule_inner(
    pool: &sqlx::PgPool,
    attr: &Attribution,
    key: &str,
    seconds: i64,
    payload: Value,
    host_action: Option<&str>,
) -> Result<Value, String> {
    if key.is_empty()
        || key.len() > 128
        || !(1..=31_536_000).contains(&seconds)
        || payload.to_string().len() > 65536
    {
        return Err("invalid_job".into());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,8))")
        .bind(attr.project_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let has: bool = sqlx::query_scalar(
        "SELECT has_on_timer FROM script_revisions WHERE project_id=$1 AND revision=$2",
    )
    .bind(attr.project_id)
    .bind(attr.revision)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if !has && host_action.is_none() {
        return Err("missing_on_timer".into());
    }
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM script_jobs WHERE project_id=$1 AND status IN ('scheduled','blocked','queued')").bind(attr.project_id).fetch_one(&mut *tx).await.map_err(db)?;
    if count >= 256 {
        return Err("scheduler_project_limit".into());
    }
    // Replacing a key keeps the old job as cancelled history. Queued jobs cannot be replaced.
    sqlx::query("UPDATE script_jobs SET status='cancelled',reason='replaced',completed_at=now() WHERE project_id=$1 AND job_key=$2 AND status IN ('scheduled','blocked')")
        .bind(attr.project_id).bind(key).execute(&mut *tx).await.map_err(db)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO script_jobs(id,project_id,revision,job_key,payload,scheduled_for,host_action) VALUES($1,$2,$3,$4,$5,now()+$6*interval '1 second',$7)")
        .bind(id).bind(attr.project_id).bind(attr.revision).bind(key).bind(payload).bind(seconds as f64).bind(host_action).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(json!({"id":id,"ok":true}))
}

/// Shared normalized statistics query, independent of HTTP and credentials.
pub async fn recent_chatters(
    pool: &sqlx::PgPool,
    channel: &str,
    seconds: i64,
    filter: UserFilter,
    user: Option<&str>,
) -> Result<Value, String> {
    if !(60..=31_536_000).contains(&seconds) || filter.min_messages < 0 || filter.min_characters < 0
    {
        return Err("invalid_chat_window".into());
    }
    if let Some(reward) = &filter.reward {
        reward.validate()?;
    }
    if let Some(activity) = &filter.activity {
        activity.validate()?;
    }
    if let Some(messages) = &filter.messages {
        messages.validate()?;
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "WITH candidates AS (SELECT chatter_user_id,max(chatter_user_login) AS login,\
         count(*) AS messages,sum(char_count) AS characters,min(sent_at) AS first_activity,\
         max(sent_at) AS last_activity FROM chat_messages WHERE broadcaster_id=",
    );
    query.push_bind(channel).push(" AND sent_at>=now()-");
    query
        .push_bind(seconds as f64)
        .push("*interval '1 second' AND sent_at<=now()");
    if let Some(user) = user {
        query.push(" AND chatter_user_id=").push_bind(user);
    }
    query.push(" GROUP BY chatter_user_id)");
    if let Some(activity) = &filter.activity {
        query
            .push(
                ", activity_eligible AS MATERIALIZED (SELECT c.chatter_user_id FROM candidates c \
            LEFT JOIN chat_messages a ON a.chatter_user_id=c.chatter_user_id AND a.broadcaster_id=",
            )
            .push_bind(channel);
        if let Some(seconds) = activity.seconds {
            query
                .push(" AND a.sent_at>=now()-")
                .push_bind(seconds as f64)
                .push("*interval '1 second'");
        }
        query
            .push(" AND a.sent_at<=now() GROUP BY c.chatter_user_id HAVING count(a.id)>=")
            .push_bind(activity.min_messages);
        query
            .push(" AND coalesce(sum(a.char_count),0)>=")
            .push_bind(activity.min_characters)
            .push(")");
    }
    if let Some(reward) = &filter.reward
        && reward.min_count > 0
    {
        query
            .push(
                ", reward_eligible AS MATERIALIZED (SELECT d.user_id FROM candidates c JOIN redemptions d \
                 ON d.user_id=c.chatter_user_id JOIN rewards r ON r.twitch_id=d.twitch_reward_id \
                 WHERE r.streamer_id=",
            )
            .push_bind(channel);
        if let Some(alias) = &reward.alias {
            query.push(" AND r.script_alias=").push_bind(alias);
        }
        if !reward.statuses.is_empty() {
            query
                .push(" AND d.status=ANY(")
                .push_bind(&reward.statuses)
                .push(")");
        }
        if let Some(seconds) = reward.seconds {
            query
                .push(" AND d.created_at>=now()-")
                .push_bind(seconds as f64)
                .push("*interval '1 second'");
        }
        query
            .push(" AND d.created_at<=now() GROUP BY d.user_id HAVING count(*)>=")
            .push_bind(reward.min_count)
            .push(")");
    }
    if let Some(messages) = &filter.messages {
        // Each nested predicate reads its own window; only candidate IDs are shared.
        query
            .push(
                ", message_eligible AS MATERIALIZED (SELECT DISTINCT m.chatter_user_id FROM candidates c \
            JOIN chat_messages m ON m.chatter_user_id=c.chatter_user_id WHERE m.broadcaster_id=",
            )
            .push_bind(channel);
        if let Some(seconds) = messages.seconds {
            query
                .push(" AND m.sent_at>=now()-")
                .push_bind(seconds as f64)
                .push("*interval '1 second'");
        }
        query.push(" AND m.sent_at<=now() AND (");
        for (index, clause) in messages.clauses.iter().enumerate() {
            if index > 0 {
                query.push(match messages.mode {
                    MessageMode::Any => " OR ",
                    MessageMode::All => " AND ",
                });
            }
            let mut pattern = String::new();
            if matches!(
                clause.operation,
                MessageOperation::Contains | MessageOperation::EndsWith
            ) {
                pattern.push('%');
            }
            for ch in clause.text.chars() {
                if matches!(ch, '%' | '_' | '\\') {
                    pattern.push('\\');
                }
                pattern.push(ch);
            }
            if matches!(
                clause.operation,
                MessageOperation::Contains | MessageOperation::StartsWith
            ) {
                pattern.push('%');
            }
            query.push(if messages.case_sensitive {
                "m.message_text COLLATE \"C\" LIKE "
            } else {
                "m.message_text COLLATE \"und-x-icu\" ILIKE "
            });
            query.push_bind(pattern).push(" ESCAPE E'\\\\'");
        }
        query.push("))");
    }
    query.push(
        " SELECT jsonb_build_object('id',c.chatter_user_id,'login',c.login,\
        'messages',c.messages,'characters',c.characters,'first_activity',c.first_activity,\
        'last_activity',c.last_activity) FROM candidates c",
    );
    if filter.activity.is_some() {
        query.push(" JOIN activity_eligible a ON a.chatter_user_id=c.chatter_user_id");
    }
    if filter.messages.is_some() {
        query.push(" JOIN message_eligible m ON m.chatter_user_id=c.chatter_user_id");
    }
    if filter.reward.is_some_and(|r| r.min_count > 0) {
        query.push(" JOIN reward_eligible r ON r.user_id=c.chatter_user_id");
    }
    // Released convenience methods retain their candidate-window thresholds.
    query
        .push(" WHERE c.messages>=")
        .push_bind(filter.min_messages);
    query
        .push(" AND c.characters>=")
        .push_bind(filter.min_characters);
    query.push(" ORDER BY c.last_activity DESC,c.chatter_user_id LIMIT 1000");
    let rows: Vec<Value> = query
        .build_query_scalar()
        .fetch_all(pool)
        .await
        .map_err(db)?;
    Ok(json!(rows))
}

async fn call(
    state: &Arc<AppState>,
    attr: &Attribution,
    method: &str,
    args: &Value,
    dry: bool,
    overlay: &Mutex<BTreeMap<String, Option<Value>>>,
) -> Result<Value, String> {
    let pool = state.db.pool();
    match method {
        "storage.get" => {
            let k=key(args)?;
            if dry && let Some(v)=overlay.lock().get(k) { return Ok(v.clone().unwrap_or_else(||args[1].clone())); }
            Ok(sqlx::query_scalar::<_,Value>("SELECT value FROM script_storage WHERE project_id=$1 AND key=$2").bind(attr.project_id).bind(k).fetch_optional(pool).await.map_err(db)?.unwrap_or_else(||args[1].clone()))
        }
        "storage.set" | "storage.delete" | "storage.increment" => {
            let k=key(args)?;
            if dry {
                let mut value=if method=="storage.delete" {Value::Null} else {args[1].clone()};
                if method=="storage.increment" {
                    let old=overlay.lock().get(k).cloned();
                    let old=match old {Some(v)=>v.unwrap_or(json!(0)),None=>sqlx::query_scalar::<_,Value>("SELECT value FROM script_storage WHERE project_id=$1 AND key=$2").bind(attr.project_id).bind(k).fetch_optional(pool).await.map_err(db)?.unwrap_or(json!(0))};
                    value=json!(old.as_i64().ok_or("storage_not_integer")?.checked_add(args[1].as_i64().ok_or("storage_not_integer")?).ok_or("storage_integer_overflow")?);
                }
                overlay.lock().insert(k.into(),(method!="storage.delete").then(||value.clone())); return Ok(value);
            }
            storage_write(pool,attr.project_id,k,(method!="storage.delete").then(||args[1].clone()),method=="storage.increment").await
        }
        "scheduler.after" => {
            let k=key(args)?;
            if dry { return Ok(json!({"ok":true,"planned":true})); }
            let result=schedule(pool,attr,k,args[1].as_i64().ok_or("invalid_duration")?,args[2].clone()).await?;
            audit_script(state,attr,method,json!({"actor_type":"script","script":attr,"job":result,"key":k})).await; Ok(result)
        }
        "scheduler.exists" => Ok(json!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM script_jobs WHERE project_id=$1 AND job_key=$2 AND status IN ('scheduled','blocked','queued'))").bind(attr.project_id).bind(key(args)?).fetch_one(pool).await.map_err(db)?)),
        "scheduler.cancel" => {
            if !dry { let changed=sqlx::query("UPDATE script_jobs SET status='cancelled',completed_at=now() WHERE project_id=$1 AND job_key=$2 AND status IN ('scheduled','blocked')").bind(attr.project_id).bind(key(args)?).execute(pool).await.map_err(db)?.rows_affected();
                if changed > 0 { audit_script(state,attr,method,json!({"actor_type":"script","script":attr,"key":args[0]})).await; } }
            Ok(json!({"ok":true}))
        }
        "chat.recent_chatters" | "users.recent_chatters" => recent_chatters(pool,&attr.channel_id,args[0].as_i64().ok_or("invalid_duration")?,serde_json::from_value(args[1].clone()).map_err(db)?,None).await,
        "chat.user_stats" | "users.user_stats" => {
            let rows=recent_chatters(pool,&attr.channel_id,args[1].as_i64().ok_or("invalid_duration")?,UserFilter::default(),Some(key(args)?)).await?;
            let mut stats=rows[0].clone();
            if stats.is_null() {stats=json!({"id":key(args)?,"messages":0,"characters":0,"first_activity":null,"last_activity":null});}
            let redemptions:Value=sqlx::query_scalar("SELECT jsonb_build_object('total',count(*),'completed',count(*) FILTER(WHERE d.status='COMPLETED'),'script',count(*) FILTER(WHERE d.origin='SCRIPT')) FROM redemptions d JOIN rewards r ON r.twitch_id=d.twitch_reward_id WHERE r.streamer_id=$1 AND d.user_id=$2 AND d.created_at>=now()-$3*interval '1 second'")
                .bind(&attr.channel_id).bind(key(args)?).bind(args[1].as_i64().unwrap() as f64).fetch_one(pool).await.map_err(db)?;
            stats["redemptions"]=redemptions; Ok(stats)
        }
        "chat.send" | "chat.reply" => {
            let (message,reply)=if method=="chat.reply" {(args[1].as_str(),args[0].as_str())} else {(args[0].as_str(),None)};
            let message=message.filter(|m|!m.is_empty() && m.chars().count()<=500).ok_or("invalid_message")?;
            if !dry { state.send_chat_message(&attr.channel_id,message,reply).await.map_err(db)?;
                audit_script(state,attr,method,json!({"actor_type":"script","script":attr})).await; }
            Ok(json!({"ok":true,"planned":dry}))
        }
        "rewards.get" => {
            let row:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',twitch_id,'alias',script_alias,'title',twitch_title,'is_visible',is_visible,'is_paused',is_paused) FROM rewards WHERE streamer_id=$1 AND script_alias=$2 AND NOT is_deleted")
                .bind(&attr.channel_id).bind(key(args)?).fetch_optional(pool).await.map_err(db)?;
            Ok(row.unwrap_or(Value::Null))
        }
        "rewards.set_visible" | "rewards.set_paused" | "rewards.enable_for" => {
            let id:Option<Uuid>=sqlx::query_scalar("SELECT twitch_id FROM rewards WHERE streamer_id=$1 AND script_alias=$2 AND NOT is_deleted").bind(&attr.channel_id).bind(key(args)?).fetch_optional(pool).await.map_err(db)?;
            let id=id.ok_or("reward_not_found")?;
            if method=="rewards.enable_for" {
                if dry {return Ok(json!({"ok":true,"planned":true}));}
                // Persist the reversal before changing visibility. This is a host job,
                // not a call to on_timer, and needs no user-defined handler.
                let scheduled=schedule_inner(pool,attr,&format!("visibility:{id}"),args[1].as_i64().ok_or("invalid_duration")?,json!({"reward_id":id}),Some("hide_reward")).await?;
                mutate_reward(state,&attr.channel_id,id,Some(true),None).await?;
                audit_script(state,attr,method,json!({"actor_type":"script","script":attr,"reward_id":id,"reward_alias":args[0],"job":scheduled})).await;
                return Ok(scheduled);
            }
            if !dry { mutate_reward(state,&attr.channel_id,id,(method=="rewards.set_visible").then(||args[1].as_bool().unwrap_or(false)),(method=="rewards.set_paused").then(||args[1].as_bool().unwrap_or(false))).await?;
                audit_script(state,attr,method,json!({"actor_type":"script","script":attr,"reward_id":id,"reward_alias":args[0],"value":args[1]})).await; }
            Ok(json!({"ok":true,"planned":dry}))
        }
        "rewards.trigger" => trigger(state,attr,key(args)?,args[1].as_str().ok_or("invalid_user")?,dry).await,
        _ => Err("unknown_capability".into())
    }
}

pub(super) async fn trigger(
    state: &Arc<AppState>,
    attr: &Attribution,
    alias: &str,
    user: &str,
    dry: bool,
) -> Result<Value, String> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT twitch_id FROM rewards WHERE streamer_id=$1 AND script_alias=$2 AND NOT is_deleted",
    )
    .bind(&attr.channel_id)
    .bind(alias)
    .fetch_optional(state.db.pool())
    .await
    .map_err(db)?;
    let Some(id) = id else {
        return Ok(json!({"ok":false,"code":"reward_not_found"}));
    };
    let reward = state
        .db
        .get_reward_by_twitch_id(id)
        .await
        .map_err(db)?
        .ok_or("reward_not_found")?;
    let login:Option<String>=sqlx::query_scalar("SELECT login FROM (SELECT chatter_user_login AS login,0 AS priority,sent_at FROM chat_messages WHERE broadcaster_id=$1 AND chatter_user_id=$2 UNION ALL SELECT login,1,NULL::timestamptz FROM users WHERE twitch_id=$2) u ORDER BY priority,sent_at DESC LIMIT 1").bind(&attr.channel_id).bind(user).fetch_optional(state.db.pool()).await.map_err(db)?;
    let Some(login) = login else {
        return Ok(json!({"ok":false,"code":"user_not_found"}));
    };
    if reward.is_paused {
        return Ok(json!({"ok":false,"code":"reward_paused"}));
    }
    let recent:i64=sqlx::query_scalar("SELECT count(*) FROM redemptions WHERE script_project_id=$1 AND created_at>now()-interval '1 minute'").bind(attr.project_id).fetch_one(state.db.pool()).await.map_err(db)?;
    if recent >= 10 {
        return Ok(json!({"ok":false,"code":"trigger_rate_limit","retry_after":60}));
    }
    if dry {
        let since = reward
            .chat_time_window_hours
            .filter(|h| *h > 0)
            .map(|h| chrono::Utc::now() - chrono::Duration::hours(i64::from(h)));
        let (messages, characters) = state
            .db
            .get_user_chat_stats(&attr.channel_id, user, since)
            .await
            .map_err(db)?;
        if !crate::processor::redemption::chat_requirements_pass(&reward, messages, characters) {
            return Ok(json!({"ok":false,"code":"activity_requirement_failed","planned":true}));
        }
        if let Some(limits) = &reward.purchase_limits {
            let mut connection = state.db.pool().acquire().await.map_err(db)?;
            for (rules, viewer) in [(&limits.0.global, None), (&limits.0.user, Some(user))] {
                for rule in rules {
                    let count = crate::db::redemptions::count_reward_redemptions_on_connection(
                        &mut connection,
                        id,
                        viewer,
                        rule.window_hours,
                        None,
                    )
                    .await
                    .map_err(db)?;
                    if count >= i64::from(rule.max_redemptions) {
                        return Ok(
                            json!({"ok":false,"code":"purchase_limit_reached","planned":true}),
                        );
                    }
                }
            }
        }
        return Ok(
            json!({"ok":true,"planned":true,"origin":"SCRIPT","validation":"Chat activity and current purchase counts passed. No capacity reserved; market/buyer validation has not run."}),
        );
    }
    let fulfillment = Uuid::new_v4();
    let input = crate::processor::model::RedemptionEvent {
        id: fulfillment,
        broadcaster_user_id: attr.channel_id.clone(),
        broadcaster_user_login: attr.channel_id.clone(),
        user_id: user.into(),
        user_login: login.clone(),
        user_name: login,
        user_input: String::new(),
        status: "unfulfilled".into(),
        reward: crate::processor::model::RedemptionReward {
            id,
            title: reward.twitch_title,
            cost: 0,
            prompt: None,
        },
        redeemed_at: chrono::Utc::now(),
    };
    // The common domain pipeline performs chat requirements, atomic purchase-limit
    // admission, item selection, inventory creation and market validation. No EventSub
    // notification or Twitch redemption is constructed or sent.
    crate::processor::redemption::process_fulfillment(
        state.clone(),
        input,
        false,
        Some(attr.clone()),
    )
    .await;
    let row = state
        .db
        .get_redemption(fulfillment)
        .await
        .map_err(db)?
        .ok_or("service_unavailable")?;
    audit_script(
        state,
        attr,
        "reward.script_trigger",
        json!({"actor_type":"script","script":attr,"fulfillment_id":fulfillment,"reward_id":id,"reward_alias":alias,"user_id":user}),
    ).await;
    if let Some(cause) = row.fail_cause {
        let code = match cause.as_str() {
            "chat_requirements_unmet" => "activity_requirement_failed",
            "global_limit_reached" | "user_limit_reached" => "purchase_limit_reached",
            _ => cause.as_str(),
        };
        return Ok(json!({"ok":false,"code":code,"fulfillment_id":fulfillment,"origin":"SCRIPT"}));
    }
    let item: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id,item_name,lifecycle_status FROM inventory_items WHERE redemption_id=$1",
    )
    .bind(fulfillment)
    .fetch_optional(state.db.pool())
    .await
    .map_err(db)?;
    Ok(match item {
        Some((id, name, status)) => {
            let code = match status.as_str() {
                "TRADE_LINK_REQUIRED" => Some("trade_link_required"),
                "INSUFFICIENT_FUNDS" => Some("market_unavailable"),
                "RETRY_AVAILABLE" => Some("market_rejected"),
                "RECONCILIATION_REQUIRED" => Some("fulfillment_pending"),
                _ => None,
            };
            json!({"ok":code.is_none(),"code":code,"fulfillment_id":fulfillment,"inventory_item_id":id,"selected_item":name,"origin":"SCRIPT","inventory_status":status})
        }
        None => {
            json!({"ok":false,"code":"fulfillment_pending","fulfillment_id":fulfillment,"origin":"SCRIPT"})
        }
    })
}

/// Domain mutation shared with scripting; visibility does not alter pause/business state.
pub async fn mutate_reward(
    state: &Arc<AppState>,
    channel: &str,
    id: Uuid,
    visible: Option<bool>,
    paused: Option<bool>,
) -> Result<(), String> {
    let channel_owned = channel.to_owned();
    let s = state.clone();
    state
        .with_broadcaster_token(channel, move |token| {
            let channel = channel_owned.clone();
            let s = s.clone();
            async move {
                s.helix_client
                    .update_custom_reward(
                        &channel,
                        &id.to_string(),
                        crate::helix::api::custom_rewards::model::UpdateCustomReward {
                            is_visible: visible,
                            is_paused: paused,
                            ..Default::default()
                        },
                        &token,
                    )
                    .await
            }
        })
        .await
        .map_err(|_| "twitch_unavailable")?;
    if let Some(visible) = visible {
        sqlx::query("UPDATE rewards SET is_visible=$2,updated_at=now() WHERE twitch_id=$1 AND streamer_id=$3").bind(id).bind(visible).bind(channel).execute(state.db.pool()).await.map_err(|_|"service_unavailable")?;
    }
    if let Some(paused) = paused {
        state
            .db
            .set_reward_paused(
                id,
                paused,
                if paused {
                    Some(crate::db::rewards::PauseReason::Manual)
                } else {
                    None
                },
            )
            .await
            .map_err(|_| "service_unavailable")?;
    }
    Ok(())
}
