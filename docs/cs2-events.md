# CS2 semantic events — evidence and contract

## Analysis completed before implementation

Source: the supplied `logs.txt`, 869,262 bytes, parsed in full: **127 complete accepted payloads**, seq 4–623, 2026-09-27 15:27:30–15:50:44 UTC. There are three missing ranges: 44–271, 309–523, 551–600. This is an excerpt, NOT a lossless event stream. Device/session identifiers were absent from the JSON itself; the old log did not print session_id. Fixtures use a declared synthetic session and anonymized identities, preserving equality, values, ordering, timestamps and sequence gaps.

The backend previously accepted an arbitrary JSON object after signature/timestamp/replay verification and only optionally logged it. There was no gameplay parser, normalized state or semantic event layer. The companion validates local auth, removes it, signs and forwards the latest watch-channel snapshot. Intermediate updates can be coalesced. Security and transport remain unchanged.

The checked-in companion config enables provider, map, round, player_id, player_state, player_weapons, player_match_stats. **Contrary to the task's starting assumption, it does not enable phase_countdowns. No countdown field appears anywhere in this capture.** The config is unchanged. The earlier CS2_INTEGRATION_SPEC.md is not present in this checkout; this task's explicit semantic requirements govern this pass.

### Observations that constrain the design

* provider.name still says `Counter-Strike: Global Offensive` although this is CS2; appid=730, not the legacy display name, gates parsing.
* provider.steamid stays constant. 57 payloads describe one of four different spectator targets; player is entirely absent in 3 payloads (33, 282, 307). Only 67 payloads identify the local player. The local player dies at 21, 40, 296, 544, then spectator state appears. Spectator kills at 24, 273, 299, 547 must produce no local-player events.
* `previously` is a sparse change description, not a complete previous snapshot. At 33 it is `{player:true}` (a disappearing structure). At 621 it includes `{map:true,round:true}` while current state is menu-only. It also contains removed weapon objects. It cannot establish local identity or replace server state.
* `added` contains boolean markers, not values. At 7 map appears without added.map. At 8 round appears without added.round. Returning player at 34 has no added.player marker. Do not require these markers or use them to initialize values.
* Current state is mostly full for enabled categories, but menu-only and player-absent records occur. Missing optional defusekit is not an explicit false. Empty weapons is observed on death. Absence alone cannot prove item removal/disconnection. Missing/invalid fields become unknown; they are never zero-filled or merged into a supposedly current local snapshot.
* Repeated effective menu state at 4/5/6 and 621/622/623 differs only in provider time. Weapon states and ammo can change several times in the same provider second. Do not deduplicate by provider timestamp.
* map.round increments with round.phase=over (31→32, 280→281, 305→306, 524→525); next freezetime retains that number. It is exposed as **completed_rounds**, not a globally unique round ID. Per-round baselines reset on new freezetime, not on every increment at round end.
* 7→8 is warmup→live/freezetime. 9→10, 36→37, 291→292, 538→539 are freezetime→live. At 616→617, map.phase becomes gameover, scores 2:12→2:13, but round.phase becomes **freezetime**, with win_team=T. Match end is an explicit map-phase transition, never a hardcoded score or freezetime assumption.
* CT/T scores change from 7:1 to 1:11 across the 308→524 gap, while local side changes T→CT. CT and T are side labels, not persistent team identities. This must not manufacture a comeback, score reset or team victory.
* Local match kills jump 2→10 across that gap, round kills 0→3 and headshots 0→2. No continuously observed local kill occurs in this excerpt. Spectator kills do correlate with round/headshot counters, but that is not evidence for weapon/victim attribution. Positive counter deltas can be counted only inside an established continuous local baseline.
* Smoke intensity changes 0→21→116→247 at 41–43 while local health stays **0**. Expose the reported intensity, not a claim of physical damage/exposure. Flash and burning are always zero in this sample.
* Glock reload observation 26→27 has clip 8→20 and reserve **3→2**, not a decrement of 12. Reserve units must be treated as reported, not assumed bullets. This sequence belongs to a spectator. `reloading` is an explicit weapon state, but completion, shots and physical weapon continuity remain separate claims.
* Equipment value can remain 850 at death with empty inventory (21), and remain 4100 when M4 disappears and money rises (534). Do not recompute equipment value or call these guaranteed purchases/refunds.
* No bomb state/site/owner, positions, allplayers, round-win history, countdowns, attacker/victim IDs or damage source appears.

## Candidate catalog (analysis decisions)

`IMPOSSIBLE_WITH_CURRENT_DATA` refers to fields actually present in this capture, not a universal claim about everything the configured categories might expose in other matches. Unobserved optional fields require a new capture before promotion. Classification is of the stated contract, not a promise that all real-world occurrences are observable. All implemented changes require two valid observations in the same continuity domain. `previously` alone is **insufficient for every candidate**; server-held prior state is required. Local candidates additionally require current AND previous provider/player IDs to match and the same local ID. Unknown→known establishes a baseline, not an event. Resets and gaps cause deliberate false negatives. Examples below abbreviate current-state fields; delta metadata never supplies an old baseline.

