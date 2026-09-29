use super::runtime::{self, Files};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

fn catalog() -> Value {
    serde_json::from_str(include_str!("../../docs/scripting/api.json")).unwrap()
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
