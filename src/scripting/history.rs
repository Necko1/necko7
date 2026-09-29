//! Retained execution history. All predicates run in PostgreSQL before pagination.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Default, Clone, Deserialize, Serialize)]
pub struct Filter {
    #[serde(default)]
    pub search: String,
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub source: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    pub job_id: Option<Uuid>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}
fn default_mode() -> String {
    "noteworthy".into()
}
#[derive(Deserialize, Serialize)]
struct Cursor {
    upper: DateTime<Utc>,
    before: Option<DateTime<Utc>>,
    kind: i32,
    id: Uuid,
    filter: String,
}
impl Filter {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.search.chars().count() > 128 {
            return Err("Execution search is limited to 128 characters");
        }
        for (value, allowed) in [
            (
                self.status.as_str(),
                &[
                    "",
                    "queued",
                    "running",
                    "success",
                    "failed",
                    "interrupted",
                    "skipped",
                ][..],
            ),
            (
                self.level.as_str(),
                &["", "debug", "info", "warn", "error"][..],
            ),
            (
                self.source.as_str(),
                &["", "cs2", "timer", "validate", "publish", "dry_run"][..],
            ),
            (self.mode.as_str(), &["", "noteworthy", "all"][..]),
        ] {
            if !allowed.contains(&value) {
                return Err("Invalid execution history filter");
            }
        }
        if self.limit.is_some_and(|n| !(1..=100).contains(&n)) {
            return Err("Execution page size must be 1–100");
        }
        self.decode_cursor()?;
        Ok(())
    }
    fn key(&self) -> String {
        json!([
            self.search.trim(),
            self.project_id,
            self.status,
            self.level,
            self.source,
            self.mode,
            self.job_id
        ])
        .to_string()
    }
    fn decode_cursor(&self) -> Result<Option<Cursor>, &'static str> {
        let Some(token) = &self.cursor else {
            return Ok(None);
        };
        if token.len() > 2048 {
            return Err("Invalid execution history cursor");
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| "Invalid execution history cursor")?;
        let cursor: Cursor =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid execution history cursor")?;
        if cursor.filter != self.key()
            || !(0..=1).contains(&cursor.kind)
            || cursor.before.is_some_and(|before| before > cursor.upper)
        {
            return Err("Execution cursor does not match these filters");
        }
        Ok(Some(cursor))
    }
}

// Runtime and editor records share one ordering and one limit, including timestamp ties.
const FILTERED: &str = r#"
WITH history AS (
 SELECT e.created_at, 0::int AS kind, e.id, e.project_id, e.job_id, e.source, e.status,
        e.report, p.name,
        to_jsonb(e) || jsonb_build_object('event',s.context->'events'->e.event_index,
          'source_meta',s.context->'source') AS payload
 FROM script_executions e JOIN script_projects p ON p.id=e.project_id
 LEFT JOIN script_snapshots s ON s.id=e.snapshot_id WHERE p.channel_id=$1
 UNION ALL
 SELECT e.created_at, 1::int, e.id, e.project_id, NULL::uuid, e.source, e.status,
        e.report, p.name, to_jsonb(e)
 FROM script_editor_reports e JOIN script_projects p ON p.id=e.project_id WHERE p.channel_id=$1
), filtered AS (
 SELECT * FROM history WHERE created_at <= $2
 AND ($3::uuid IS NULL OR project_id=$3) AND ($4='' OR status=$4)
 AND ($5='' OR source=$5) AND ($6::uuid IS NULL OR job_id=$6)
 AND ($7='' OR EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(report->'logs','[]')) l WHERE l->>'level'=$7)
      OR ($7='error' AND (report->>'error' IS NOT NULL OR status IN ('failed','interrupted')
          OR EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(report->'actions','[]')) a
                    WHERE a->>'error' IS NOT NULL OR a->'result'->>'ok'='false'))))
 AND ($8='all' OR source<>'cs2' OR status IN ('failed','interrupted','skipped')
      OR report->>'error' IS NOT NULL
      OR jsonb_array_length(COALESCE(report->'logs','[]'))>0
      OR EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(report->'actions','[]')) a
         WHERE a->>'error' IS NOT NULL OR a->'result'->>'ok'='false'
            OR COALESCE(a->>'method','') NOT IN ('storage.get','scheduler.exists','rewards.get',
               'chat.user_stats','chat.recent_chatters','users.recent_chatters','random.pick')))
 AND ($9='' OR strpos(lower(payload::text || name),lower($9))>0)
)
"#;