| Candidate / source | Example and transition; required baseline | Reliability; false positives / negatives; reset | Implement now |
|---|---|---|---|
| activity_changed / player.activity | playing→textinput, same local ID | RELIABLE reported UI activity, not leaving gameplay; missing identity suppresses; local reset | Yes |
| gameplay entered/left / activity + map | menu→playing or menu overlay | AMBIGUOUS: menu can overlay an active match; missing payload is not exit | No |
| map_changed / map.name | known A→known B | RELIABLE observed map identity; reconnect/same-map rematch unobservable; full reset | Yes |
| match joined/left / map presence | absent→map, map→absent | AMBIGUOUS under partial updates; state availability is sufficient | No |
| map_phase_changed / map.phase | warmup→live→gameover | RELIABLE reported phase, same map; unknown old phase suppresses | Yes |
| warmup start/end | map phase enters/leaves warmup | RELIABLE as phase change; does not prove matchmaking lifecycle | Covered by map_phase_changed |
| match_started | warmup→live | RELIABLE observed start; joining already-live misses start; match reset | Yes |
| match_ended | live→gameover | RELIABLE observed end; do not guess from score or absence | Yes |
| round_phase_changed | freezetime→live→over, same map | RELIABLE reported phase; final gameover exception; round reset | Yes |
| freeze time start/end | phase enters/leaves freezetime | RELIABLE reported phase, not round outcome | Covered by round_phase_changed |
| round_started | freezetime→live with map.phase=live | RELIABLE; initial live snapshot/gaps miss start | Yes |
| round_ended | live→over with map.phase=live | RELIABLE; gameover+freezetime intentionally emits match_ended, not guessed round_ended | Yes |
| round winner / round.win_team | absent→CT at over | RELIABLE reported winner state; repeated winners need result identity; delayed/partial reports complicate dedup | State only |
| local round win/loss | winner + local side | INFERRED if side retained while spectating; side swap / delayed winner risks attribution | No |
| score_changed / team_ct.score, team_t.score | 0:0→1:0 | RELIABLE observed side scores; neither persistent team identity nor cause; match/gap reset | Yes |
| tied/leading/trailing / scores + local side | equal→CT ahead | INFERRED local result across spectator/side changes; direct side relation can be derived from state | No separate event |
| score milestones/comeback/match winner | score thresholds + team history | AMBIGUOUS: side switches, forfeits, modes, gaps, missing stable team identity | No |
| team_changed / local player.team | T→CT | RELIABLE observed local side; gap suppresses; not a new team identity | Yes |
| player_spawned / health | 0→100 | INFERRED: respawn, reset, re-entry or resync; establish new round baseline | No |
| health_changed / state.health | 100→92 | RELIABLE reported health delta; no attacker/cause/exact hit count; local/round reset | Yes |
| damage_taken / health decrease | 92→82 | INFERRED physical cause/amount; falls, reset, heal between samples; use health_changed | No |
| armor_changed / state.armor | 100→95 | RELIABLE reported armor; death clears armor, not armor damage | Yes |
| armor_damage | armor decreases | AMBIGUOUS damage vs death/equipment/reset | No |
| helmet_changed / state.helmet | false→true | RELIABLE boolean change; missing not false | Yes |
| defuse_kit_changed / state.defusekit | explicit false→true | RELIABLE boolean observation; only true observed, so absence never emits loss | Yes, no positive live example |
| money_changed / state.money | 800→150 | RELIABLE balance delta; no cause attribution; round reset | Yes |
| equipment_value_changed / state.equip_value | 200→850 | RELIABLE reported valuation, can lag inventory | Yes |
| purchase/refund/pickup/drop | money + weapons + equipment | AMBIGUOUS: observed 532→534 refund-like but not proof | No |
| exposure_changed / flashed, smoked, burning | smoked 0→21 even at health=0 | RELIABLE reported intensity only; dead-player/camera effects allowed, not damage | Yes |
| flash/smoke/burning started/ended | intensity crosses zero | INFERRED physical exposure, especially after death; no flash/burn positive evidence | No separate event |
| match_stats_changed / kills,deaths,assists,mvps,score | MVP 0→1 at 525 | RELIABLE observed counter changes incl downward corrections; each field needs own known baseline | Yes |
| player_kill / match_stats.kills | k→k+n, n>0, continuous same local round | RELIABLE aggregate kill-counter increment; no victim, weapon, exact timing; resets/absence suppress | Yes, derived fixture required |
| player_died / health + deaths | health>0→0 AND deaths d→d+1 | RELIABLE corroborated local death; health-only or missed terminal snapshot suppressed | Yes |
| player assist / MVP award | assists/MVP increases | RELIABLE counter increment, ambiguous singular award timing/target; represented by stats change | Covered by match_stats_changed |
| personal score change | match_stats.score changes | RELIABLE numeric delta, not kill attribution | Covered by match_stats_changed |
| round_stats_changed / round_kills,round_killhs | 1→2 | RELIABLE reported counters in same round; no invented zero on missing fields | Yes |
| headshot kill | round_killhs rises with kill counter | INFERRED correlation, batched kills/identity gaps; no local contiguous example | No; retain round counter change |
| multi-kill / first local kill / ace | round_kills reaches 2/1/5 | INFERRED milestones, cannot prove opponents distinct/all dead; first kill overall unknown | No separate event |
| first death of round | health/deaths alone | IMPOSSIBLE_WITH_CURRENT_DATA for first among all players | No |
| kill/death streak | accumulated local kills/deaths | INFERRED with gaps/restarts/spectating; exact reset history not available | No |
| weapon_changed / active or reloading weapon | Glock→knife | RELIABLE selected weapon description changed; no physical entity ID | Yes |
| weapon_state_changed | active→reloading, same description | RELIABLE reported handling state, not successful reload | Yes |
| ammo_changed / clip,capacity,reserve | 20→19 | RELIABLE observed values for same selected description; units not assumed, no physical entity claim | Yes |
| shot / rounds fired | clip decrease | INFERRED: net consumption only; drops/resync/coalescing/bursts | No |
| reload completed | clip up, reserve down | INFERRED even with reloading; replacement/reset and intermediate states | No |
| inventory acquired/removed | weapon key appears/disappears | AMBIGUOUS partial maps, slot reuse, death/reset/pickup/drop; inventory state only | No |
| C4 possession changed | weapon of type C4 present | RELIABLE positive observation; absence in partial inventory not proof of loss | State only |
| health/armor/money thresholds, empty clip | known numeric crossing, e.g. health 30→20 or clip 1→0 | RELIABLE reported threshold crossing if continuous; values may skip thresholds; no physical shot/damage cause | Covered by numeric changes; threshold policy belongs to future consumers |
| double/triple kill counter milestone | round_kills 1→2/3 | RELIABLE numeric milestone only, not timing or distinct victims | Covered by round_stats_changed |
| KD/KDA/headshot ratio | known match/round counters | RELIABLE arithmetic when denominator known/nonzero; no event beyond underlying counters, unknown on missing input | State-derived only |
| spectator target changed | known player.steamid A→B, provider unchanged | RELIABLE observed target change; no local action, cannot infer target death caused switch | View state only |
| skin/name/clan/observer-slot changed | metadata fields differ | RELIABLE metadata observation, often spectator switching; not gameplay action | State only |
| timeouts/loss-bonus/series changes | team fields change | RELIABLE reported metadata, not proof timeout actually starts or payout cause | State only |
| mode/provider build/account changed | metadata differs | RELIABLE context metadata; mode/account/build change resets comparisons | State only |
| phase/countdown/low-time threshold | phase_countdowns.phase/phase_ends_in | IMPOSSIBLE_WITH_CURRENT_DATA: category absent from config/capture | No |
| exact killer/victim identity, kill weapon | local counters + selected weapon | IMPOSSIBLE_WITH_CURRENT_DATA: delayed grenades, spectator selection, multiple events | No |
| enemy/teammate alive counts, clutch | single player state | IMPOSSIBLE_WITH_CURRENT_DATA: no roster/alive snapshot | No |
| trade kill | deaths/kills timing | IMPOSSIBLE_WITH_CURRENT_DATA: no attacker/victim chain | No |
| bomb site/planter/defuser identity | C4 inventory only | IMPOSSIBLE_WITH_CURRENT_DATA: possession does not prove location/actor | No |
| bomb planted/defused/exploded | no bomb-state field in this capture | AMBIGUOUS / unvalidated: optional round bomb signals may be available in another capture with the existing round category; C4 disappearance alone proves none of these | No |
| grenade thrower/throw/detonation | local grenade inventory | IMPOSSIBLE_WITH_CURRENT_DATA for actor/action; partial possession insufficient | No |
| positions/zones/line of sight | absent spatial data | IMPOSSIBLE_WITH_CURRENT_DATA; positions alone would still not prove visibility | No |

