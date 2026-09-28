use super::{model::*, parse};
use serde_json::Value;

const MAX_GAP_SECONDS: i64 = 90;

#[derive(Default)]
pub struct Normalizer {
    previous: Option<(Source, Cs2State)>,
}
fn change<T: PartialEq + Clone>(previous: &Option<T>, current: &Option<T>) -> Option<Change<T>> {
    let (previous, current) = (previous.as_ref()?, current.as_ref()?);
    (previous != current).then(|| Change {
        previous: previous.clone(),
        current: current.clone(),
    })
}
fn stat(changes: &mut Vec<StatChange>, stat: Stat, old: Option<u32>, new: Option<u32>) {
    if let Some(change) = NumberChange::between(old, new) {
        changes.push(StatChange { stat, change });
    }
}
impl Normalizer {
    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Option<(Source, Cs2State)> {
        self.previous.clone()
    }

    pub fn apply(&mut self, source: Source, payload: &Value) -> Transition {
        if let Some((old_source, old)) = &self.previous
            && source.session_id == old_source.session_id
            && source.source_seq <= old_source.source_seq
        {
            return Transition {
                resets: vec![],
                previous: Some(old.clone()),
                current: old.clone(),
                events: vec![],
            };
        }
        let current = parse::normalize(payload);
        let mut resets = Vec::new();
        let mut kinds = Vec::new();
        let previous = self.previous.as_ref().map(|(_, s)| s.clone());
        if let Some((old_source, old)) = &self.previous {
            if source.session_id == old_source.session_id
                && source.source_seq > old_source.source_seq + 1
            {
                tracing::warn!(channel_id=%source.channel_id, device_id=%source.device_id, session_id=%source.session_id,
                    previous_seq=old_source.source_seq, seq=source.source_seq, stage="receive", reason="source_sequence_gap",
                    "CS2 source messages were not received continuously");
            }
            if source.session_id != old_source.session_id {
                resets.push(ResetReason::SessionChanged);
            }
            let elapsed = (source.timestamp - old_source.timestamp).num_seconds();
            let game_elapsed = old
                .game
                .observed_at
                .zip(current.game.observed_at)
                .map(|(a, b)| b.saturating_sub(a));
            if elapsed > MAX_GAP_SECONDS || game_elapsed.is_some_and(|n| n > MAX_GAP_SECONDS) {
                resets.push(ResetReason::ObservationGap);
            }
            if elapsed < 0 || game_elapsed.is_some_and(|n| n < 0) {
                resets.push(ResetReason::ClockRegression);
            }
            if current.game.local_steam_id.is_none()
                || current.game.local_steam_id != old.game.local_steam_id
                || current.game.app_id != Some(730)
                || old.game.app_id != Some(730)
                || old.game.build != current.game.build
            {
                resets.push(ResetReason::ProviderChanged);
            }
            if resets.is_empty() {
                derive(old, &current, &mut resets, &mut kinds);
            }
        } else {
            resets.push(ResetReason::Initial);
        }
        let events = kinds
            .into_iter()
            .map(|event| Event {
                source: source.clone(),
                event,
                reliability: Reliability::Reliable,
                player: current.player.as_ref().map(|p| PlayerContext {
                    steam_id: p.steam_id.clone(),
                    team: p.team,
                }),
                r#match: current.r#match.clone(),
                round: current.round.clone(),
            })
            .collect();
        self.previous = Some((source, current.clone()));
        Transition {
            resets,
            previous,
            current,
            events,
        }
    }
}
fn derive(old: &Cs2State, new: &Cs2State, resets: &mut Vec<ResetReason>, out: &mut Vec<EventKind>) {
    let same_map = match (&old.r#match, &new.r#match) {
        (Some(a), Some(b)) => {
            a.map == b.map
                && a.mode.is_some()
                && a.mode == b.mode
                && a.phase.is_some()
                && b.phase.is_some()
        }
        _ => false,
    };
    if !same_map && old.r#match != new.r#match {
        resets.push(ResetReason::MapContextChanged);
        if let (Some(a), Some(b)) = (&old.r#match, &new.r#match)
            && a.map != b.map
        {
            out.push(EventKind::MapChanged {
                change: Change {
                    previous: a.map.clone(),
                    current: b.map.clone(),
                },
            });
        }
    }
    let mut restarted = false;
    let mut starting_match = false;
    if same_map {
        let a = old.r#match.as_ref().unwrap();
        let b = new.r#match.as_ref().unwrap();
        starting_match = a.phase == Some(MatchPhase::Warmup) && b.phase == Some(MatchPhase::Live);
        restarted = (a.phase != b.phase
            && (b.phase == Some(MatchPhase::Warmup) || a.phase == Some(MatchPhase::GameOver)))
            || old
                .round
                .completed_rounds
                .zip(new.round.completed_rounds)
                .is_some_and(|(a, b)| b < a);
        if restarted {
            resets.push(ResetReason::MatchRestarted);
        }
        if let Some(change) = change(&a.phase, &b.phase) {
            out.push(EventKind::MapPhaseChanged { change });
            if a.phase == Some(MatchPhase::Warmup) && b.phase == Some(MatchPhase::Live) {
                out.push(EventKind::MatchStarted);
            }
            if a.phase == Some(MatchPhase::Live) && b.phase == Some(MatchPhase::GameOver) {
                out.push(EventKind::MatchEnded);
            }
        }
        if !restarted && !starting_match {
            if let Some(change) = change(&old.round.phase, &new.round.phase) {
                out.push(EventKind::RoundPhaseChanged { change });
                if a.phase == Some(MatchPhase::Live)
                    && b.phase == Some(MatchPhase::Live)
                    && old.round.phase == Some(RoundPhase::FreezeTime)
                    && new.round.phase == Some(RoundPhase::Live)
                    && old
                        .round
                        .completed_rounds
                        .zip(new.round.completed_rounds)
                        .is_some_and(|(a, b)| a == b)
                {
                    out.push(EventKind::RoundStarted);
                }
            }
            // The final round of a half reports intermission and over in
            // the same snapshot, including its last kill and score update.
            let observed_over = new.round.phase == Some(RoundPhase::Over)
                && old
                    .round
                    .completed_rounds
                    .zip(new.round.completed_rounds)
                    .is_some_and(|(a, b)| b == a || b == a + 1);
            // Valve can finish the last live round directly in gameover/freezetime.
            let final_advance = b.phase == Some(MatchPhase::GameOver)
                && old
                    .round
                    .completed_rounds
                    .zip(new.round.completed_rounds)
                    .is_some_and(|(a, b)| b == a + 1);
            if a.phase == Some(MatchPhase::Live)
                && matches!(
                    b.phase,
                    Some(MatchPhase::Live | MatchPhase::Intermission | MatchPhase::GameOver)
                )
                && old.round.phase == Some(RoundPhase::Live)
                && (observed_over || final_advance)
            {
                out.push(EventKind::RoundEnded);
            }
            let ct = NumberChange::between(a.score.ct, b.score.ct);
            let t = NumberChange::between(a.score.t, b.score.t);
            if ct.is_some() || t.is_some() {
                out.push(EventKind::ScoreChanged { ct, t });
            }
        }
    }
    let local = match (&old.player, &new.player) {
        (Some(a), Some(b)) if a.steam_id == b.steam_id => Some((a, b)),
        _ => None,
    };
    if old.player.is_some() && new.player.is_none() {
        resets.push(ResetReason::LocalIdentityLost);
    }
    if old.player.is_none() && new.player.is_some() {
        resets.push(ResetReason::LocalIdentityRestored);
    }
    if let Some((a, b)) = local {
        if let Some(change) = change(&old.view.activity, &new.view.activity) {
            out.push(EventKind::ActivityChanged { change });
        }
        if !same_map || restarted || starting_match {
            return;
        }
        if let Some(change) = change(&a.team, &b.team) {
            out.push(EventKind::TeamChanged { change });
            resets.push(ResetReason::TeamChanged);
            return;
        }
        // The end-of-round increment still belongs to the ending round. A new
        // freezetime (or a skipped boundary) cannot compare health/ammo/counters.
        let same_round = match (
            old.round.completed_rounds,
            new.round.completed_rounds,
            old.round.phase,
            new.round.phase,
        ) {
            (Some(a), Some(b), Some(p), Some(q)) if a == b => {
                !(q == RoundPhase::FreezeTime && p != RoundPhase::FreezeTime)
                    && !(p == RoundPhase::Over && q != RoundPhase::Over)
            }
            (Some(a), Some(b), Some(RoundPhase::Live), Some(q)) if b == a + 1 => {
                q == RoundPhase::Over
                    || new
                        .r#match
                        .as_ref()
                        .is_some_and(|m| m.phase == Some(MatchPhase::GameOver))
            }
            _ => false,
        };
        if !same_round {
            resets.push(ResetReason::RoundBoundary);
            return;
        }
        player_events(a, b, out);
    }
}
fn player_events(a: &Player, b: &Player, out: &mut Vec<EventKind>) {
    if let Some(change) = NumberChange::between(a.health, b.health) {
        out.push(EventKind::HealthChanged { change });
    }
    if let Some(change) = NumberChange::between(a.armor, b.armor) {
        out.push(EventKind::ArmorChanged { change });
    }
    if let Some(change) = change(&a.helmet, &b.helmet) {
        out.push(EventKind::HelmetChanged { change });
    }
    if let Some(change) = change(&a.defuse_kit, &b.defuse_kit) {
        out.push(EventKind::DefuseKitChanged { change });
    }
    if let Some(change) = NumberChange::between(a.money, b.money) {
        out.push(EventKind::MoneyChanged { change });
    }
    if let Some(change) = NumberChange::between(a.equipment_value, b.equipment_value) {
        out.push(EventKind::EquipmentValueChanged { change });
    }
    for (effect, old, new) in [
        (Effect::Flash, a.flash, b.flash),
        (Effect::Smoke, a.smoke, b.smoke),
        (Effect::Burning, a.burning, b.burning),
    ] {
        if let Some(change) = NumberChange::between(old, new) {
            out.push(EventKind::ExposureChanged { effect, change });
        }
    }
    let mut changes = Vec::new();
    for (name, old, new) in [
        (Stat::Kills, a.match_stats.kills, b.match_stats.kills),
        (Stat::Deaths, a.match_stats.deaths, b.match_stats.deaths),
        (Stat::Assists, a.match_stats.assists, b.match_stats.assists),
        (Stat::Mvps, a.match_stats.mvps, b.match_stats.mvps),
        (Stat::Score, a.match_stats.score, b.match_stats.score),
    ] {
        stat(&mut changes, name, old, new);
    }
    let correction = changes.iter().any(|c| c.change.delta < 0);
    if !changes.is_empty() {
        out.push(EventKind::MatchStatsChanged { changes });
    }
    if !correction {
        if let Some(change) =
            NumberChange::between(a.match_stats.kills, b.match_stats.kills).filter(|c| c.delta > 0)
        {
            out.push(EventKind::PlayerKill {
                count: change.current - change.previous,
                total: change.current,
            });
        }
        if a.health.is_some_and(|h| h > 0)
            && b.health == Some(0)
            && let Some(change) = NumberChange::between(a.match_stats.deaths, b.match_stats.deaths)
                .filter(|c| c.delta == 1)
        {
            out.push(EventKind::PlayerDied {
                total: change.current,
            });
        }
    }
    let mut changes = Vec::new();
    stat(
        &mut changes,
        Stat::RoundKills,
        a.round_stats.kills,
        b.round_stats.kills,
    );
    stat(
        &mut changes,
        Stat::RoundHeadshotKills,
        a.round_stats.headshot_kills,
        b.round_stats.headshot_kills,
    );
    if !changes.is_empty() {
        out.push(EventKind::RoundStatsChanged { changes });
    }
    if let (Some(a), Some(b)) = (a.active_weapon(), b.active_weapon()) {
        if a.identity.name != b.identity.name || a.identity.category != b.identity.category {
            out.push(EventKind::WeaponChanged {
                change: Change {
                    previous: a.identity.clone(),
                    current: b.identity.clone(),
                },
            });
        } else if a.identity.finish == b.identity.finish {
            if a.status != b.status {
                out.push(EventKind::WeaponStateChanged {
                    weapon: b.identity.clone(),
                    change: Change {
                        previous: a.status,
                        current: b.status,
                    },
                });
            }
            let clip = NumberChange::between(a.ammo.clip, b.ammo.clip);
            let capacity = NumberChange::between(a.ammo.capacity, b.ammo.capacity);
            let reserve = NumberChange::between(a.ammo.reserve, b.ammo.reserve);
            if clip.is_some() || capacity.is_some() || reserve.is_some() {
                out.push(EventKind::AmmoChanged {
                    weapon: b.identity.clone(),
                    clip,
                    capacity,
                    reserve,
                });
            }
        }
    }
}
