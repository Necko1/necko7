use parking_lot::Mutex;
use rand::seq::SliceRandom;
use rhai::{
    AST, Array, Dynamic, Engine, EvalAltResult, FuncRegistration, Map, Module, ModuleResolver,
    Position, Scope,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Instant};

pub type Files = BTreeMap<String, String>;
type Result<T> = std::result::Result<T, Box<EvalAltResult>>;
pub type Host = Arc<dyn Fn(&str, Value) -> std::result::Result<Value, String> + Send + Sync>;

pub fn valid_path(path: &str) -> bool {
    path.len() <= 180
        && path.ends_with(".rhai")
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}
pub fn validate_files(files: &Files) -> std::result::Result<(), String> {
    if !files.contains_key("main.rhai")
        || files.len() > 64
        || files.values().map(String::len).sum::<usize>() > 262144
    {
        return Err("Project requires main.rhai, at most 64 files and 256 KiB of source".into());
    }
    if files.keys().any(|p| !valid_path(p)) {
        return Err("Invalid project path".into());
    }
    Ok(())
}

/// The only resolver installed on the engine. It cannot touch the filesystem.
struct Resolver {
    files: Files,
    stack: Mutex<Vec<String>>,
}
impl ModuleResolver for Resolver {
    fn resolve(
        &self,
        engine: &Engine,
        _: Option<&str>,
        path: &str,
        pos: Position,
    ) -> Result<Arc<Module>> {
        let key = if path.ends_with(".rhai") {
            path.to_owned()
        } else {
            format!("{path}.rhai")
        };
        if !valid_path(&key) {
            return Err("Invalid import path".into());
        }
        let source = self
            .files
            .get(&key)
            .ok_or_else(|| Box::new(EvalAltResult::ErrorModuleNotFound(path.into(), pos)))?;
        {
            let mut stack = self.stack.lock();
            if stack.len() >= 16 || stack.contains(&key) {
                return Err("Circular import or module depth exceeded".into());
            }
            stack.push(key.clone());
        }
        let result = (|| {
            let mut ast = engine.compile_into_self_contained(&Scope::new(), source)?;
            ast.set_source(&key);
            Module::eval_ast_as_new(Scope::new(), &ast, engine).map(Arc::new)
        })();
        self.stack.lock().pop();
        result
    }
}