## External documentation boundary

The Valve developer wiki GSI page was queried but returned HTTP 403 during this pass. Decisions above are supported by the capture and checked-in code, not unverified third-party schema assumptions. Future category suggestions below must be validated in player mode: observer-only information may not be exposed to a normal live client.

## Complete observed leaf-field inventory

Every leaf path found in all 127 records is listed below. `weapon_*` groups numbered entries; values of removed objects also appear under `previously`. Empty objects additionally occur at current player.weapons (17 records) and previously.player.weapons (1). No arrays, nulls, phase_countdowns, bomb, positions or allplayers were observed. Counts for weapon leaves count entries, not payloads.

| Raw field/subfield | Observations | Values / domain |
|---|---:|---|
| `added.player.clan` | 1 | anonymized identity/display metadata; not action evidence |
| `added.player.match_stats` | 1 | true |
| `added.player.observer_slot` | 1 | true |
| `added.player.state` | 1 | true |
| `added.player.state.defusekit` | 1 | true |
| `added.player.team` | 1 | true |
| `added.player.weapons` | 1 | true |
| `added.player.weapons.weapon_*` | 19 | true |
| `added.round.win_team` | 5 | true |
| `map.mode` | 121 | "competitive" |
| `map.name` | 121 | "de_inferno" |
| `map.num_matches_to_win_series` | 121 | 0…0 (1 distinct) |
| `map.phase` | 121 | "warmup", "live", "gameover" |
| `map.round` | 121 | 0…15 (9 distinct) |
| `map.team_ct.consecutive_round_losses` | 121 | 0…2 (3 distinct) |
| `map.team_ct.matches_won_this_series` | 121 | 0…0 (1 distinct) |
| `map.team_ct.score` | 121 | 0…7 (6 distinct) |
| `map.team_ct.timeouts_remaining` | 121 | 3…3 (1 distinct) |
| `map.team_t.consecutive_round_losses` | 121 | 0…6 (6 distinct) |
| `map.team_t.matches_won_this_series` | 121 | 0…0 (1 distinct) |
| `map.team_t.score` | 121 | 0…13 (5 distinct) |
| `map.team_t.timeouts_remaining` | 121 | 3…3 (1 distinct) |
| `player.activity` | 124 | "menu", "playing", "textinput" |
| `player.clan` | 17 | anonymized identity/display metadata; not action evidence |
| `player.match_stats.assists` | 118 | 0…3 (3 distinct) |
| `player.match_stats.deaths` | 118 | 0…14 (9 distinct) |
| `player.match_stats.kills` | 118 | 0…10 (8 distinct) |
| `player.match_stats.mvps` | 118 | 0…1 (2 distinct) |
| `player.match_stats.score` | 118 | 0…23 (12 distinct) |
| `player.name` | 124 | anonymized identity/display metadata; not action evidence |
| `player.observer_slot` | 118 | 0…9 (8 distinct) |
| `player.state.armor` | 118 | 0…100 (9 distinct) |
| `player.state.burning` | 118 | 0…0 (1 distinct) |
| `player.state.defusekit` | 12 | true |
| `player.state.equip_value` | 118 | 200…6300 (12 distinct) |
| `player.state.flashed` | 118 | 0…0 (1 distinct) |
| `player.state.health` | 118 | 0…100 (16 distinct) |
| `player.state.helmet` | 118 | false, true |
| `player.state.money` | 118 | 0…16000 (26 distinct) |
| `player.state.round_killhs` | 118 | 0…2 (3 distinct) |
| `player.state.round_kills` | 118 | 0…3 (4 distinct) |
| `player.state.smoked` | 118 | 0…247 (4 distinct) |
| `player.steamid` | 124 | anonymized identity/display metadata; not action evidence |
| `player.team` | 118 | "T", "CT" |
| `player.weapons.weapon_*.ammo_clip` | 152 | 6…30 (22 distinct) |
| `player.weapons.weapon_*.ammo_clip_max` | 152 | 7…30 (5 distinct) |
| `player.weapons.weapon_*.ammo_reserve` | 182 | 1…4 (4 distinct) |
| `player.weapons.weapon_*.name` | 284 | "weapon_knife_t", "weapon_glock", "weapon_knife_push", "weapon_c4", "weapon_deagle", "weapon_ak47", "weapon_knife_kukri", "weapon_knife", "weapon_usp_silencer", "weapon_m4a1", "weapon_ssg08", "weapon_m4a1_silencer", "weapon_hegrenade", "weapon_knife_canis", "weapon_incgrenade" |
| `player.weapons.weapon_*.paintkit` | 284 | "default", "am_gamma_doppler_phase3", "glock_train_green", "aq_deagle_corinthian", "soe_varicamo", "cu_glock_moon_rabbit", "sp_tape_urban", "deagle_calligraff", "am_circuitboard_silver", "ht_red_edges_fineline", "usps_bruteforce_green", "aa_fade", "hy_overpass_doodle_m4a1s", "soe_tropical" |
| `player.weapons.weapon_*.state` | 284 | "holstered", "active", "reloading" |
| `player.weapons.weapon_*.type` | 284 | "Knife", "Pistol", "C4", "Rifle", "SniperRifle", "Grenade" |
| `previously.map` | 1 | true |
| `previously.map.phase` | 2 | "warmup", "live" |
| `previously.map.round` | 5 | 0…14 (5 distinct) |
| `previously.map.team_ct.consecutive_round_losses` | 3 | 1…1 (1 distinct) |
| `previously.map.team_ct.score` | 4 | 0…6 (4 distinct) |
| `previously.map.team_t.consecutive_round_losses` | 4 | 1…5 (3 distinct) |
| `previously.map.team_t.score` | 1 | 12…12 (1 distinct) |
| `previously.player` | 3 | true |
| `previously.player.activity` | 18 | "menu", "playing", "textinput" |
| `previously.player.clan` | 1 | anonymized identity/display metadata; not action evidence |
| `previously.player.match_stats.assists` | 2 | 0…3 (2 distinct) |
| `previously.player.match_stats.deaths` | 17 | 0…14 (8 distinct) |
| `previously.player.match_stats.kills` | 11 | 0…10 (8 distinct) |
| `previously.player.match_stats.mvps` | 5 | 0…1 (2 distinct) |
| `previously.player.match_stats.score` | 11 | 0…23 (10 distinct) |
| `previously.player.name` | 7 | anonymized identity/display metadata; not action evidence |
| `previously.player.observer_slot` | 7 | 1…9 (5 distinct) |
| `previously.player.state.armor` | 21 | 0…100 (9 distinct) |
| `previously.player.state.burning` | 1 | 0…0 (1 distinct) |
| `previously.player.state.defusekit` | 2 | true |
| `previously.player.state.equip_value` | 13 | 200…6300 (8 distinct) |
| `previously.player.state.flashed` | 1 | 0…0 (1 distinct) |
| `previously.player.state.health` | 27 | 0…100 (16 distinct) |
| `previously.player.state.helmet` | 12 | true, false |
| `previously.player.state.money` | 25 | 0…16000 (20 distinct) |
| `previously.player.state.round_killhs` | 6 | 0…2 (3 distinct) |
| `previously.player.state.round_kills` | 8 | 0…3 (3 distinct) |
| `previously.player.state.smoked` | 4 | 0…116 (3 distinct) |
| `previously.player.steamid` | 7 | anonymized identity/display metadata; not action evidence |
| `previously.player.team` | 1 | "CT" |
| `previously.player.weapons.weapon_*.ammo_clip` | 48 | 6…30 (22 distinct) |
| `previously.player.weapons.weapon_*.ammo_clip_max` | 18 | 7…30 (5 distinct) |
| `previously.player.weapons.weapon_*.ammo_reserve` | 24 | 1…4 (4 distinct) |
| `previously.player.weapons.weapon_*.name` | 34 | "weapon_knife_t", "weapon_glock", "weapon_knife_push", "weapon_c4", "weapon_deagle", "weapon_ak47", "weapon_knife_kukri", "weapon_m4a1", "weapon_knife", "weapon_usp_silencer", "weapon_ssg08", "weapon_knife_canis", "weapon_m4a1_silencer", "weapon_incgrenade", "weapon_hegrenade" |
| `previously.player.weapons.weapon_*.paintkit` | 34 | "default", "am_gamma_doppler_phase3", "glock_train_green", "aq_deagle_corinthian", "soe_varicamo", "cu_glock_moon_rabbit", "sp_tape_urban", "deagle_calligraff", "am_circuitboard_silver", "ht_red_edges_fineline", "aa_fade", "hy_overpass_doodle_m4a1s", "soe_tropical" |
| `previously.player.weapons.weapon_*.state` | 92 | "holstered", "active", "reloading" |
| `previously.player.weapons.weapon_*.type` | 33 | "Knife", "Pistol", "C4", "Rifle", "SniperRifle", "Grenade" |
| `previously.round` | 1 | true |
| `previously.round.phase` | 13 | "freezetime", "live", "over" |
| `previously.round.win_team` | 4 | "CT" |
| `provider.appid` | 127 | 730…730 (1 distinct) |
| `provider.name` | 127 | "Counter-Strike: Global Offensive" |
| `provider.steamid` | 127 | one constant local Steam ID |
| `provider.timestamp` | 127 | 1790522844…1790524238 (89 distinct) |
| `provider.version` | 127 | 14185…14185 (1 distinct) |
| `round.phase` | 120 | "freezetime", "live", "over" |
| `round.win_team` | 15 | "CT", "T" |