pub async fn page(
    pool: &sqlx::PgPool,
    channel: &str,
    filter: &Filter,
) -> Result<Value, sqlx::Error> {
    let cursor = filter
        .decode_cursor()
        .map_err(|e| sqlx::Error::Protocol(e.into()))?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let upper = match &cursor {
        Some(c) => c.upper,
        None => {
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
                .fetch_one(&mut *tx)
                .await?
        }
    };
    let size = filter.limit.unwrap_or(50);
    let mut count_sql = sqlx::QueryBuilder::<sqlx::Postgres>::new(FILTERED);
    count_sql.push("SELECT count(*) FROM filtered");
    let total: i64 = count_sql
        .build_query_scalar()
        .bind(channel)
        .bind(upper)
        .bind(filter.project_id)
        .bind(&filter.status)
        .bind(&filter.source)
        .bind(filter.job_id)
        .bind(&filter.level)
        .bind(&filter.mode)
        .bind(filter.search.trim())
        .fetch_one(&mut *tx)
        .await?;
    let mut rows_sql = sqlx::QueryBuilder::<sqlx::Postgres>::new(FILTERED);
    rows_sql.push("SELECT payload,created_at,kind,id FROM filtered WHERE ($10::timestamptz IS NULL OR (created_at,kind,id)<($10,$11,$12)) ORDER BY created_at DESC,kind DESC,id DESC LIMIT $13");
    let mut rows: Vec<(Value, DateTime<Utc>, i32, Uuid)> = rows_sql
        .build_query_as()
        .bind(channel)
        .bind(upper)
        .bind(filter.project_id)
        .bind(&filter.status)
        .bind(&filter.source)
        .bind(filter.job_id)
        .bind(&filter.level)
        .bind(&filter.mode)
        .bind(filter.search.trim())
        .bind(cursor.as_ref().and_then(|c| c.before))
        .bind(cursor.as_ref().map(|c| c.kind))
        .bind(cursor.as_ref().map(|c| c.id))
        .bind(size + 1)
        .fetch_all(&mut *tx)
        .await?;
    let more = rows.len() > size as usize;
    rows.truncate(size as usize);
    let next = if more {
        rows.last().map(|(_, before, kind, id)| {
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&Cursor {
                    upper,
                    before: Some(*before),
                    kind: *kind,
                    id: *id,
                    filter: filter.key(),
                })
                .expect("cursor serializes"),
            )
        })
    } else {
        None
    };
    tx.commit().await?;
    let start = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&Cursor {
            upper,
            before: None,
            kind: 0,
            id: Uuid::nil(),
            filter: filter.key(),
        })
        .expect("cursor serializes"),
    );
    Ok(
        json!({"executions": rows.into_iter().map(|r| r.0).collect::<Vec<_>>(), "total": total,
        "next_cursor": next, "start_cursor": start, "limit": size, "retained_days": 30}),
    )
}

pub async fn jobs(pool: &sqlx::PgPool, channel: &str) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(r#"SELECT to_jsonb(j) || jsonb_build_object('last_execution',
       (SELECT jsonb_build_object('id',e.id,'status',e.status,'created_at',e.created_at,'finished_at',e.finished_at)
        FROM script_executions e WHERE e.job_id=j.id ORDER BY e.sequence DESC LIMIT 1))
       FROM script_jobs j JOIN script_projects p ON p.id=j.project_id WHERE p.channel_id=$1
       ORDER BY j.created_at DESC,j.id DESC LIMIT 500"#).bind(channel).fetch_all(pool).await
}
