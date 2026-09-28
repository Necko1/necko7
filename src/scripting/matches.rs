use crate::cs2::model::{EventKind, MatchPhase, ResetReason, Source, Transition};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Only consume normalized observations. Missing information remains JSON null.
pub fn advance(mut data: Value, source: &Source, t: &Transition) -> Value {
    if !data.is_object() {
        data = json!({"rounds":[],"current_round":null});
    }
    data["state"] = json!(t.current);
    data["updated_at"] = json!(source.timestamp);
    let new_round = t.resets.iter().any(|r| {
        matches!(
            r,
            ResetReason::RoundBoundary
                | ResetReason::ObservationGap
                | ResetReason::ClockRegression
                | ResetReason::ProviderChanged
                | ResetReason::MapContextChanged
        )
    });
    if new_round {
        data["current_round"] = Value::Null;
    }
    let mut round = data["current_round"].clone();
    if round.is_null() {
        round = json!({"index": t.current.round.completed_rounds, "started_at":source.timestamp,
            "completed":false,"start":t.current.player,"score_before":t.current.r#match.as_ref().map(|m|&m.score),"events":[]});
    }
    if let Some(player) = &t.current.player {
        round["end"] = json!(player);
        round["player"] = json!({"kills":player.round_stats.kills,"headshot_kills":player.round_stats.headshot_kills,
            "side":player.team,"health":player.health,"match_stats":player.match_stats});
    }
    if t.current.player.is_none() {
        round["end"] = Value::Null;
        round["player"] = json!({"kills":null,"headshot_kills":null,"side":null,"health":null,"match_stats":null});
    }
    round["score_after"] = json!(t.current.r#match.as_ref().map(|m| &m.score));
    round["winner"] = json!(t.current.round.winner);
    if let Some(events) = round["events"].as_array_mut() {
        for event in &t.events {
            if events.len() < 512 {
                events.push(json!({"timestamp":source.timestamp,"seq":source.source_seq,"event":event.event}));
            }
        }
    }
    if t.events
        .iter()
        .any(|e| matches!(e.event, EventKind::RoundEnded))
        && round["completed"] != true
    {
        round["completed"] = json!(true);
        round["ended_at"] = json!(source.timestamp);
        let rounds = data["rounds"].as_array_mut().expect("stored round list");
        if rounds.len() < 256 {
            rounds.push(round.clone());
        }
    }
    data["current_round"] = round;
    data
}

pub async fn persist(pool: &PgPool, source: &Source, t: &Transition) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout='500ms'")
        .execute(&mut *tx)
        .await?;
    // Channel lock also serializes devices after re-pairing.
    sqlx::query("SELECT channel_id FROM broadcasters WHERE channel_id=$1 FOR UPDATE")
        .bind(&source.channel_id)
        .fetch_one(&mut *tx)
        .await?;
    let active: Option<(Uuid,Uuid,Uuid,String,Value)>=sqlx::query_as("SELECT id,device_id,session_id,map,data FROM script_matches WHERE channel_id=$1 AND completed_at IS NULL FOR UPDATE")
        .bind(&source.channel_id).fetch_optional(&mut *tx).await?;
    let mut match_data = Value::Null;
    if let Some(m) = &t.current.r#match {
        let replace = active.as_ref().is_some_and(|(_, device, session, map, _)| {
            *device != source.device_id || *session != source.session_id || map != &m.map
        }) || t.resets.contains(&ResetReason::MatchRestarted);
        if replace {
            sqlx::query("UPDATE script_matches SET completed_at=now(),data=data || '{\"end_reason\":\"observation_reset\"}'::jsonb WHERE channel_id=$1 AND completed_at IS NULL").bind(&source.channel_id).execute(&mut *tx).await?;
        }
        let (id, data) = if !replace {
            active
                .as_ref()
                .map(|(id, _, _, _, data)| (*id, data.clone()))
                .unwrap_or((Uuid::new_v4(), Value::Null))
        } else {
            (Uuid::new_v4(), Value::Null)
        };
        match_data = advance(data, source, t);
        let ended = m.phase == Some(MatchPhase::GameOver);
        // Do not create a new completed match on every game-over heartbeat.
        if active.is_some() || !ended {
            sqlx::query("INSERT INTO script_matches(id,channel_id,device_id,session_id,map,data,completed_at) VALUES($1,$2,$3,$4,$5,$6,CASE WHEN $7 THEN now() END) ON CONFLICT(id) DO UPDATE SET data=$6,completed_at=CASE WHEN $7 THEN now() END")
                .bind(id).bind(&source.channel_id).bind(source.device_id).bind(source.session_id).bind(&m.map).bind(&match_data).bind(ended).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM script_matches WHERE id IN (SELECT id FROM script_matches WHERE channel_id=$1 AND completed_at IS NOT NULL ORDER BY completed_at DESC OFFSET 30)").bind(&source.channel_id).execute(&mut *tx).await?;
    }
    if !t.events.is_empty() {
        // Event timelines already live in shared match history; omit them from execution contexts.
        if let Some(rounds) = match_data["rounds"].as_array_mut() {
            for round in rounds {
                round.as_object_mut().map(|m| m.remove("events"));
            }
        }
        if let Some(round) = match_data["current_round"].as_object_mut() {
            round.remove("events");
        }
        let context = json!({"state":t.current,"previous":t.previous,"current_match":match_data,"events":t.events,"source":source});
        let snapshot:i64=sqlx::query_scalar("INSERT INTO script_snapshots(channel_id,device_id,session_id,seq,context) VALUES($1,$2,$3,$4,$5) RETURNING id")
            .bind(&source.channel_id).bind(source.device_id).bind(source.session_id).bind(source.source_seq).bind(context).fetch_one(&mut *tx).await?;
        let projects:Vec<(Uuid,i64,i64)>=sqlx::query_as("SELECT p.id,p.active_revision,(SELECT count(*) FROM script_executions e WHERE e.project_id=p.id AND e.status='queued') FROM script_projects p JOIN script_revisions r ON r.project_id=p.id AND r.revision=p.active_revision WHERE p.channel_id=$1 AND p.enabled AND p.deleted_at IS NULL AND r.has_on_event ORDER BY p.id")
            .bind(&source.channel_id).fetch_all(&mut *tx).await?;
        for (project, revision, queued) in projects {
            let capacity = 1000usize.saturating_sub(queued as usize);
            if capacity < t.events.len() {
                tracing::warn!(%project,channel=%source.channel_id,seq=source.source_seq,"Script queue full; event delivery truncated");
            }
            for index in 0..t.events.len().min(capacity) {
                sqlx::query("INSERT INTO script_executions(id,project_id,revision,source,snapshot_id,event_index) VALUES($1,$2,$3,'cs2',$4,$5)")
                .bind(Uuid::new_v4()).bind(project).bind(revision).bind(snapshot).bind(index as i32).execute(&mut *tx).await?;
            }
        }
    }
    tx.commit().await
}