## Normalized model and absence semantics

`src/cs2/model.rs` defines the serializable contract, independently of Axum, SQLx and Valve paths:

* `state.game`: authoritative local Steam ID, app ID, build, provider name and reported observation timestamp.
* `state.view`: observed target ID/name/clan/slot/activity and `identity: local | spectator | unknown`. This is view metadata, **not** local gameplay state.
* `state.match`: map/mode/phase, side scores (`score.ct`, `score.t`), each side's consecutive losses, series wins, remaining timeouts, series target. None means map context is unavailable.
* `state.round`: completed_rounds, phase, reported winning side. There is intentionally no invented match ID, roster, current-round ordinal or countdown. Countdown is unknown/unavailable until that category is enabled and validated.
* `state.player`: local-only team, health, armor, helmet, defuse kit, money, reported equipment value, flash/smoke/burning intensity, match statistics, round statistics, observed weapons. None for spectator/unknown identity. Weapons expose a description (name/category/finish), handling status and clip/capacity/reported reserve. Raw weapon slot keys are discarded; they are not durable entity IDs. `Player::active_weapon()` requires exactly one active/reloading entry.

All scalar observations are optional. Missing/null/wrong-type/out-of-range fields become None **on that update**. We do not silently carry a last-known value forward as current; an intervening missing observation invalidates that field's comparison baseline. For example `100 -> missing -> 80` produces no health event. `100 -> 80 -> 80` produces exactly one event. An explicit 0/false is known, not absent. Empty weapons is a known empty observed collection; omitted weapons is unknown. Neither produces a drop/acquisition claim. A partial weapon entry lacking name/category/recognized handling state makes inventory unknown, and more than 64 entries is rejected as unknown.

