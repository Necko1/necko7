//! Stable, transport-independent observations. None means unknown, never zero/false.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $wire)] $variant),+ }
    };
}
vocabulary!(Team { Ct => "ct", T => "t" });
vocabulary!(Activity { Menu => "menu", Playing => "playing", TextInput => "text_input" });
vocabulary!(MatchPhase { Warmup => "warmup", Live => "live", Intermission => "intermission", GameOver => "game_over" });
vocabulary!(RoundPhase { FreezeTime => "freeze_time", Live => "live", Over => "over" });
vocabulary!(WeaponStatus { Active => "active", Holstered => "holstered", Reloading => "reloading" });
vocabulary!(Identity { Local => "local", Spectator => "spectator", Unknown => "unknown" });
vocabulary!(Effect { Flash => "flash", Smoke => "smoke", Burning => "burning" });
vocabulary!(Stat { Kills => "kills", Deaths => "deaths", Assists => "assists", Mvps => "mvps", Score => "score", RoundKills => "round_kills", RoundHeadshotKills => "round_headshot_kills" });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Game {
    pub local_steam_id: Option<String>,
    pub app_id: Option<u32>,
    pub build: Option<u32>,
    pub name: Option<String>,
    pub observed_at: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    pub identity: Identity,
    pub steam_id: Option<String>,
    pub display_name: Option<String>,
    pub clan: Option<String>,
    pub observer_slot: Option<u32>,
    pub activity: Option<Activity>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Score {
    pub ct: Option<u32>,
    pub t: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TeamInfo {
    pub consecutive_losses: Option<u32>,
    pub series_wins: Option<u32>,
    pub timeouts_remaining: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchState {
    pub map: String,
    pub mode: Option<String>,
    pub phase: Option<MatchPhase>,
    pub score: Score,
    pub ct: TeamInfo,
    pub t: TeamInfo,
    pub series_matches_to_win: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Round {
    /// Observed map counter: increments at round END in this capture.
    pub completed_rounds: Option<u32>,
    pub phase: Option<RoundPhase>,
    pub winner: Option<Team>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct MatchStats {
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    pub assists: Option<u32>,
    pub mvps: Option<u32>,
    pub score: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct RoundStats {
    pub kills: Option<u32>,
    pub headshot_kills: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Ammo {
    pub clip: Option<u32>,
    pub capacity: Option<u32>,
    /// Reported units. CS2 capture has Glock reserve 3 -> 2 during reload.
    pub reserve: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct WeaponIdentity {
    pub name: String,
    pub category: String,
    pub finish: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Weapon {
    pub identity: WeaponIdentity,
    pub status: WeaponStatus,
    pub ammo: Ammo,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Player {
    pub steam_id: String,
    pub team: Option<Team>,
    pub health: Option<u32>,
    pub armor: Option<u32>,
    pub helmet: Option<bool>,
    pub defuse_kit: Option<bool>,
    pub money: Option<u32>,
    pub equipment_value: Option<u32>,
    pub flash: Option<u32>,
    pub smoke: Option<u32>,
    pub burning: Option<u32>,
    pub match_stats: MatchStats,
    pub round_stats: RoundStats,
    /// Observed inventory only. No assertion that absent entries were dropped.
    pub weapons: Option<Vec<Weapon>>,
}
impl Player {
    pub fn active_weapon(&self) -> Option<&Weapon> {
        let mut selected = self
            .weapons
            .as_ref()?
            .iter()
            .filter(|w| w.status != WeaponStatus::Holstered);
        let first = selected.next()?;
        selected.next().is_none().then_some(first)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cs2State {
    pub game: Game,
    pub view: View,
    pub r#match: Option<MatchState>,
    pub round: Round,
    /// Never contains spectator-target stats. None if identity is not proven.
    pub player: Option<Player>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub device_id: Uuid,
    pub channel_id: String,
    pub session_id: Uuid,
    pub source_seq: i64,
    /// Signed envelope observation time, NOT exact game-event occurrence time.
    pub timestamp: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change<T> {
    pub previous: T,
    pub current: T,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NumberChange {
    pub previous: u32,
    pub current: u32,
    pub delta: i64,
}
impl NumberChange {
    pub fn between(previous: Option<u32>, current: Option<u32>) -> Option<Self> {
        let (previous, current) = (previous?, current?);
        (previous != current).then_some(Self {
            previous,
            current,
            delta: i64::from(current) - i64::from(previous),
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatChange {
    pub stat: Stat,
    #[serde(flatten)]
    pub change: NumberChange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    MapChanged {
        change: Change<String>,
    },
    MapPhaseChanged {
        change: Change<MatchPhase>,
    },
    MatchStarted,
    MatchEnded,
    RoundPhaseChanged {
        change: Change<RoundPhase>,
    },
    RoundStarted,
    RoundEnded,
    ScoreChanged {
        ct: Option<NumberChange>,
        t: Option<NumberChange>,
    },
    ActivityChanged {
        change: Change<Activity>,
    },
    TeamChanged {
        change: Change<Team>,
    },
    HealthChanged {
        change: NumberChange,
    },
    ArmorChanged {
        change: NumberChange,
    },
    HelmetChanged {
        change: Change<bool>,
    },
    DefuseKitChanged {
        change: Change<bool>,
    },
    MoneyChanged {
        change: NumberChange,
    },
    EquipmentValueChanged {
        change: NumberChange,
    },
    ExposureChanged {
        effect: Effect,
        change: NumberChange,
    },
    MatchStatsChanged {
        changes: Vec<StatChange>,
    },
    PlayerKill {
        count: u32,
        total: u32,
    },
    PlayerDied {
        total: u32,
    },
    RoundStatsChanged {
        changes: Vec<StatChange>,
    },
    WeaponChanged {
        change: Change<WeaponIdentity>,
    },
    WeaponStateChanged {
        weapon: WeaponIdentity,
        change: Change<WeaponStatus>,
    },
    AmmoChanged {
        weapon: WeaponIdentity,
        clip: Option<NumberChange>,
        capacity: Option<NumberChange>,
        reserve: Option<NumberChange>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct PlayerContext {
    pub steam_id: String,
    pub team: Option<Team>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub event: EventKind,
    pub reliability: Reliability,
    pub player: Option<PlayerContext>,
    pub r#match: Option<MatchState>,
    pub round: Round,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Reliability {
    Reliable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetReason {
    Initial,
    SessionChanged,
    ObservationGap,
    ClockRegression,
    ProviderChanged,
    MapContextChanged,
    MatchRestarted,
    RoundBoundary,
    LocalIdentityLost,
    LocalIdentityRestored,
    TeamChanged,
}
#[derive(Debug, Clone, Serialize)]
pub struct Transition {
    pub resets: Vec<ResetReason>,
    pub previous: Option<Cs2State>,
    pub current: Cs2State,
    pub events: Vec<Event>,
}