#[derive(Clone)]
struct Capability {
    name: &'static str,
    host: Host,
    calls: Arc<Mutex<BTreeMap<String, usize>>>,
}
impl Capability {
    fn call(&mut self, method: &str, args: Value) -> Result<Dynamic> {
        let name = format!("{}.{method}", self.name);
        let mut calls = self.calls.lock();
        let total = calls.entry("total".into()).or_default();
        *total += 1;
        if *total > 100 {
            return Err("Host call budget exceeded".into());
        }
        let count = calls.entry(name.clone()).or_default();
        *count += 1;
        let limit = match name.as_str() {
            "rewards.trigger" => 3,
            "chat.send" | "chat.reply" => 3,
            "rewards.set_visible" | "rewards.set_paused" | "rewards.enable_for" => 10,
            _ => 50,
        };
        if *count > limit {
            return Err("Capability budget exceeded".into());
        }
        drop(calls);
        let value = (self.host)(&name, args).map_err(|e| -> Box<EvalAltResult> { e.into() })?;
        rhai::serde::to_dynamic(value)
    }
}
#[derive(Clone, Copy)]
pub struct Duration(pub i64);
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct UserFilter {
    pub min_messages: i64,
    pub min_characters: i64,
    pub reward: Option<RewardFilter>,
    #[serde(default)]
    pub messages: Option<MessageFilter>,
    #[serde(default)]
    pub activity: Option<ActivityFilter>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ActivityFilter {
    pub min_messages: i64,
    pub min_characters: i64,
    pub seconds: Option<i64>,
}
impl ActivityFilter {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.min_messages < 0 || self.min_characters < 0 {
            return Err("invalid_activity_filter".into());
        }
        if self
            .seconds
            .is_some_and(|seconds| !(1..=31_536_000).contains(&seconds))
        {
            return Err("invalid_activity_filter".into());
        }
        Ok(())
    }
}
pub const MAX_MESSAGE_CLAUSES: usize = 16;
pub const MAX_MESSAGE_PATTERN_CHARS: usize = 256;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    Any,
    All,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOperation {
    Contains,
    StartsWith,
    EndsWith,
    Equals,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MessageClause {
    pub operation: MessageOperation,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MessageFilter {
    pub mode: MessageMode,
    #[serde(default)]
    pub case_sensitive: bool,
    pub clauses: Vec<MessageClause>,
    #[serde(default)]
    pub seconds: Option<i64>,
}
impl MessageFilter {
    pub fn new(mode: MessageMode) -> Self {
        Self {
            mode,
            case_sensitive: false,
            clauses: Vec::new(),
            seconds: None,
        }
    }
    fn valid_pattern(text: &str) -> bool {
        !text.is_empty()
            && !text.contains('\0')
            && text.chars().take(MAX_MESSAGE_PATTERN_CHARS + 1).count() <= MAX_MESSAGE_PATTERN_CHARS
    }
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.clauses.is_empty()
            || self.clauses.len() > MAX_MESSAGE_CLAUSES
            || self.clauses.iter().any(|c| !Self::valid_pattern(&c.text))
        {
            return Err("invalid_message_filter".into());
        }
        if self
            .seconds
            .is_some_and(|seconds| !(1..=31_536_000).contains(&seconds))
        {
            return Err("invalid_message_filter".into());
        }
        Ok(())
    }
    pub fn clause(mut self, operation: MessageOperation, text: &str) -> Result<Self> {
        if self.clauses.len() >= MAX_MESSAGE_CLAUSES || !Self::valid_pattern(text) {
            return Err("invalid_message_filter".into());
        }
        self.clauses.push(MessageClause {
            operation,
            text: text.into(),
        });
        Ok(self)
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RewardFilter {
    pub alias: Option<String>,
    pub statuses: Vec<String>,
    pub min_count: i64,
    pub seconds: Option<i64>,
}
impl RewardFilter {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.min_count < 0 {
            return Err("invalid_reward_filter".into());
        }
        if self
            .seconds
            .is_some_and(|seconds| !(60..=31_536_000).contains(&seconds))
        {
            return Err("invalid_reward_filter".into());
        }
        Ok(())
    }
}

fn safe_value(value: Dynamic) -> Result<Value> {
    let value: Value = rhai::serde::from_dynamic(&value)?;
    if value.to_string().len() > 65536 {
        return Err("Value exceeds 64 KiB".into());
    }
    Ok(value)
}

pub fn engine(files: Files, host: Host) -> Engine {
    let mut e = Engine::new();
    e.set_module_resolver(Resolver {
        files,
        stack: Mutex::new(Vec::new()),
    });
    e.disable_symbol("eval");
    e.set_max_operations(100_000)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 32)
        .set_max_string_size(65536)
        .set_max_array_size(2048)
        .set_max_map_size(512)
        .set_max_modules(64);
    let start = Instant::now();
    e.on_progress(move |_| {
        (start.elapsed().as_secs() >= 3).then(|| Dynamic::from("Execution deadline exceeded"))
    });
    e.on_print(|_| {});
    e.on_debug(|_, _, _| {});
    let mut globals = Module::new();
    let calls = Arc::new(Mutex::new(BTreeMap::new()));
    for name in [
        "storage",
        "scheduler",
        "rewards",
        "chat",
        "users",
        "log",
        "random",
    ] {
        globals.set_var(
            name,
            Capability {
                name,
                host: host.clone(),
                calls: calls.clone(),
            },
        );
    }
    e.register_global_module(globals.into());
    let mut duration = Module::new();
    for (name, multiplier) in [
        ("from_secs", 1),
        ("from_mins", 60),
        ("from_hours", 3600),
        ("from_days", 86400),
        ("from_weeks", 604800),
    ] {
        FuncRegistration::new(name).set_into_module(
            &mut duration,
            move |n: i64| -> Result<Duration> {
                let seconds = n
                    .checked_mul(multiplier)
                    .filter(|n| *n > 0 && *n <= 31_536_000)
                    .ok_or("Duration must be positive and at most one year")?;
                Ok(Duration(seconds))
            },
        );
    }
    e.register_static_module("Duration", duration.into());
    let mut users = Module::new();
    FuncRegistration::new("create").set_into_module(&mut users, UserFilter::default);
    e.register_static_module("UserFilter", users.into());
    let mut activity = Module::new();
    FuncRegistration::new("create").set_into_module(&mut activity, ActivityFilter::default);
    e.register_static_module("ActivityFilter", activity.into());
    e.register_fn(
        "min_messages",
        |mut f: ActivityFilter, n: i64| -> Result<ActivityFilter> {
            if n < 0 {
                return Err("invalid_activity_filter".into());
            }
            f.min_messages = n;
            Ok(f)
        },
    );
    e.register_fn(
        "min_characters",
        |mut f: ActivityFilter, n: i64| -> Result<ActivityFilter> {
            if n < 0 {
                return Err("invalid_activity_filter".into());
            }
            f.min_characters = n;
            Ok(f)
        },
    );
    e.register_fn("during", |mut f: ActivityFilter, d: Duration| {
        f.seconds = Some(d.0);
        f
    });
    e.register_fn(
        "activity",
        |mut f: UserFilter, activity: ActivityFilter| -> Result<UserFilter> {
            activity
                .validate()
                .map_err(|error| -> Box<EvalAltResult> { error.into() })?;
            f.activity = Some(activity);
            Ok(f)
        },
    );
    let mut messages = Module::new();
    FuncRegistration::new("any")
        .set_into_module(&mut messages, || MessageFilter::new(MessageMode::Any));
    FuncRegistration::new("all")
        .set_into_module(&mut messages, || MessageFilter::new(MessageMode::All));
    e.register_static_module("MessageFilter", messages.into());
    for (name, operation) in [
        ("contains", MessageOperation::Contains),
        ("starts_with", MessageOperation::StartsWith),
        ("ends_with", MessageOperation::EndsWith),
        ("equals", MessageOperation::Equals),
    ] {
        e.register_fn(name, move |f: MessageFilter, text: &str| {
            f.clause(operation, text)
        });
    }
    e.register_fn("case_sensitive", |mut f: MessageFilter, enabled: bool| {
        f.case_sensitive = enabled;
        f
    });
    e.register_fn("during", |mut f: MessageFilter, d: Duration| {
        f.seconds = Some(d.0);
        f
    });
    e.register_fn(
        "messages",
        |mut f: UserFilter, messages: MessageFilter| -> Result<UserFilter> {
            messages
                .validate()
                .map_err(|error| -> Box<EvalAltResult> { error.into() })?;
            f.messages = Some(messages);
            Ok(f)
        },
    );
    let mut rewards = Module::new();
    FuncRegistration::new("create").set_into_module(&mut rewards, RewardFilter::default);
    e.register_static_module("RewardFilter", rewards.into());
    e.register_fn("min_messages", |mut f: UserFilter, n: i64| {
        f.min_messages = n;
        f
    });
    e.register_fn("min_characters", |mut f: UserFilter, n: i64| {
        f.min_characters = n;
        f
    });
    e.register_fn(
        "reward_redemptions",
        |mut f: UserFilter, r: RewardFilter| -> Result<UserFilter> {
            r.validate()
                .map_err(|error| -> Box<EvalAltResult> { error.into() })?;
            f.reward = Some(r);
            Ok(f)
        },
    );
    e.register_fn("reward", |mut f: RewardFilter, alias: &str| {
        f.alias = Some(alias.into());
        f
    });
    e.register_fn("min_count", |mut f: RewardFilter, n: i64| {
        f.min_count = n;
        f
    });
    e.register_fn("during", |mut f: RewardFilter, d: Duration| {
        f.seconds = Some(d.0);
        f
    });
    e.register_fn(
        "statuses",
        |mut f: RewardFilter, items: Array| -> Result<RewardFilter> {
            f.statuses = items
                .into_iter()
                .map(|v| {
                    v.into_string()
                        .map_err(|_| "Expected status strings".into())
                })
                .collect::<Result<_>>()?;
            Ok(f)
        },
    );
    e.register_fn("get", |c: &mut Capability, key: &str| {
        c.call("get", json!([key]))
    });
    e.register_fn("get", |c: &mut Capability, key: &str, default: Dynamic| {
        c.call("get", json!([key, safe_value(default)?]))
    });
    e.register_fn("set", |c: &mut Capability, key: &str, value: Dynamic| {
        c.call("set", json!([key, safe_value(value)?]))
    });
    e.register_fn("increment", |c: &mut Capability, key: &str, amount: i64| {
        c.call("increment", json!([key, amount]))
    });
    for method in ["delete", "cancel", "exists"] {
        e.register_fn(method, move |c: &mut Capability, key: &str| {
            c.call(method, json!([key]))
        });
    }
    for method in ["set_visible", "set_paused"] {
        e.register_fn(method, move |c: &mut Capability, key: &str, flag: bool| {
            c.call(method, json!([key, flag]))
        });
    }
    e.register_fn(
        "enable_for",
        |c: &mut Capability, key: &str, d: Duration| c.call("enable_for", json!([key, d.0])),
    );
    e.register_fn("trigger", |c: &mut Capability, key: &str, user: &str| {
        c.call("trigger", json!([key, user]))
    });
    e.register_fn(
        "after",
        |c: &mut Capability, key: &str, d: Duration, value: Dynamic| {
            c.call("after", json!([key, d.0, safe_value(value)?]))
        },
    );
    e.register_fn(
        "recent_chatters",
        |c: &mut Capability, d: Duration, filter: UserFilter| {
            c.call("recent_chatters", json!([d.0, filter]))
        },
    );
    e.register_fn(
        "user_stats",
        |c: &mut Capability, user: &str, d: Duration| c.call("user_stats", json!([user, d.0])),
    );
    e.register_fn("send", |c: &mut Capability, message: &str| {
        c.call("send", json!([message]))
    });
    e.register_fn("reply", |c: &mut Capability, id: &str, message: &str| {
        c.call("reply", json!([id, message]))
    });
    // Rhai's reserved debug() call expects a string from the native overload.
    // The report receives log.debug; bare debug output remains suppressed.
    e.register_fn("debug", |c: &mut Capability, message: &str| -> Result<String> {
        let _ = c.call("debug", json!([message]))?;
        Ok(message.to_owned())
    });
    for level in ["info", "warn", "error"] {
        e.register_fn(level, move |c: &mut Capability, message: &str| {
            c.call(level, json!([message]))
        });
    }
    e.register_fn(
        "pick",
        |c: &mut Capability, items: Array| -> Result<Dynamic> {
            if c.name != "random" {
                return Err("Unsupported capability".into());
            }
            Ok(items
                .choose(&mut rand::thread_rng())
                .cloned()
                .unwrap_or(Dynamic::UNIT))
        },
    );
    e.register_fn("last_rounds", |m: &mut Map, n: i64| -> Result<Array> {
        if !(0..=256).contains(&n) {
            return Err("Round count must be 0..256".into());
        }
        let rounds = m
            .get("rounds")
            .and_then(|r| r.clone().try_cast::<Array>())
            .unwrap_or_default();
        let completed: Vec<_> = rounds
            .into_iter()
            .filter(|r| {
                r.clone()
                    .try_cast::<Map>()
                    .and_then(|m| m.get("completed").cloned())
                    .and_then(|v| v.try_cast::<bool>())
                    .unwrap_or(false)
            })
            .collect();
        Ok(completed[completed.len().saturating_sub(n as usize)..].to_vec())
    });
    e
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Handlers {
    pub has_on_event: bool,
    pub has_on_timer: bool,
}
pub fn compile(e: &Engine, files: &Files) -> std::result::Result<(AST, Handlers), String> {
    validate_files(files)?;
    // Compile every file, including orphaned files. Imports are resolved eagerly.
    for (path, source) in files {
        let ast = e
            .compile_into_self_contained(&Scope::new(), source)
            .map_err(|err| format!("{path}: {err}"))?;
        e.run_ast(&ast).map_err(|err| format!("{path}: {err}"))?;
    }
    let mut ast = e
        .compile_into_self_contained(&Scope::new(), &files["main.rhai"])
        .map_err(|err| format!("main.rhai: {err}"))?;
    ast.set_source("main.rhai");
    let has = |name| {
        ast.iter_functions()
            .any(|f| f.name == name && f.params.len() == 1)
    };
    let handlers = Handlers {
        has_on_event: has("on_event"),
        has_on_timer: has("on_timer"),
    };
    Ok((ast, handlers))
}
pub fn validate(files: &Files) -> std::result::Result<Handlers, String> {
    let e = engine(
        files.clone(),
        Arc::new(|_, _| Err("Host actions are forbidden in module initialization".into())),
    );
    compile(&e, files).map(|(_, h)| h)
}
pub fn execute(
    files: Files,
    entry: &str,
    context: Value,
    host: Host,
) -> std::result::Result<(), String> {
    // Never run module initialization with live capabilities during compilation.
    let start = Instant::now();
    validate(&files)?;
    let mut e = engine(files.clone(), host);
    e.on_progress(move |_| {
        (start.elapsed().as_secs() >= 3).then(|| Dynamic::from("Execution deadline exceeded"))
    });
    let ast = e.compile(&files["main.rhai"]).map_err(|e| e.to_string())?;
    let context = rhai::serde::to_dynamic(context).map_err(|e| e.to_string())?;
    e.call_fn::<Dynamic>(&mut Scope::new(), &ast, entry, (context,))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files(source: &str) -> Files {
        BTreeMap::from([("main.rhai".into(), source.into())])
    }
    fn host() -> Host {
        Arc::new(|_, _| Ok(Value::Null))
    }
    #[test]
    fn paths_are_project_relative() {
        for p in [
            "../x.rhai",
            "a/../x.rhai",
            "/x.rhai",
            "C:/x.rhai",
            "a\\b.rhai",
            ".x.rhai",
            "a//x.rhai",
        ] {
            assert!(!valid_path(p), "{p}");
        }
        assert!(valid_path("events/kill.rhai"));
    }
    #[test]
    fn native_rhai_keywords_and_diagnostics_are_preserved() {
        for source in [
            "fn on_event(ctx) { ctx.match; }",
            "fn on_event(ctx) { UserFilter::new(); }",
        ] {
            let error = validate(&files(source)).unwrap_err();
            assert!(error.contains("reserved keyword"), "{error}");
            assert!(
                error.contains("main.rhai") && error.contains("line 1"),
                "{error}"
            );
        }
        execute(files("fn on_event(ctx) { let f=UserFilter::create(); if ctx.current_match.rounds.len != 0 { throw \"rounds\"; } if ctx.state[\"match\"].map != \"de_test\" { throw \"map\"; } }"),"on_event",json!({"current_match":{"rounds":[]},"state":{"match":{"map":"de_test"}}}),host()).unwrap();
    }
    #[test]
    fn metadata_and_multifile() {
        let mut f =
            files("import \"events/kill\" as kill; fn on_event(ctx) { kill::handle(ctx); }");
        f.insert(
            "events/kill.rhai".into(),
            "fn handle(ctx) { if ctx.event.kind != \"player_kill\" { throw \"wrong event\"; } }"
                .into(),
        );
        let h = validate(&f).unwrap();
        assert!(h.has_on_event);
        assert!(!h.has_on_timer);
        execute(
            f,
            "on_event",
            json!({"event":{"kind":"player_kill"}}),
            host(),
        )
        .unwrap();
    }
    #[test]
    fn missing_and_circular_imports_fail() {
        assert!(validate(&files("import \"../secret\" as x; fn on_event(ctx) {}")).is_err());
        assert!(validate(&files("import \"main\" as x;")).is_err());
        let mut f = files("fn on_event(ctx) {}");
        f.insert("unused.rhai".into(), "let x = ;".into());
        assert!(validate(&f).is_err());
    }
    #[test]
    fn forbidden_capabilities_and_instruction_budget() {
        for source in [
            "read_file(\"secret\");",
            "eval(\"1+1\");",
            "http.get(\"https://example.com\");",
            "loop {}",
            "let x=[]; loop { x.push(1); }",
        ] {
            assert!(
                execute(
                    files(&format!("fn on_event(ctx) {{ {source} }}")),
                    "on_event",
                    json!({}),
                    host()
                )
                .is_err()
            );
        }
    }
    #[test]
    fn duration_filter_and_capabilities_in_modules() {
        execute(files("fn on_event(ctx) { let f=UserFilter::create().min_messages(3).reward_redemptions(RewardFilter::create().reward(\"skin\").statuses([\"COMPLETED\"]).min_count(1).during(Duration::from_days(7))); chat.recent_chatters(Duration::from_mins(30), f); }"),"on_event",json!({}),host()).unwrap();
        assert!(
            execute(
                files("fn on_event(ctx) { Duration::from_days(999999999999999); }"),
                "on_event",
                json!({}),
                host()
            )
            .is_err()
        );
    }
    #[test]
    fn budget_stops_expensive_loop() {
        let calls = Arc::new(Mutex::new(0));
        let c = calls.clone();
        let h: Host = Arc::new(move |_, _| {
            *c.lock() += 1;
            Ok(json!({"ok":true}))
        });
        assert!(
            execute(
                files("fn on_event(ctx) { for n in 0..100 { rewards.trigger(\"skin\",\"1\"); } }"),
                "on_event",
                json!({}),
                h
            )
            .is_err()
        );
        assert_eq!(*calls.lock(), 3);
    }
}

#[cfg(test)]
mod documented_examples {
    use super::*;
    #[test]
    fn bundled_project_compiles_and_routes_round_history() {
        let f = Files::from([
            (
                "main.rhai".into(),
                include_str!("../../docs/scripting/examples/main.rhai").into(),
            ),
            (
                "events/rounds.rhai".into(),
                include_str!("../../docs/scripting/examples/events/rounds.rhai").into(),
            ),
            (
                "events/giveaway.rhai".into(),
                include_str!("../../docs/scripting/examples/events/giveaway.rhai").into(),
            ),
            (
                "timers/router.rhai".into(),
                include_str!("../../docs/scripting/examples/timers/router.rhai").into(),
            ),
        ]);
        let h = validate(&f).unwrap();
        assert!(h.has_on_event && h.has_on_timer);
        let actions = Arc::new(Mutex::new(Vec::new()));
        let recorded = actions.clone();
        execute(f,"on_event",json!({"event":{"kind":"round_ended"},"current_match":{"rounds":[
            {"index":0,"completed":true,"player":{"kills":3}},{"index":1,"completed":true,"player":{"kills":4}},{"index":2,"completed":true,"player":{"kills":3}}
        ]}}),Arc::new(move |method,_|{recorded.lock().push(method.to_owned());Ok(Value::Null)})).unwrap();
        assert_eq!(*actions.lock(), vec!["rewards.enable_for"]);
    }
}