Strings are capped at 256 UTF-8 bytes; IDs must be nonzero 17-digit strings; numeric observations are unsigned integers <=1,000,000. Unknown phase/activity/status values remain unknown, not guessed. Only appid=730 authorizes CS2 context. No observed negative stat values exist in this capture; unexpected negatives become unknown rather than wrapping. Unknown JSON fields are ignored for normalization. Raw previously/added remain available only in sanitized diagnostic output/fixtures; neither influences comparisons.

This is deliberately conservative observation replacement, not generic JSON merge-patch semantics. It distinguishes unknown via Options, unchanged via equal known observations, changed via typed deltas, and reset via `Transition.resets`. Initial/returning values populate state without backfilling actions. Consumers must inspect these statuses rather than assume a missing event means nothing happened.

## Exact implemented event contract

All 24 event kinds are RELIABLE **under their stated observation contract**. Reliability does not imply lossless delivery, exact occurrence time, physical cause or an authoritative killfeed. Events have source timestamp/device_id/channel_id/session_id/source_seq, reliability, optional local player context, current match and round context. The timestamp is signed envelope observation time; `game.observed_at` is Valve's integer-second time. Multiple events may share it. No rewards, script runtime, durable event queue or raw-payload database is added.

`C(x,y)` below means both values are known and unequal. Numeric changes carry `{previous:x,current:y,delta:y-x}`; enum/bool/string changes carry `{previous:x,current:y}`. `local continuity` requires current and previous local identity equality, same known map/mode and known match phases, no match reset/gap, no side change and the same round comparison domain. All emitted examples below show the event-kind body; shared correlation/context fields are added by `Event`.

