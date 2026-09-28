use super::*;
use model::*;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Record {
    seq: i64,
    received_at: chrono::DateTime<chrono::Utc>,
    payload: Value,
}
fn records() -> Vec<Record> {
    serde_json::from_str(include_str!("fixtures/live-gameplay.json")).unwrap()
}
fn payload(seq: i64) -> Value {
    records()
        .into_iter()
        .find(|r| r.seq == seq)
        .unwrap()
        .payload
}
fn source(seq: i64) -> Source {
    Source {
        device_id: Uuid::from_u128(1),
        channel_id: "fixture-channel".into(),
        session_id: Uuid::from_u128(2),
        source_seq: seq,
        timestamp: chrono::DateTime::from_timestamp(1790523000 + seq, 0).unwrap(),
    }
}
fn kinds(t: &Transition) -> Vec<Value> {
    t.events
        .iter()
        .map(|e| serde_json::to_value(&e.event).unwrap())
        .collect()
}
fn names(t: &Transition) -> Vec<String> {
    kinds(t)
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect()
}
fn combat() -> (Value, Value) {
    // Real adjacent spectator combat snapshots, with ONLY player identity changed
    // to local for exercising the positive branch not present in this capture.
    let mut a = payload(272);
    let mut b = payload(273);
    a["player"]["steamid"] = a["provider"]["steamid"].clone();
    b["player"]["steamid"] = b["provider"]["steamid"].clone();
    (a, b)
}
#[test]
fn complete_real_payload_timeline() {
    let mut normalizer = Normalizer::default();
    let mut timeline = Vec::new();
    let mut deaths = Vec::new();
    let mut spectators = 0;
    for r in records() {
        let source = Source {
            timestamp: r.received_at,
            ..source(r.seq)
        };
        let t = normalizer.apply(source.clone(), &sanitize(r.payload));
        for event in &t.events {
            assert_eq!(event.source.source_seq, r.seq);
            assert_eq!(event.source.session_id, source.session_id);
            if matches!(event.event, EventKind::PlayerDied { .. }) {
                deaths.push(r.seq);
            }
            assert!(
                !matches!(event.event, EventKind::PlayerKill { .. }),
                "No contiguous local kills in real capture at {}",
                r.seq
            );
        }
        if t.current.view.identity == Identity::Spectator {
            spectators += 1;
            assert!(t.current.player.is_none());
            assert!(
                t.events.iter().all(|e| matches!(
                    e.event,
                    EventKind::MapChanged { .. }
                        | EventKind::MapPhaseChanged { .. }
                        | EventKind::MatchStarted
                        | EventKind::MatchEnded
                        | EventKind::RoundPhaseChanged { .. }
                        | EventKind::RoundStarted
                        | EventKind::RoundEnded
                        | EventKind::ScoreChanged { .. }
                )),
                "spectator local event at {}",
                r.seq
            );
            assert!(t.events.iter().all(|e| e.player.is_none()));
        }
        timeline.push(json!({"seq":r.seq,"resets":t.resets,"events":kinds(&t)}));
    }
    assert_eq!(spectators, 57);
    assert_eq!(deaths, [21, 40, 296, 544]);
    // A readable full timeline: every payload, all event details, including [].
    let actual = serde_json::to_value(&timeline).unwrap();
    let expected: Value = serde_json::from_str(include_str!("fixtures/timeline.json")).unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn correlated_local_combat_has_exact_deterministic_order_and_details() {
    let (a, b) = combat();
    let mut n = Normalizer::default();
    assert!(n.apply(source(100), &a).events.is_empty());
    let t = n.apply(source(101), &b);
    assert_eq!(
        names(&t),
        [
            "health_changed",
            "armor_changed",
            "money_changed",
            "match_stats_changed",
            "player_kill",
            "round_stats_changed",
            "ammo_changed"
        ]
    );
    assert_eq!(
        kinds(&t)[0],
        json!({"kind":"health_changed","change":{"previous":49,"current":16,"delta":-33}})
    );
    assert_eq!(
        kinds(&t)[4],
        json!({"kind":"player_kill","count":1,"total":6})
    );
    assert_eq!(
        kinds(&t)[5]["changes"],
        json!([
            {"stat":"round_kills","previous":1,"current":2,"delta":1},
            {"stat":"round_headshot_kills","previous":1,"current":2,"delta":1}
        ])
    );
    assert!(
        t.events
            .iter()
            .all(|e| e.player.as_ref().unwrap().steam_id == "76561198000000001")
    );
    assert!(n.apply(source(102), &b).events.is_empty());
}
#[test]
fn spectator_switch_cannot_supply_local_baselines_even_via_previously() {
    let mut n = Normalizer::default();
    n.apply(source(1), &payload(21));
    for (i, seq) in [22, 23, 24, 26, 27].into_iter().enumerate() {
        let t = n.apply(source(i as i64 + 2), &payload(seq));
        assert!(t.events.is_empty());
        assert!(t.current.player.is_none());
    }
    let mut reentry = payload(21);
    reentry["player"]["state"]["health"] = json!(100);
    reentry["player"]["match_stats"]["kills"] = json!(9);
    reentry["provider"]["timestamp"] = json!(1790523000);
    let t = n.apply(source(10), &reentry);
    assert!(t.events.is_empty());
    assert!(t.resets.contains(&ResetReason::LocalIdentityRestored));
}
#[test]
fn missing_identity_or_malformed_identity_never_inherits_previous_local_player() {
    for path in ["provider", "player"] {
        let (a, mut b) = combat();
        let mut n = Normalizer::default();
        n.apply(source(1), &a);
        b[path].as_object_mut().unwrap().remove("steamid");
        b["previously"][path]["steamid"] = a[path]["steamid"].clone();
        let t = n.apply(source(2), &b);
        assert!(t.current.player.is_none());
        assert!(t.events.is_empty());
    }
    let (a, mut b) = combat();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    b["provider"]["appid"] = json!(570);
    assert!(n.apply(source(2), &b).events.is_empty());
}
#[test]
fn sparse_invalid_and_removed_fields_are_unknown_not_zero_or_false() {
    let (a, mut sparse) = combat();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    sparse["player"]["state"] = json!({"health":"0","armor":-1,"helmet":null,"money":1000001});
    sparse["player"]
        .as_object_mut()
        .unwrap()
        .remove("match_stats");
    sparse["player"].as_object_mut().unwrap().remove("weapons");
    sparse["previously"] = json!({"player":{"state":{"health":1},"match_stats":{"kills":0}}});
    sparse["added"] = json!({"player":{"state":true}});
    let t = n.apply(source(2), &sparse);
    assert!(t.events.is_empty());
    let p = t.current.player.unwrap();
    assert!(
        p.health.is_none()
            && p.armor.is_none()
            && p.money.is_none()
            && p.helmet.is_none()
            && p.weapons.is_none()
    );
    assert!(n.apply(source(3), &a).events.is_empty());
    let mut empty = a.clone();
    empty["player"]["weapons"] = json!({});
    assert!(n.apply(source(4), &empty).events.is_empty());
    assert!(n.apply(source(5), &a).events.is_empty());
}
#[test]
fn equivalent_payloads_ignore_delta_markers_provider_time_and_weapon_slot_order() {
    let (a, _) = combat();
    let mut b = a.clone();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    b["previously"] = json!({"player":{"match_stats":{"kills":0},"state":{"health":100}}});
    b["added"] = json!({"player":true});
    b["provider"]["timestamp"] = json!(1790523463);
    let weapons = b["player"]["weapons"].as_object_mut().unwrap();
    let first = weapons.remove("weapon_0").unwrap();
    weapons.insert("weapon_99".into(), first);
    assert!(n.apply(source(2), &b).events.is_empty());
}
#[test]
fn round_reset_does_not_create_fake_health_ammo_or_kill_events() {
    let mut n = Normalizer::default();
    n.apply(source(1), &payload(529));
    let t = n.apply(source(2), &payload(530));
    assert_eq!(names(&t), ["round_phase_changed"]);
    assert!(t.resets.contains(&ResetReason::RoundBoundary));
    assert_eq!(t.current.player.unwrap().round_stats.kills, Some(0));
}
#[test]
fn final_gameover_freezetime_is_match_end_not_new_round_or_spawn() {
    let mut n = Normalizer::default();
    n.apply(source(1), &payload(616));
    let t = n.apply(source(2), &payload(617));
    assert_eq!(
        names(&t),
        [
            "map_phase_changed",
            "match_ended",
            "round_phase_changed",
            "round_ended",
            "score_changed"
        ]
    );
    assert_eq!(t.current.r#match.unwrap().phase, Some(MatchPhase::GameOver));
    assert!(n.apply(source(3), &payload(617)).events.is_empty());
}
#[test]
fn gaps_sessions_accounts_maps_modes_and_clock_regressions_rebaseline() {
    let (a, b) = combat();
    for case in 0..7 {
        let mut n = Normalizer::default();
        n.apply(source(1), &a);
        let mut s = source(2);
        let mut next = b.clone();
        match case {
            0 => s.session_id = Uuid::from_u128(5),
            1 => s.timestamp += chrono::Duration::seconds(100),
            2 => next["provider"]["timestamp"] = json!(1),
            3 => {
                next["provider"]["steamid"] = json!("76561198000000009");
                next["player"]["steamid"] = json!("76561198000000009");
            }
            4 => next["map"]["name"] = json!("de_other"),
            5 => next["map"]["mode"] = json!("casual"),
            _ => next["provider"]["version"] = json!(999),
        }
        let t = n.apply(s, &next);
        assert!(
            t.events
                .iter()
                .all(|e| matches!(e.event, EventKind::MapChanged { .. })),
            "case {case}"
        );
        assert!(!t.resets.is_empty());
    }
}
#[test]
fn full_context_disappearance_and_reappearance_do_not_resurrect_baselines() {
    let (a, b) = combat();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    let mut menu = payload(621);
    menu["provider"]["timestamp"] = a["provider"]["timestamp"].clone();
    let t = n.apply(source(2), &menu);
    assert!(t.current.r#match.is_none());
    let t = n.apply(source(3), &b);
    assert!(!names(&t).contains(&"player_kill".into()));
}
#[test]
fn counter_corrections_do_not_prove_kills_or_deaths() {
    let (a, mut b) = combat();
    b["player"]["match_stats"]["deaths"] = json!(0);
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    let t = n.apply(source(2), &b);
    assert!(names(&t).contains(&"match_stats_changed".into()));
    assert!(!names(&t).contains(&"player_kill".into()));
}
#[test]
fn death_requires_both_health_transition_and_death_counter_corroboration() {
    for alter in [false, true] {
        let a = payload(20);
        let mut b = payload(21);
        if alter {
            b["player"]["match_stats"]["deaths"] = json!(0);
        }
        let mut n = Normalizer::default();
        n.apply(source(1), &a);
        let t = n.apply(source(2), &b);
        assert_eq!(names(&t).contains(&"player_died".into()), !alter);
    }
}
#[test]
fn reload_state_is_observable_but_no_shot_or_reload_completion_is_invented() {
    let mut n = Normalizer::default();
    let mut all = Vec::new();
    for (i, seq) in [25, 26, 27].into_iter().enumerate() {
        let mut p = payload(seq);
        p["player"]["steamid"] = p["provider"]["steamid"].clone();
        all.push(n.apply(source(i as i64 + 1), &p));
    }
    assert!(all[0].events.is_empty());
    assert_eq!(names(&all[1]), ["weapon_state_changed"]);
    assert_eq!(names(&all[2]), ["ammo_changed"]);
    assert_eq!(kinds(&all[2])[0]["reserve"]["delta"], -1);
}
#[test]
fn explicit_bools_effects_and_batched_stats_keep_observation_semantics() {
    let (a, mut b) = combat();
    let mut a = a;
    a["player"]["state"]["defusekit"] = json!(false);
    b["player"]["state"]["defusekit"] = json!(true);
    b["player"]["state"]["flashed"] = json!(100);
    b["player"]["state"]["burning"] = json!(33);
    b["player"]["match_stats"]["kills"] = json!(8);
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    let t = n.apply(source(2), &b);
    assert!(names(&t).contains(&"defuse_kit_changed".into()));
    assert!(kinds(&t).contains(&json!({"kind":"player_kill","count":3,"total":8})));
    assert_eq!(
        t.events
            .iter()
            .filter(|e| matches!(e.event, EventKind::ExposureChanged { .. }))
            .count(),
        2
    );
}
#[test]
fn state_is_bounded_expires_and_revocation_removes_it() {
    let pipeline = Pipeline::default();
    let now = Instant::now();
    let p = payload(14);
    for id in 1..=CAPACITY + 1 {
        let s = Source {
            device_id: Uuid::from_u128(id as u128),
            ..source(1)
        };
        pipeline.process_at(s, &p, now + Duration::from_millis(id as u64));
    }
    assert_eq!(pipeline.entries.lock().len(), CAPACITY);
    assert!(!pipeline.contains(Uuid::from_u128(1)));
    pipeline.remove(Uuid::from_u128(2));
    assert!(!pipeline.contains(Uuid::from_u128(2)));
    pipeline.expire_at(now + TTL + Duration::from_secs(10));
    assert!(pipeline.entries.lock().is_empty());
    assert!(
        pipeline
            .process_at(source(2), &payload(15), now + TTL + Duration::from_secs(11))
            .events
            .is_empty()
    );
}
#[test]
fn older_normalization_calls_do_not_roll_state_back() {
    let (a, b) = combat();
    let mut n = Normalizer::default();
    n.apply(source(10), &a);
    n.apply(source(11), &b);
    let t = n.apply(source(9), &a);
    assert!(t.events.is_empty());
    assert_eq!(t.current.player.unwrap().health, Some(16));
    assert!(n.apply(source(12), &b).events.is_empty());
}
#[tokio::test]
async fn device_serialization_prevents_commit_processing_or_revocation_reordering() {
    let pipeline = std::sync::Arc::new(Pipeline::default());
    let id = source(1).device_id;
    let guard = pipeline.serial(id).await;
    let next = pipeline.clone();
    let task = tokio::spawn(async move {
        let _g = next.serial(id).await;
        next.remove(id);
    });
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    pipeline.process(source(1), &payload(14));
    drop(guard);
    task.await.unwrap();
    assert!(!pipeline.contains(id));
}
#[test]
fn sanitized_pipeline_never_retains_auth_even_inside_delta_arrays() {
    let mut raw = payload(14);
    raw["auth"] = json!({"token":"local-secret"});
    raw["added"] = json!([{"auth":{"token":"nested-secret"}}]);
    let clean = sanitize(raw);
    assert!(!clean.to_string().contains("secret"));
    let t = Normalizer::default().apply(source(1), &clean);
    assert!(!serde_json::to_string(&t).unwrap().contains("auth"));
}

#[test]
fn debug_logs_correlate_raw_state_events_including_empty_results() {
    #[derive(Clone)]
    struct Writer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = Writer(output.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let mut n = Normalizer::default();
        let a = payload(14);
        let mut b = payload(15);
        b["auth"] = json!({"token":"local-secret"});
        let b = sanitize(b);
        n.apply(source(14), &a);
        let t = n.apply(source(15), &b);
        log_with_options(&source(15), &b, &t, true, false);
        let t = n.apply(source(16), &b);
        log_with_options(&source(16), &b, &t, true, false);
        let mut next = a.clone();
        next["provider"]["timestamp"] = json!(1790522990);
        let t = n.apply(source(17), &next);
        log_with_options(&source(17), &next, &t, false, false);
    });
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    for expected in [
        "RAW GSI",
        "NORMALIZED STATE",
        "NORMALIZED EVENTS",
        "events=[]",
        "CS2 event",
        "health_changed",
        "armor_changed",
        "device_id=",
        "channel_id=fixture-channel",
        "session_id=",
        "seq=15",
        "seq=16",
        "seq=17",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("local-secret"));
    assert_eq!(
        output.matches("RAW GSI").count(),
        2,
        "event-only mode must omit raw"
    );
}

#[test]
fn incomplete_finish_and_skipped_rounds_do_not_create_selection_or_lifecycle_claims() {
    let (a, mut b) = combat();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    b["player"]["weapons"]["weapon_2"]
        .as_object_mut()
        .unwrap()
        .remove("paintkit");
    let t = n.apply(source(2), &b);
    assert!(!names(&t).contains(&"weapon_changed".into()));
    assert!(!names(&t).contains(&"ammo_changed".into()));
    let mut n = Normalizer::default();
    n.apply(source(1), &payload(9));
    let mut live = payload(10);
    live["map"]["round"] = json!(3);
    assert!(!names(&n.apply(source(2), &live)).contains(&"round_started".into()));
}

#[test]
fn missing_match_phase_breaks_combat_baseline_until_context_is_known_again() {
    let (a, mut b) = combat();
    let mut n = Normalizer::default();
    n.apply(source(1), &a);
    b["map"].as_object_mut().unwrap().remove("phase");
    assert!(n.apply(source(2), &b).events.is_empty());
    let (_, b) = combat();
    assert!(n.apply(source(3), &b).events.is_empty());
}

fn wingman_records() -> Vec<Record> {
    serde_json::from_str(include_str!("fixtures/wingman-two-rounds.json")).unwrap()
}

#[test]
fn complete_wingman_match_preserves_halftime_kill_and_side_switch() {
    let mut n = Normalizer::default();
    let mut timeline = Vec::new();
    let mut kills = 0;
    let mut starts = 0;
    let mut ends = 0;
    for r in wingman_records() {
        let t = n.apply(
            Source {
                timestamp: r.received_at,
                ..source(r.seq)
            },
            &r.payload,
        );
        for event in &t.events {
            match event.event {
                EventKind::PlayerKill { count, .. } => kills += count,
                EventKind::MatchStarted => starts += 1,
                EventKind::MatchEnded => ends += 1,
                _ => {}
            }
        }
        if r.seq == 3623 {
            assert_eq!(
                serde_json::to_value(t.current.r#match.as_ref().unwrap().phase).unwrap(),
                json!("intermission")
            );
            assert!(
                t.resets.is_empty(),
                "Halftime preserves the ending round's counters"
            );
        }
        if r.seq == 3625 {
            assert!(t.resets.contains(&ResetReason::TeamChanged));
            assert_eq!(
                t.current.player.as_ref().unwrap().match_stats.kills,
                Some(2)
            );
            assert_eq!(
                t.current.player.as_ref().unwrap().round_stats.kills,
                Some(0)
            );
        }
        if r.seq == 3633 {
            assert_eq!(
                t.current.r#match.as_ref().unwrap().score,
                Score {
                    ct: Some(1),
                    t: Some(1)
                }
            );
            assert!(names(&t).contains(&"match_ended".into()));
            assert!(names(&t).contains(&"round_ended".into()));
            assert!(names(&t).contains(&"player_died".into()));
            assert!(!names(&t).contains(&"round_started".into()));
        }
        timeline.push(json!({"seq":r.seq,"events":kinds(&t)}));
    }
    assert_eq!((kills, starts, ends), (2, 1, 1));
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/wingman-timeline.json")).unwrap();
    assert_eq!(serde_json::to_value(timeline).unwrap(), expected);
}

#[test]
fn halftime_keeps_identity_guards_and_equivalent_payload_deduplication() {
    let rs = wingman_records();
    let a = &rs.iter().find(|r| r.seq == 3622).unwrap().payload;
    let b = &rs.iter().find(|r| r.seq == 3623).unwrap().payload;
    let mut n = Normalizer::default();
    n.apply(source(1), a);
    let t = n.apply(source(2), b);
    assert!(names(&t).contains(&"player_kill".into()));
    assert!(n.apply(source(3), b).events.is_empty());
    let mut spectated = b.clone();
    spectated["player"]["steamid"] = json!("76561198000000002");
    let mut n = Normalizer::default();
    n.apply(source(1), a);
    let t = n.apply(source(2), &spectated);
    assert_eq!(
        names(&t),
        [
            "map_phase_changed",
            "round_phase_changed",
            "round_ended",
            "score_changed"
        ]
    );
    assert!(t.current.player.is_none());
    let mut unknown = b.clone();
    unknown["map"]["phase"] = json!("unrecognized_future_phase");
    let mut n = Normalizer::default();
    n.apply(source(1), a);
    assert!(n.apply(source(2), &unknown).events.is_empty());
}
