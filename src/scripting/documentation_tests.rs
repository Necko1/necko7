use super::runtime::{self, Files};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

fn catalog() -> Value {
    serde_json::from_str(include_str!("../../docs/scripting/api.json")).unwrap()
}

fn event_catalog() -> Value {
    serde_json::from_str(include_str!("../../docs/scripting/events.json")).unwrap()
}

fn merge_observation(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            merge_observation(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    } else {
        *target = patch.clone();
    }
}

// Exhaustive on purpose: a new runtime variant must acquire a public section.
fn documented_event_kind(event: &crate::cs2::model::EventKind) -> &'static str {
    use crate::cs2::model::EventKind::*;
    match event {
        MapChanged { .. } => "map_changed",
        MapPhaseChanged { .. } => "map_phase_changed",
        MatchStarted => "match_started",
        MatchEnded => "match_ended",
        RoundPhaseChanged { .. } => "round_phase_changed",
        RoundStarted => "round_started",
        RoundEnded => "round_ended",
        ScoreChanged { .. } => "score_changed",
        ActivityChanged { .. } => "activity_changed",
        TeamChanged { .. } => "team_changed",
        HealthChanged { .. } => "health_changed",
        ArmorChanged { .. } => "armor_changed",
        HelmetChanged { .. } => "helmet_changed",
        DefuseKitChanged { .. } => "defuse_kit_changed",
        MoneyChanged { .. } => "money_changed",
        EquipmentValueChanged { .. } => "equipment_value_changed",
        ExposureChanged { .. } => "exposure_changed",
        MatchStatsChanged { .. } => "match_stats_changed",
        PlayerKill { .. } => "player_kill",
        PlayerDied { .. } => "player_died",
        RoundStatsChanged { .. } => "round_stats_changed",
        WeaponChanged { .. } => "weapon_changed",
        WeaponStateChanged { .. } => "weapon_state_changed",
        AmmoChanged { .. } => "ammo_changed",
    }
}