| Kind | Exact additional rule | Example emitted body |
|---|---|---|
| map_changed | same provider/session/time continuity, known old/new map names differ; other gameplay comparisons reset | `{kind:"map_changed",change:{previous:"de_inferno",current:"de_other"}}` |
| map_phase_changed | same map/mode, C(known map phase) | `{kind:"map_phase_changed",change:{previous:"warmup",current:"live"}}` |
| match_started | known warmup→live, same map/mode; reset gameplay baselines | `{kind:"match_started"}` |
| match_ended | known live→game_over, same map/mode; no score threshold | `{kind:"match_ended"}` |
| round_phase_changed | same map/mode, no match restart, C(known round phase) | `{kind:"round_phase_changed",change:{previous:"live",current:"over"}}` |
| round_started | both map phases live, freezetime→live, same known completed_rounds | `{kind:"round_started"}` |
| round_ended | both map phases live, live→over, completed_rounds equal or +1 | `{kind:"round_ended"}` |
| score_changed | same map/mode and no match restart; at least one side has a known changed score; compare sides independently | `{kind:"score_changed",ct:{previous:0,current:1,delta:1},t:null}` |
| activity_changed | both player IDs proven local, same provider/session/time; C(activity); may occur on map entry; not gameplay exit | `{kind:"activity_changed",change:{previous:"playing",current:"text_input"}}` |
| team_changed | both IDs local, same map/mode, no match restart; C(team); suppress other local comparisons on that payload | `{kind:"team_changed",change:{previous:"t",current:"ct"}}` |
| health_changed | local continuity, C(health) | `{kind:"health_changed",change:{previous:100,current:92,delta:-8}}` |
| armor_changed | local continuity, C(armor) | `{kind:"armor_changed",change:{previous:100,current:95,delta:-5}}` |
| helmet_changed | local continuity, C(explicit helmet bool) | `{kind:"helmet_changed",change:{previous:false,current:true}}` |
| defuse_kit_changed | local continuity, C(explicit kit bool); absent kit never means false | `{kind:"defuse_kit_changed",change:{previous:false,current:true}}` |
| money_changed | local continuity, C(balance), no cause inferred | `{kind:"money_changed",change:{previous:800,current:150,delta:-650}}` |
| equipment_value_changed | local continuity, C(reported valuation), no inventory recomputation | `{kind:"equipment_value_changed",change:{previous:200,current:850,delta:650}}` |
| exposure_changed | local continuity, C(intensity); flash/smoke/burning separately; allowed while dead because this reports a signal | `{kind:"exposure_changed",effect:"smoke",change:{previous:0,current:21,delta:21}}` |
| match_stats_changed | local continuity; collect each known changed kills/deaths/assists/mvps/score, including decreases | `{kind:"match_stats_changed",changes:[{stat:"mvps",previous:0,current:1,delta:1}]}` |
| player_kill | local continuity; kills increases; no match-stat field decreased in same observation; one aggregate event, not N invented timestamps | `{kind:"player_kill",count:1,total:6}` |
| player_died | local continuity; previous health>0, current health=0 AND deaths increases by exactly 1; no match-stat decrease | `{kind:"player_died",total:1}` |
| round_stats_changed | local continuity; known round kills/headshot kills changed; counters remain distinct from kill occurrence | `{kind:"round_stats_changed",changes:[{stat:"round_headshot_kills",previous:1,current:2,delta:1}]}` |
| weapon_changed | local continuity; exactly one selected weapon in each observation; model name or category differs | `{kind:"weapon_changed",change:{previous:{name:"weapon_glock",category:"Pistol",finish:"default"},current:{name:"weapon_knife_t",category:"Knife",finish:"default"}}}` |
| weapon_state_changed | local continuity; selected description (including optional finish) equal; active/reloading status changed | `{kind:"weapon_state_changed",weapon:{name:"weapon_glock",category:"Pistol",finish:"default"},change:{previous:"active",current:"reloading"}}` |
| ammo_changed | local continuity; selected description equal; at least one known clip/capacity/reserve changed, independently | `{kind:"ammo_changed",weapon:{name:"weapon_glock",category:"Pistol",finish:"default"},clip:{previous:20,current:19,delta:-1},capacity:null,reserve:null}` |

No finish-only change creates a weapon switch. Finish uncertainty interrupts ammo/handling comparison. A valid descriptor is still not proof of physical weapon continuity, so ammo events make no firing/reloading claim. No current selected weapon (death, unknown, multiple actives or all holstered) invalidates the selected-weapon baseline. Returning selection initializes without a switch.

For every catalog row marked **No**, **State only**, or **Covered by**, the independent candidate event's emitted example is `[]`; the given raw example updates state or emits only the explicitly named covering event. There are no secretly implemented INFERRED or AMBIGUOUS variants. Positive local kills, kit toggles, flash/burn changes, map swaps, counter corrections and several malformed/partial cases use clearly labeled synthetic/derived tests because the capture does not demonstrate them as continuous local transitions.

## Reset rules and ordering

1. Initial state, session UUID change, envelope or provider-time gap >90 seconds, time regression, provider identity/app/build change: establish a full baseline, emit no game events. Equal provider seconds are allowed. Numeric source-sequence gaps alone do not prove a game boundary (watch-channel coalescing is permitted).
2. Map/mode/recognized match-phase context becomes unavailable, or map/mode changes: reset gameplay comparisons. A known map-name change may emit map_changed; missing map alone never emits match-left. Reappearance does not compare stale state. Repeated menu observations emit nothing.
3. Known phase enters warmup, leaves gameover, warmup→live, or completed-round counter decreases: match reset. Explicit phase/start/end observations may still emit; scores and player counters are not compared across that reset. Same-map rematches with no visible boundary cannot be proved and are a limitation of this data surface.
4. Local identity missing/spectated: current `player=None`, discarding its comparison baseline. Returning local identity restores state without comparing the spectator or pre-spectator local snapshot. Local side change emits team_changed and resets other local comparisons.
5. Local round comparison requires known counts and phases. Equal completed-round counters are continuous unless entering new freezetime from another phase or leaving over. A +1 increment while prior phase was live and current phase is over (or current match gameover) is still the ending round. Other jumps/missing counters/phases reset all local comparisons. Thus 529→530 resets health, equipment, ammo, all stats and round counters; 524→525 preserves the ending-round MVP/money observations. A skipped local boundary causes deliberate false negatives. Warmup without an observed round phase has normalized state but no health/combat/inventory comparisons.
6. Counter decrease is a reported correction, not a negative kill. A match-stat decrease suppresses derived kill/death events for that update; the new observations become baseline. No streak counters are maintained. No reward backlog is reconstructed.
7. Individual field absence invalidates only that observation baseline (plus identity/map/round availability gates above). `previously` and `added` never synthesize a reset or missing value.

