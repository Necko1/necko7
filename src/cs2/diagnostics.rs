use super::model::{EventKind, MatchPhase, Source, Transition};
use serde_json::Value;

pub fn payload_issues(payload: &Value) -> Vec<&'static str> {
    let mut issues = Vec::new();
    for pointer in [
        "/provider/appid",
        "/provider/version",
        "/map/round",
        "/map/team_ct/score",
        "/map/team_t/score",
        "/player/state/health",
        "/player/state/armor",
        "/player/state/money",
        "/player/state/equip_value",
        "/player/state/flashed",
        "/player/state/smoked",
        "/player/state/burning",
        "/player/state/round_kills",
        "/player/state/round_killhs",
        "/player/match_stats/kills",
        "/player/match_stats/deaths",
        "/player/match_stats/assists",
        "/player/match_stats/mvps",
        "/player/match_stats/score",
    ] {
        if let Some(value) = payload.pointer(pointer)
            && !value.is_null()
            && value.as_u64().is_none_or(|value| value > 1_000_000)
        {
            issues.push(pointer);
        }
    }
    if let Some(value) = payload.pointer("/provider/timestamp")
        && !value.is_null()
        && value.as_i64().is_none_or(|value| value <= 0)
    {
        issues.push("/provider/timestamp");
    }
    for pointer in ["/player/state/helmet", "/player/state/defusekit"] {
        if let Some(value) = payload.pointer(pointer)
            && !value.is_null()
            && !value.is_boolean()
        {
            issues.push(pointer);
        }
    }
    for (pointer, allowed) in [
        (
            "/map/phase",
            &["warmup", "live", "intermission", "gameover"][..],
        ),
        ("/round/phase", &["freezetime", "live", "over"][..]),
    ] {
        if let Some(value) = payload.pointer(pointer)
            && !value.is_null()
            && value.as_str().is_none_or(|s| !allowed.contains(&s))
        {
            issues.push(pointer);
        }
    }
    issues
}

pub fn transition_issues(t: &Transition) -> Vec<&'static str> {
    let mut issues = Vec::new();
    if let Some(old) = &t.previous
        && let Some((before, after)) = old
            .round
            .completed_rounds
            .zip(t.current.round.completed_rounds)
        && old
            .r#match
            .as_ref()
            .is_some_and(|m| m.phase == Some(MatchPhase::Live))
        && old.r#match.as_ref().map(|m| &m.map) == t.current.r#match.as_ref().map(|m| &m.map)
        && after > before
    {
        if after > before + 1 {
            issues.push("completed_rounds_jump");
        }
        // An already observed 'over' may precede the increment in the next payload.
        if old.round.phase != Some(super::model::RoundPhase::Over)
            && !t
                .events
                .iter()
                .any(|e| matches!(e.event, EventKind::RoundEnded))
        {
            issues.push("completed_rounds_without_round_end");
        }
    }
    issues
}

pub fn log_diagnostic(source: &Source, t: &Transition, stage: &str, reason: &str) {
    let m = t.current.r#match.as_ref();
    tracing::warn!(channel_id=%source.channel_id, device_id=%source.device_id,
        session_id=%source.session_id, seq=source.source_seq, timestamp=%source.timestamp,
        provider_timestamp=?t.current.game.observed_at, map=?m.map(|m| &m.map),
        mode=?m.and_then(|m| m.mode.as_ref()), phase=?m.and_then(|m| m.phase),
        completed_rounds=?t.current.round.completed_rounds, stage, reason,
        "CS2 pipeline diagnostic");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_fields_are_diagnosed_but_sparse_payloads_are_not_errors() {
        assert!(payload_issues(&serde_json::json!({})).is_empty());
        assert_eq!(
            payload_issues(&serde_json::json!({"map":{"round":"eight"},"round":{"phase":false}})),
            ["/map/round", "/round/phase"]
        );
        assert_eq!(
            payload_issues(
                &serde_json::json!({"map":{"round":1000001},"provider":{"timestamp":0},"player":{"state":{"helmet":"yes"}}})
            ),
            ["/map/round", "/provider/timestamp", "/player/state/helmet"]
        );
    }
}
