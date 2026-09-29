use super::{
    runtime::{self, Files, MessageFilter, MessageMode, MessageOperation, UserFilter},
    service,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct Fixture {
    pub(super) db: crate::db::Db,
    pub(super) channel: String,
}
impl Fixture {
    pub(super) async fn new() -> Self {
        Self {
            db: crate::db::Db::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable PostgreSQL required"),
            )
            .await
            .unwrap(),
            channel: format!("message-filter-{}", uuid::Uuid::new_v4()),
        }
    }
    pub(super) async fn message(&self, user: &str, login: &str, text: &str, age: f64) {
        sqlx::query("INSERT INTO chat_messages(message_id,broadcaster_id,chatter_user_id,chatter_user_login,message_text,char_count,sent_at) VALUES($1,$2,$3,$4,$5,$6,now()-$7*interval '1 second')")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&self.channel).bind(user)
            .bind(login).bind(text).bind(text.chars().count() as i32).bind(age)
            .execute(self.db.pool()).await.unwrap();
    }
    async fn query(&self, messages: Option<MessageFilter>) -> Value {
        service::recent_chatters(
            self.db.pool(),
            &self.channel,
            300,
            UserFilter {
                messages,
                ..UserFilter::default()
            },
            None,
        )
        .await
        .unwrap()
    }
    pub(super) async fn assert_filtered(
        &self,
        seconds: i64,
        filter: UserFilter,
        expected: &[&str],
    ) -> Value {
        let rows = service::recent_chatters(self.db.pool(), &self.channel, seconds, filter, None)
            .await
            .unwrap();
        let mut actual: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        actual.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(actual, expected);
        rows
    }
    async fn assert_users(&self, filter: MessageFilter, expected: &[&str]) {
        let rows = self.query(Some(filter)).await;
        let mut actual: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        actual.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
pub(super) fn any(op: MessageOperation, text: &str) -> MessageFilter {
    let mut filter = MessageFilter::new(MessageMode::Any)
        .clause(op, text)
        .unwrap();
    filter.seconds = Some(300);
    filter
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_contains_only_matches_message_content() {
    let f = Fixture::new().await;
    f.message("match", "viewer", "a dinosaur appeared", 10.0)
        .await;
    f.message("no", "viewer", "a lizard appeared", 10.0).await;
    f.assert_users(any(MessageOperation::Contains, "dinosaur"), &["match"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_starts_with_is_anchored() {
    let f = Fixture::new().await;
    f.message("match", "viewer", "beast incoming", 10.0).await;
    f.message("no", "viewer", "a beast incoming", 10.0).await;
    f.assert_users(any(MessageOperation::StartsWith, "beast"), &["match"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_ends_with_is_anchored() {
    let f = Fixture::new().await;
    f.message("match", "viewer", "hello dinosaur", 10.0).await;
    f.message("no", "viewer", "dinosaur hello", 10.0).await;
    f.assert_users(any(MessageOperation::EndsWith, "dinosaur"), &["match"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_equals_matches_whole_message_without_trimming() {
    let f = Fixture::new().await;
    f.message("match", "viewer", "dinosaur", 10.0).await;
    f.message("prefix", "viewer", "dinosaur!", 10.0).await;
    f.message("space", "viewer", " dinosaur", 10.0).await;
    f.assert_users(any(MessageOperation::Equals, "dinosaur"), &["match"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_any_combines_clauses_with_or() {
    let f = Fixture::new().await;
    f.message("contains", "viewer", "a dinosaur", 10.0).await;
    f.message("starts", "viewer", "beast here", 10.0).await;
    f.message("no", "viewer", "a beast", 10.0).await;
    let filter = any(MessageOperation::Contains, "dinosaur")
        .clause(MessageOperation::StartsWith, "beast")
        .unwrap();
    f.assert_users(filter, &["contains", "starts"]).await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_all_requires_all_clauses_in_the_same_message() {
    let f = Fixture::new().await;
    f.message("same", "viewer", "beast dinosaur", 10.0).await;
    f.message("split", "viewer", "beast here", 10.0).await;
    f.message("split", "viewer", "a dinosaur", 9.0).await;
    f.message("one", "viewer", "a dinosaur", 10.0).await;
    let mut filter = MessageFilter::new(MessageMode::All)
        .clause(MessageOperation::Contains, "dinosaur")
        .unwrap()
        .clause(MessageOperation::StartsWith, "beast")
        .unwrap();
    filter.seconds = Some(300);
    f.assert_users(filter, &["same"]).await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_default_and_explicit_false_ignore_case_for_ascii_and_cyrillic() {
    let f = Fixture::new().await;
    f.message("ascii", "viewer", "DINOSAUR", 10.0).await;
    f.message("unicode", "viewer", "ДИНОЗАВР ЁЖ", 10.0).await;
    for explicit_false in [false, true] {
        let mut filter = any(MessageOperation::Equals, "dinosaur")
            .clause(MessageOperation::Equals, "динозавр ёж")
            .unwrap();
        if explicit_false {
            filter.case_sensitive = false;
        }
        f.assert_users(filter, &["ascii", "unicode"]).await;
    }
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_sensitive_mode_applies_to_every_clause_including_cyrillic() {
    let f = Fixture::new().await;
    f.message("lower", "viewer", "динозавр ёж", 10.0).await;
    f.message("upper", "viewer", "ДИНОЗАВР ЁЖ", 10.0).await;
    f.message("ascii_lower", "viewer", "dinosaur", 10.0).await;
    f.message("ascii_upper", "viewer", "DINOSAUR", 10.0).await;
    let mut filter = any(MessageOperation::Contains, "динозавр")
        .clause(MessageOperation::StartsWith, "dino")
        .unwrap();
    filter.case_sensitive = true;
    f.assert_users(filter, &["lower", "ascii_lower"]).await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_unicode_all_operators_support_both_case_modes() {
    let f = Fixture::new().await;
    f.message("lower", "viewer", "зверь динозавр ёж", 10.0)
        .await;
    f.message("upper", "viewer", "ЗВЕРЬ ДИНОЗАВР ЁЖ", 10.0)
        .await;
    for (op, pattern) in [
        (MessageOperation::Contains, "динозавр"),
        (MessageOperation::StartsWith, "зверь"),
        (MessageOperation::EndsWith, "ёж"),
        (MessageOperation::Equals, "зверь динозавр ёж"),
    ] {
        for sensitive in [false, true] {
            let mut filter = any(op, pattern);
            filter.case_sensitive = sensitive;
            f.assert_users(
                filter,
                if sensitive {
                    &["lower"]
                } else {
                    &["lower", "upper"]
                },
            )
            .await;
        }
    }
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_window_and_channel_isolate_content_matches() {
    let f = Fixture::new().await;
    f.message("current", "viewer", "dinosaur", 10.0).await;
    f.message("expired", "viewer", "dinosaur", 400.0).await;
    f.message("expired", "viewer", "unrelated", 10.0).await;
    f.message("future", "viewer", "dinosaur", -60.0).await;
    let other = Fixture::new().await;
    other.message("elsewhere", "viewer", "dinosaur", 10.0).await;
    f.assert_users(any(MessageOperation::Contains, "dinosaur"), &["current"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_multiple_messages_preserve_all_activity_counts_and_thresholds() {
    let f = Fixture::new().await;
    f.message("multi", "viewer", "dinosaur", 10.0).await;
    f.message("multi", "viewer", "hello", 9.0).await;
    f.message("multi", "viewer", "dinosaur", 8.0).await;
    f.message("single", "viewer", "dinosaur", 10.0).await;
    let rows = service::recent_chatters(
        f.db.pool(),
        &f.channel,
        300,
        UserFilter {
            min_messages: 3,
            min_characters: 21,
            messages: Some(any(MessageOperation::Contains, "dinosaur")),
            ..UserFilter::default()
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["id"], "multi");
    assert_eq!(rows[0]["messages"], 3);
    assert_eq!(rows[0]["characters"], 21);
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_identity_strings_never_substitute_for_message_text() {
    let f = Fixture::new().await;
    // Chat storage has a login, not a separate display-name column. Neither identity is searched.
    f.message("динозавр", "ДИНОЗАВР_display_name", "hello", 10.0)
        .await;
    f.assert_users(any(MessageOperation::Contains, "динозавр"), &[])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_empty_messages_and_no_history_do_not_match() {
    let f = Fixture::new().await;
    f.assert_users(any(MessageOperation::Contains, "dinosaur"), &[])
        .await;
    f.message("empty", "viewer", "", 10.0).await;
    f.assert_users(any(MessageOperation::Contains, "dinosaur"), &[])
        .await;
    assert_eq!(f.query(None).await[0]["characters"], 0);
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_sql_wildcards_backslashes_and_injection_are_literal() {
    let f = Fixture::new().await;
    f.message("literal", "viewer", r"100%_\ ' OR true --", 10.0)
        .await;
    f.message("no", "viewer", "100ABC", 10.0).await;
    f.assert_users(any(MessageOperation::Contains, r"100%_\"), &["literal"])
        .await;
    f.assert_users(
        any(MessageOperation::Contains, "' OR true --"),
        &["literal"],
    )
    .await;
    f.assert_users(any(MessageOperation::Contains, "%"), &["literal"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_runtime_builder_reaches_query_with_global_case_toggle() {
    let f = Fixture::new().await;
    f.message("upper", "viewer", "ДИНОЗАВР", 10.0).await;
    let args = Arc::new(parking_lot::Mutex::new(Value::Null));
    let captured = args.clone();
    runtime::execute(Files::from([("main.rhai".into(),
        "fn on_event(ctx) { let filter = UserFilter::create().messages(MessageFilter::any().contains(\"динозавр\").case_sensitive(true).starts_with(\"зверь\").case_sensitive(false).during(Duration::from_mins(5))); chat.recent_chatters(Duration::from_mins(5), filter); }".into())]),
        "on_event", json!({}), Arc::new(move |method, value| {
            assert_eq!(method, "chat.recent_chatters");
            *captured.lock() = value;
            Ok(json!([]))
        })).unwrap();
    let args = args.lock().clone();
    let filter: UserFilter = serde_json::from_value(args[1].clone()).unwrap();
    assert!(!filter.messages.as_ref().unwrap().case_sensitive);
    let rows = service::recent_chatters(
        f.db.pool(),
        &f.channel,
        args[0].as_i64().unwrap(),
        filter,
        None,
    )
    .await
    .unwrap();
    assert_eq!(rows[0]["id"], "upper");
}

#[test]
fn runtime_message_filter_rejects_empty_long_nul_and_excessive_clauses_without_host_calls() {
    for expression in [
        "MessageFilter::any()".to_owned(),
        "MessageFilter::all()".to_owned(),
        "MessageFilter::any().contains(\"\")".to_owned(),
        format!("MessageFilter::any().contains(\"{}\")", "я".repeat(257)),
        "MessageFilter::any().contains(\"\\u0000\")".to_owned(),
        format!("MessageFilter::any(){}", ".contains(\"a\")".repeat(17)),
    ] {
        let error = runtime::execute(Files::from([("main.rhai".into(), format!("fn on_event(ctx) {{ chat.recent_chatters(Duration::from_mins(5), UserFilter::create().messages({expression})); }}"))]),
            "on_event", json!({}), Arc::new(|_, _| panic!("invalid filter must not call host"))).unwrap_err();
        assert!(
            error.contains("invalid_message_filter"),
            "{expression}: {error}"
        );
    }
}
#[test]
fn runtime_message_filter_limits_count_unicode_scalars_not_bytes() {
    let pattern = "🦕".repeat(256);
    let mut filter = any(MessageOperation::Contains, &pattern);
    for _ in 1..16 {
        filter = filter.clause(MessageOperation::Equals, "я").unwrap();
    }
    assert!(filter.validate().is_ok());
    assert!(filter.clause(MessageOperation::Contains, "a").is_err());
}
#[test]
fn runtime_old_user_filter_serialization_remains_compatible() {
    let filter: UserFilter =
        serde_json::from_value(json!({"min_messages": 2,"min_characters": 5,"reward": null}))
            .unwrap();
    assert!(filter.messages.is_none());
    assert!(filter.activity.is_none());
}
#[tokio::test]
async fn service_revalidates_message_filter_before_database_access() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    for messages in [
        MessageFilter::new(MessageMode::All),
        MessageFilter {
            mode: MessageMode::Any,
            case_sensitive: false,
            seconds: None,
            clauses: vec![runtime::MessageClause {
                operation: MessageOperation::Contains,
                text: "я".repeat(257),
            }],
        },
        MessageFilter {
            mode: MessageMode::Any,
            case_sensitive: false,
            seconds: None,
            clauses: vec![
                runtime::MessageClause {
                    operation: MessageOperation::Equals,
                    text: "a".into()
                };
                17
            ],
        },
    ] {
        let result = service::recent_chatters(
            &pool,
            "channel",
            300,
            UserFilter {
                messages: Some(messages),
                ..UserFilter::default()
            },
            None,
        )
        .await;
        assert_eq!(result.unwrap_err(), "invalid_message_filter");
    }
}
