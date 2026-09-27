//! Raw Valve vocabulary ends here. No delta marker can supply current values.
use super::model::*;
use serde_json::Value;

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .map(str::to_owned)
}
fn number(value: &Value) -> Option<u32> {
    value.as_u64().filter(|n| *n <= 1_000_000).map(|n| n as u32)
}
fn steam_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| {
            s.len() == 17 && s.bytes().all(|b| b.is_ascii_digit()) && *s != "00000000000000000"
        })
        .map(str::to_owned)
}
fn team(value: &Value) -> Option<Team> {
    match value.as_str()? {
        "CT" => Some(Team::Ct),
        "T" => Some(Team::T),
        _ => None,
    }
}
fn activity(value: &Value) -> Option<Activity> {
    match value.as_str()? {
        "menu" => Some(Activity::Menu),
        "playing" => Some(Activity::Playing),
        "textinput" => Some(Activity::TextInput),
        _ => None,
    }
}
fn match_phase(value: &Value) -> Option<MatchPhase> {
    match value.as_str()? {
        "warmup" => Some(MatchPhase::Warmup),
        "live" => Some(MatchPhase::Live),
        "gameover" => Some(MatchPhase::GameOver),
        _ => None,
    }
}
fn round_phase(value: &Value) -> Option<RoundPhase> {
    match value.as_str()? {
        "freezetime" => Some(RoundPhase::FreezeTime),
        "live" => Some(RoundPhase::Live),
        "over" => Some(RoundPhase::Over),
        _ => None,
    }
}
fn team_info(v: &Value) -> TeamInfo {
    TeamInfo {
        consecutive_losses: number(&v["consecutive_round_losses"]),
        series_wins: number(&v["matches_won_this_series"]),
        timeouts_remaining: number(&v["timeouts_remaining"]),
    }
}
fn weapon(v: &Value) -> Option<Weapon> {
    Some(Weapon {
        identity: WeaponIdentity {
            name: text(&v["name"])?,
            category: text(&v["type"])?,
            finish: text(&v["paintkit"]),
        },
        status: match v["state"].as_str()? {
            "active" => WeaponStatus::Active,
            "holstered" => WeaponStatus::Holstered,
            "reloading" => WeaponStatus::Reloading,
            _ => return None,
        },
        ammo: Ammo {
            clip: number(&v["ammo_clip"]),
            capacity: number(&v["ammo_clip_max"]),
            reserve: number(&v["ammo_reserve"]),
        },
    })
}
fn weapons(value: &Value) -> Option<Vec<Weapon>> {
    let objects = value.as_object()?;
    if objects.len() > 64 {
        return None;
    }
    let mut weapons: Vec<Weapon> = objects.values().map(weapon).collect::<Option<_>>()?;
    // Slot names are not entity IDs; deterministic normalized order ignores slot renumbering.
    weapons.sort_by(|a, b| a.identity.cmp(&b.identity));
    Some(weapons)
}
pub fn normalize(raw: &Value) -> Cs2State {
    let provider = &raw["provider"];
    let game = Game {
        app_id: number(&provider["appid"]),
        build: number(&provider["version"]),
        name: text(&provider["name"]),
        local_steam_id: steam_id(&provider["steamid"]),
        observed_at: provider["timestamp"].as_i64().filter(|n| *n > 0),
    };
    let p = &raw["player"];
    let target = steam_id(&p["steamid"]);
    let identity = match (&game.local_steam_id, &target) {
        (Some(local), Some(target)) if game.app_id == Some(730) => {
            if local == target {
                Identity::Local
            } else {
                Identity::Spectator
            }
        }
        _ => Identity::Unknown,
    };
    let view = View {
        identity,
        steam_id: target.clone(),
        display_name: text(&p["name"]),
        clan: text(&p["clan"]),
        observer_slot: number(&p["observer_slot"]),
        activity: activity(&p["activity"]),
    };
    let m = &raw["map"];
    let r#match = (game.app_id == Some(730))
        .then(|| text(&m["name"]))
        .flatten()
        .map(|map| MatchState {
            map,
            mode: text(&m["mode"]),
            phase: match_phase(&m["phase"]),
            score: Score {
                ct: number(&m["team_ct"]["score"]),
                t: number(&m["team_t"]["score"]),
            },
            ct: team_info(&m["team_ct"]),
            t: team_info(&m["team_t"]),
            series_matches_to_win: number(&m["num_matches_to_win_series"]),
        });
    let round = if r#match.is_some() {
        Round {
            completed_rounds: number(&m["round"]),
            phase: round_phase(&raw["round"]["phase"]),
            winner: team(&raw["round"]["win_team"]),
        }
    } else {
        Round::default()
    };
    let player = if identity == Identity::Local {
        let s = &p["state"];
        let stats = &p["match_stats"];
        Some(Player {
            steam_id: target.unwrap_or_default(),
            team: team(&p["team"]),
            health: number(&s["health"]),
            armor: number(&s["armor"]),
            helmet: s["helmet"].as_bool(),
            defuse_kit: s["defusekit"].as_bool(),
            money: number(&s["money"]),
            equipment_value: number(&s["equip_value"]),
            flash: number(&s["flashed"]),
            smoke: number(&s["smoked"]),
            burning: number(&s["burning"]),
            match_stats: MatchStats {
                kills: number(&stats["kills"]),
                deaths: number(&stats["deaths"]),
                assists: number(&stats["assists"]),
                mvps: number(&stats["mvps"]),
                score: number(&stats["score"]),
            },
            round_stats: RoundStats {
                kills: number(&s["round_kills"]),
                headshot_kills: number(&s["round_killhs"]),
            },
            weapons: weapons(&p["weapons"]),
        })
    } else {
        None
    };
    Cs2State {
        game,
        view,
        r#match,
        round,
        player,
    }
}