#[test]
fn every_documented_event_payload_matches_real_normalizer_output() {
    use crate::cs2::{Normalizer, model::Source};
    let catalog = event_catalog();
    let envelope = &catalog["envelope"];
    let source = Source {
        device_id: envelope["device_id"].as_str().unwrap().parse().unwrap(),
        channel_id: envelope["channel_id"].as_str().unwrap().into(),
        session_id: envelope["session_id"].as_str().unwrap().parse().unwrap(),
        source_seq: envelope["source_seq"].as_i64().unwrap(),
        timestamp: envelope["timestamp"].as_str().unwrap().parse().unwrap(),
    };
    let api = self::catalog();
    let mut common_fields: BTreeSet<_> = envelope.as_object().unwrap().keys().cloned().collect();
    common_fields.insert("kind".into());
    assert_eq!(
        common_fields,
        api["properties"]["ctx.event"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect(),
        "Common event fields drifted from the context reference"
    );
    let mut documented = BTreeSet::new();
    let mut derived = BTreeSet::new();
    for entry in catalog["events"].as_array().unwrap() {
        let kind = entry["kind"].as_str().unwrap();
        assert!(documented.insert(kind.to_owned()), "Duplicate {kind}");
        let mut previous = catalog["baseline"].clone();
        let mut current = previous.clone();
        merge_observation(&mut previous, &entry["previous"]);
        merge_observation(&mut current, &entry["current"]);
        let mut normalizer = Normalizer::default();
        normalizer.apply(
            Source {
                source_seq: source.source_seq - 1,
                timestamp: source.timestamp - chrono::Duration::seconds(1),
                ..source.clone()
            },
            &previous,
        );
        let transition = normalizer.apply(source.clone(), &current);
        for event in &transition.events {
            derived.insert(documented_event_kind(&event.event).to_owned());
        }
        let event = transition
            .events
            .iter()
            .find(|event| documented_event_kind(&event.event) == kind)
            .unwrap_or_else(|| panic!("Documented observations did not produce {kind}"));
        let mut expected = envelope.as_object().unwrap().clone();
        if let Some(overrides) = entry["envelope"].as_object() {
            expected.extend(overrides.clone());
        }
        expected.extend(entry["payload"].as_object().unwrap().clone());
        assert_eq!(
            json!(event),
            Value::Object(expected),
            "Payload drift: {kind}"
        );
        let fields: BTreeSet<_> = entry["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field["name"].as_str().unwrap().to_owned())
            .collect();
        let mut serialized_fields: BTreeSet<_> = json!(event.event)
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        serialized_fields.remove("kind");
        assert_eq!(
            fields, serialized_fields,
            "Field descriptions drift: {kind}"
        );
    }
    assert_eq!(documented, derived, "Missing or obsolete event section");
}

#[test]
fn every_documented_event_handler_runs_and_filters_other_kinds_safely() {
    let catalog = event_catalog();
    for entry in catalog["events"].as_array().unwrap() {
        let kind = entry["kind"].as_str().unwrap();
        let files = Files::from([(
            "main.rhai".into(),
            entry["handler"].as_str().unwrap().into(),
        )]);
        let mut event = catalog["envelope"].as_object().unwrap().clone();
        if let Some(overrides) = entry["envelope"].as_object() {
            event.extend(overrides.clone());
        }
        event.extend(entry["payload"].as_object().unwrap().clone());
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed = calls.clone();
        runtime::execute(
            files.clone(),
            "on_event",
            json!({"event": event}),
            Arc::new(move |method, _| {
                observed.lock().push(method.into());
                Ok(Value::Null)
            }),
        )
        .unwrap_or_else(|error| panic!("{kind} handler: {error}"));
        assert!(!calls.lock().is_empty(), "{kind} handler was not exercised");
        assert!(calls.lock().iter().all(|call| call.starts_with("log.")));
        runtime::execute(
            files.clone(),
            "on_event",
            json!({"event": {"kind": "not_this_event"}}),
            Arc::new(|_, _| panic!("Wrong-kind handler must not call a host")),
        )
        .unwrap_or_else(|error| panic!("{kind} accessed another event's fields: {error}"));
        // Nullable changes and common round evidence must not be dereferenced blindly.
        for field in ["ct", "t", "clip", "capacity", "reserve"] {
            if event.contains_key(field) {
                event.insert(field.into(), Value::Null);
            }
        }
        event.insert(
            "round".into(),
            json!({"completed_rounds": null, "phase": null, "winner": null}),
        );
        event.insert("player".into(), Value::Null);
        event.insert("match".into(), Value::Null);
        runtime::execute(
            files,
            "on_event",
            json!({"event": event}),
            Arc::new(|_, _| Ok(Value::Null)),
        )
        .unwrap_or_else(|error| panic!("{kind} nullable-field handler: {error}"));
    }
}

#[test]
fn documented_host_signatures_are_registered_and_every_overload_executes() {
    let api = catalog();
    assert!(include_str!("../../Cargo.toml").contains(&format!(
        "version = \"={}\"",
        api["rhai_version"].as_str().unwrap()
    )));
    let e = runtime::engine(Files::new(), Arc::new(|_, _| Ok(Value::Null)));
    let signatures = e.gen_fn_signatures(false);
    let registered: BTreeSet<_> = signatures
        .iter()
        .filter(|s| {
            s.contains("Capability")
                || s.contains("UserFilter")
                || s.contains("RewardFilter")
                || s.contains("Duration")
                || s.starts_with("last_rounds(")
        })
        .map(|s| s.split('(').next().unwrap().to_owned())
        .collect();
    let documented: BTreeSet<_> = api["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let ns = f["namespace"].as_str().unwrap();
            let name = f["name"].as_str().unwrap();
            if ns == "Duration" || name == "create" {
                format!("{ns}::{name}")
            } else {
                name.to_owned()
            }
        })
        .collect();
    assert_eq!(
        registered, documented,
        "Native binding added/removed without updating API catalog"
    );
    let native_arities: BTreeSet<_> = signatures
        .iter()
        .filter(|s| {
            s.contains("Capability")
                || s.contains("UserFilter")
                || s.contains("RewardFilter")
                || s.contains("Duration")
                || s.starts_with("last_rounds(")
        })
        .map(|s| {
            let parameters = s.split_once('(').unwrap().1.split(')').next().unwrap();
            (
                s.split('(').next().unwrap().to_owned(),
                if parameters.is_empty() {
                    0
                } else {
                    parameters.split(',').count()
                },
            )
        })
        .collect();
    let documented_arities: BTreeSet<_> = api["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let ns = f["namespace"].as_str().unwrap();
            let name = f["name"].as_str().unwrap();
            let static_fn = ns == "Duration" || name == "create";
            (
                if static_fn {
                    format!("{ns}::{name}")
                } else {
                    name.into()
                },
                f["parameters"].as_array().unwrap().len() + usize::from(!static_fn),
            )
        })
        .collect();
    assert_eq!(
        native_arities, documented_arities,
        "Native overload/arity drift"
    );
    for f in api["functions"].as_array().unwrap() {
        let ns = f["namespace"].as_str().unwrap();
        if ["storage", "scheduler", "rewards", "chat", "users"].contains(&ns) {
            assert!(
                include_str!("service.rs")
                    .contains(&format!("\"{ns}.{}\"", f["name"].as_str().unwrap())),
                "Catalog capability has no service implementation: {}",
                f["signature"]
            );
        }
        let source = format!("fn on_event(ctx) {{ {} }}", f["example"].as_str().unwrap());
        let files = Files::from([("main.rhai".into(), source)]);
        runtime::execute(
            files,
            "on_event",
            json!({"current_match":{"rounds":[]}}),
            Arc::new(|_, _| Ok(Value::Null)),
        )
        .unwrap_or_else(|error| panic!("Catalog invocation {}: {error}", f["signature"]));
    }
}