Deterministic emission order within each payload is: map identity; map phase; match start/end; round phase; round start/end; side scores; local UI activity; local side; health; armor; helmet; kit; money; equipment valuation; flash/smoke/burning; aggregate match statistics (kills/deaths/assists/MVPs/score); kill; death; round statistics (kills/headshots); selected weapon; handling state; ammo (clip/capacity/reserve). Order is a stable presentation contract, **not a claim of physical causality**. Duplicated effective state generates no event even with different delta hints. Known-value baselines, not a hash or provider-second timestamp, deduplicate.

## State lifetime and security integration

`AppState.cs2` owns the existing service's in-memory pipeline. One entry per device contains its last signed Source and one normalized Cs2State; session UUID is part of comparison continuity. No raw Value or event history is retained. At most 2,048 device entries; a new entry evicts the least recently observed at capacity. Ten-minute idle expiry is checked on lookup and every 60 seconds by a shutdown-aware task registered with the existing TaskTracker. A >90-second observation gap already rebaselines before physical eviction. Desktop heartbeats do not create/refresh gameplay state.

The original Ed25519 verification, exact signed bytes, freshness, replay SQL and registered-device authorization stay in place. Only **after successful DB commit** does accepted GSI enter the normalizer. No per-field SQL is performed. A fixed array of 128 per-device hashed async lock stripes serializes the existing accept/commit/normalize operation, and dashboard revocation takes the same stripe before updating the device row. This prevents a later request normalizing before an earlier committed request and prevents an already accepted handler recreating state after revocation. Hash collisions serialize some unrelated devices but do not mix their state. Signed unpair and owner revocation remove the entry before returning; rejected GSI/heartbeats cannot recreate it. Other administrative deletion paths are bounded by inactivity expiry.

Restart/eviction means an empty baseline and missed early events, never invented startup kills. Replay protection remains persistent and independent. In-memory observations are sufficient for this diagnostic semantic layer; **there is no durable/exactly-once delivery guarantee**. Deployment currently uses one backend process. Multiple independent backend workers would need device affinity or a coordinated semantic owner before consuming events for rewards; shared replay SQL alone is not shared semantic history. No parallel infrastructure or distributed event transport is added in this pass.

Memory per entry is bounded by field/string/inventory limits; processing scales with the small enabled snapshot and <=64 weapons. Cleanup is O(active devices) once a minute; capacity eviction scans only when admitting a new device at the cap. Debug pretty printing is gated by DEBUG plus explicit flags.

## Diagnostic logging

Use `CS2_LOG_GSI_PIPELINE=true` and `RUST_LOG=necko7::cs2=debug,info` (or existing `necko7=debug,info`). Every accepted GSI emits correlated RAW GSI, NORMALIZED STATE (previous/current/reset reasons), and NORMALIZED EVENTS **including []**. `CS2_LOG_GSI_PAYLOADS=true` remains a raw-only compatibility option at the new module target. With both flags false, DEBUG still emits compact `CS2 event` records for nonempty events. Production INFO gets neither raw nor event/state verbosity. Flags default false; no private keys/signature material/envelope auth are included. Every `auth` key is recursively removed from the GSI before parsing or logging, including delta objects and arrays.

These diagnostics use existing tracing sinks and therefore their existing log retention; this layer adds no payload database/file archive. Turn verbose mode off after validation and apply finite retention to the configured external/Docker log sink. The normalized state and fixture examples are not a reason to retain production payloads indefinitely.

## Additional categories worth evaluating later (not enabled here)

| Category | Concrete potential benefit | What still must be validated |
|---|---|---|
| phase_countdowns | Reported phase time remaining and conservative same-phase low-time crossings | Availability during normal player gameplay vs observer; pauses, timer corrections, phase reset; absent today |
| map_round_wins | Historical round result corroboration/delayed winner reconciliation | Side swaps, indexing, partial history and coverage during live player mode |
| allplayers_id + allplayers_state + allplayers_match_stats | Roster, alive counts and better team identity; possible clutch-state candidates | Often observer-dependent; access while playing, partial roster, disconnects, substitute slots; does not itself provide killfeed attacker/victim links |
| bomb | Explicit bomb state/location and potentially carrier/plant/defuse observations | Exact fields/identity visibility in permitted mode; C4 disappearance alone remains insufficient |
| allgrenades | Grenade entity ownership/type/lifetime/position candidates | Observer access, stable entity IDs, omitted/expired objects, owner identity semantics |
| player_position / allplayers_position | Position and map-zone observations | Observer access, map coordinate definitions, teleports; line-of-sight still requires geometry/visibility evidence |

Even with these categories, a sampled scoreboard is not an exact killfeed. Exact killer/victim/weapon, trade attribution and grenade damage source would need explicitly observed causal data; do not promise them from added categories alone.

## Real timeline and validation fixtures

`src/cs2/fixtures/live-gameplay.json` contains all 127 anonymized snapshots. `timeline.json` asserts the complete ordered list of event bodies and reset reasons for every original seq, including empty arrays. Source hash and transformations are in the fixture README. The fixture preserves sparse previous/added, same-second updates, round endings, halftime gap, menu removal and gameover/freezetime contradiction.

A representative exact excerpt:

```text
seq=8   warmup -> live           [map_phase_changed, match_started] (match baseline reset)
seq=9   armor/money/equip        [armor_changed, money_changed, equipment_value_changed]
seq=10  freezetime -> live       [round_phase_changed, round_started, weapon_changed]
seq=14  health 100 -> 92         [health_changed]
seq=15  health 92 -> 82,
        armor 100 -> 95         [health_changed, armor_changed]
seq=16  Glock clip 20 -> 19      [ammo_changed]
seq=21  health 59 -> 0,
        deaths 0 -> 1           [health_changed, armor_changed, match_stats_changed, player_died]
seq=22  spectator appears       [] (local identity lost)
seq=24  spectator kills 1 -> 2   []
seq=26  spectator reloading     []
seq=27  spectator clip up       []
seq=32  round over, CT 0 -> 1    [round_phase_changed, round_ended, score_changed]
seq=34  local returns           [round_phase_changed] (fresh local baseline)
seq=41  smoke 0 -> 21, health 0  [exposure_changed] (reported intensity only)
seq=272 large observation gap   [] (full baseline reset)
seq=273 spectator kill/damage   []
seq=524 large halftime gap      [] (no invented 8 local kills)
seq=525 round over, MVP 0 -> 1   [round_phase_changed, round_ended, score_changed, money_changed, match_stats_changed]
seq=530 next freezetime         [round_phase_changed] (no fake spawn/reload)
seq=617 live -> gameover        [map_phase_changed, match_ended, round_phase_changed, score_changed]
seq=621 menu/map removed        [] (map/local context reset)
seq=622 equivalent menu state   []
seq=623 equivalent menu state   []
```

Every one of the 57 spectator payloads is asserted to produce zero local-player events, with no local player context. Four corroborated local deaths: 21, 40, 296, 544. Zero observed contiguous local kills in this capture. The positive local-combat test changes only player identity in the real 272→273 full snapshots and asserts the complete order and numeric values; this derived scenario is not misrepresented as real local gameplay.

## DEBUG pipeline example

[Full actual tracing output](cs2-pipeline-example.log) was captured by the logging regression test with anonymized real payloads 14→15, followed by an equivalent payload and an event-only update. Test correlation IDs/timestamps are synthetic. Below is an abbreviated projection of the same output (the linked file includes the full sanitized raw payload, both full normalized states and complete event envelopes):

```text
DEBUG cs2_pipeline{device_id=00000000-0000-0000-0000-000000000001 channel_id=fixture-channel session_id=00000000-0000-0000-0000-000000000002 seq=15}: RAW GSI
payload={"provider":{"appid":730,"steamid":"76561198000000001",...},
         "player":{"steamid":"76561198000000001","state":{"health":82,"armor":95,...},...},
         "previously":{"player":{"state":{"health":92,"armor":100}}},...}
DEBUG cs2_pipeline{... seq=15}: NORMALIZED STATE
previous.player={"health":92,"armor":100,...}
current.player={"health":82,"armor":95,...}
resets=[]
DEBUG cs2_pipeline{... seq=15}: NORMALIZED EVENTS
events=[
  {"kind":"health_changed","change":{"previous":92,"current":82,"delta":-10},"reliability":"RELIABLE",...},
  {"kind":"armor_changed","change":{"previous":100,"current":95,"delta":-5},"reliability":"RELIABLE",...}
]
DEBUG cs2_pipeline{... seq=15}: CS2 event details={"kind":"health_changed","change":{"previous":92,"current":82,"delta":-10}}
DEBUG cs2_pipeline{... seq=15}: CS2 event details={"kind":"armor_changed","change":{"previous":100,"current":95,"delta":-5}}
DEBUG cs2_pipeline{... seq=16}: NORMALIZED EVENTS events=[]
```

The logging test injects local auth before sanitization and asserts that neither the output nor retained state contains it; it also asserts raw output is absent in event-only mode and that all correlation fields and empty event results appear.

## Validation results

* `cargo check`: passed for backend 0.9.2 (bumped from the user's accepted 0.9.1 baseline; old checked-in manifests still said 0.9.0).
* `rustfmt --edition 2024 --check src/cs2/mod.rs src/api/v1/cs2.rs`: passed, including their child modules/tests. `cargo fmt --all -- --check` was run and fails on pre-existing formatting throughout unrelated modules; they were not mass-reformatted.
* `cargo clippy --all-targets`: passed with `#![deny(clippy::all)]` on both CS2 modules and their tests. Global `cargo clippy --all-targets -- -D warnings` was also run; it is blocked by 113 pre-existing warnings outside CS2. No new CS2 lint allowances were introduced.
* The reviewed real timeline contains 115 events (18 kinds exercised), 63 empty-event updates. Positive branches for other implemented kinds and invalid/partial/reset conditions are covered by derived/synthetic tests.
* Integration security tests still assert exact signed bytes, tamper/replay/stale rejection, pairing concurrency, channel authorization and revoked-device heartbeat rejection; additional assertions verify accepted real data reaches normalization, heartbeats/rejected signatures do not initialize it, replay cannot roll it back, and both revocation paths remove state.
* The first full run exposed a new logging-test timestamp regression in its test data; the fixture time was corrected. A rerun against the same disposable database exposed an existing inventory test's reliance on an empty global pending queue (previous runs leave rows). Final `cargo test -- --include-ignored` against fresh disposable PostgreSQL 17: **112 passed, 0 failed, 0 ignored**, including all six PostgreSQL integration tests. There are 21 new semantic/logging/lifecycle tests plus expanded existing API integration assertions. No inventory code/tests were weakened or changed.
* No desktop or dashboard source changes were required. Windows UI/lifecycle behavior is outside this backend-only change and was not reverified. A future manual gameplay pass should enable the pipeline flag and check new CS2 versions against these same identity/reset rules; this capture cannot validate currently unobserved countdowns/bomb fields.