#[test]
fn context_catalog_covers_serialized_local_player_and_counter_properties() {
    let api = catalog();
    for (name, value) in [
        (
            "ctx.state.player",
            json!(crate::cs2::model::Player::default()),
        ),
        (
            "match_stats",
            json!(crate::cs2::model::MatchStats::default()),
        ),
        (
            "round_stats",
            json!(crate::cs2::model::RoundStats::default()),
        ),
        ("ctx.state.game", json!(crate::cs2::model::Game::default())),
        (
            "ctx.state.round",
            json!(crate::cs2::model::Round::default()),
        ),
        ("score", json!(crate::cs2::model::Score::default())),
        ("ammo", json!(crate::cs2::model::Ammo::default())),
        ("team_info", json!(crate::cs2::model::TeamInfo::default())),
    ] {
        assert_eq!(
            value.as_object().unwrap().keys().collect::<BTreeSet<_>>(),
            api["properties"][name]
                .as_object()
                .unwrap()
                .keys()
                .collect::<BTreeSet<_>>(),
            "Schema property drift in {name}"
        );
    }
}

#[test]
fn every_public_recipe_compiles_and_exercises_success_failure_empty_and_gap_paths() {
    let recipes: Value =
        serde_json::from_str(include_str!("../../docs/scripting/recipes.json")).unwrap();
    for recipe in recipes.as_array().unwrap() {
        let files: Files = serde_json::from_value(recipe["files"].clone()).unwrap();
        runtime::validate(&files).unwrap_or_else(|e| panic!("{}: {e}", recipe["id"]));
        for scenario in recipe["scenarios"].as_array().unwrap() {
            let calls = Arc::new(Mutex::new(Vec::<String>::new()));
            let observed = calls.clone();
            let failure = scenario["trigger_failure"] == true;
            let empty = scenario["empty_chat"] == true;
            let host: runtime::Host = Arc::new(move |method, _| {
                observed.lock().push(method.into());
                Ok(match method {
                    "storage.get" => json!(""),
                    "storage.increment" => json!(2),
                    "scheduler.exists" => json!(false),
                    "chat.recent_chatters" | "users.recent_chatters" => {
                        if empty {
                            json!([])
                        } else {
                            json!([{"id":"123","login":"viewer"}])
                        }
                    }
                    "chat.user_stats" | "users.user_stats" => json!({"messages":5}),
                    "rewards.trigger" => {
                        if failure {
                            json!({"ok":false,"code":"fulfillment_pending"})
                        } else {
                            json!({"ok":true})
                        }
                    }
                    _ => Value::Null,
                })
            });
            runtime::execute(
                files.clone(),
                scenario["entry"].as_str().unwrap(),
                scenario["context"].clone(),
                host,
            )
            .unwrap_or_else(|e| panic!("Recipe {} scenario {scenario}: {e}", recipe["id"]));
            let expected: Vec<String> = serde_json::from_value(scenario["calls"].clone()).unwrap();
            assert_eq!(*calls.lock(), expected, "Recipe {}", recipe["id"]);
        }
    }
}
