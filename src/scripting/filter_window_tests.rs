use super::{
    message_filter_tests::{Fixture, any},
    runtime::{self, ActivityFilter, Files, MessageOperation, RewardFilter, UserFilter},
    service,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

fn activity(messages: i64, characters: i64, seconds: i64) -> UserFilter {
    UserFilter {
        activity: Some(ActivityFilter {
            min_messages: messages,
            min_characters: characters,
            seconds: Some(seconds),
        }),
        ..UserFilter::default()
    }
}
fn messages(text: &str, seconds: i64) -> UserFilter {
    let mut filter = any(MessageOperation::Contains, text);
    filter.seconds = Some(seconds);
    UserFilter {
        messages: Some(filter),
        ..UserFilter::default()
    }
}
async fn reward(f: &Fixture, alias: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1) ON CONFLICT DO NOTHING")
        .bind(&f.channel)
        .execute(f.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now()) ON CONFLICT DO NOTHING")
        .bind(&f.channel).execute(f.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO rewards(twitch_id,streamer_id,is_paused,script_alias,market_item_name,twitch_title,twitch_description,current_market_price,permissible_market_price_deviation,twitch_price_markup_percentage,global_cooldown_seconds,max_redemptions_per_stream,max_redemptions_per_user_per_stream,market_autobuy,created_at,updated_at) VALUES($1,$2,false,$3,'Test item','Test','',100,0,0,0,0,0,false,now(),now())")
        .bind(id).bind(&f.channel).bind(alias).execute(f.db.pool()).await.unwrap();
    id
}
async fn redemption(f: &Fixture, reward: Uuid, user: &str, age: f64, status: &str) {
    sqlx::query("INSERT INTO redemptions(fulfillment_id,twitch_reward_id,user_id,user_login,user_trade_link,twitch_points_cost,status,created_at,updated_at) VALUES($1,$2,$3,$3,'',0,$4,now()-$5*interval '1 second',now())")
        .bind(Uuid::new_v4()).bind(reward).bind(user).bind(status).bind(age)
        .execute(f.db.pool()).await.unwrap();
}
fn reward_filter(seconds: i64) -> RewardFilter {
    RewardFilter {
        alias: Some("secret_case".into()),
        min_count: 2,
        statuses: vec!["COMPLETED".into()],
        seconds: Some(seconds),
    }
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_candidate_window_is_independent_from_thirty_minute_activity() {
    let f = Fixture::new().await;
    for age in [600.0, 700.0, 800.0, 900.0] {
        f.message("eligible", "viewer", &"a".repeat(125), age).await;
        f.message("not_recent", "viewer", &"a".repeat(125), age)
            .await;
    }
    f.message("eligible", "viewer", "hello", 10.0).await;
    f.message("not_recent", "viewer", "hello", 1000.0).await;
    let rows = f
        .assert_filtered(300, activity(5, 500, 1800), &["eligible"])
        .await;
    assert_eq!(
        rows[0]["messages"], 1,
        "Returned activity is the candidate window, not nested eligibility"
    );
    assert_eq!(rows[0]["characters"], 5);
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_activity_message_threshold_uses_only_its_own_shorter_window() {
    let f = Fixture::new().await;
    for user in ["eligible", "old_messages"] {
        f.message(user, "viewer", "hello", 10.0).await;
    }
    f.message("eligible", "viewer", "hello", 20.0).await;
    f.message("old_messages", "viewer", "hello", 120.0).await;
    f.assert_filtered(300, activity(2, 0, 60), &["eligible"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_activity_character_threshold_uses_only_its_own_window() {
    let f = Fixture::new().await;
    for user in ["eligible", "expired", "future"] {
        f.message(user, "viewer", "hello", 10.0).await;
    }
    f.message("eligible", "viewer", &"я".repeat(500), 600.0)
        .await;
    f.message("expired", "viewer", &"я".repeat(500), 2000.0)
        .await;
    f.message("future", "viewer", &"я".repeat(500), -100.0)
        .await;
    f.assert_filtered(60, activity(0, 500, 1800), &["eligible"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_message_window_can_be_longer_than_candidate_window() {
    let f = Fixture::new().await;
    for user in ["eligible", "expired"] {
        f.message(user, "viewer", "hello", 10.0).await;
    }
    f.message("eligible", "viewer", "ДИНОЗАВР", 500.0).await;
    f.message("expired", "viewer", "ДИНОЗАВР", 700.0).await;
    f.message("not_recent", "viewer", "ДИНОЗАВР", 100.0).await;
    f.assert_filtered(60, messages("динозавр", 600), &["eligible"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_message_window_can_be_shorter_than_candidate_window() {
    let f = Fixture::new().await;
    f.message("eligible", "viewer", "динозавр", 10.0).await;
    f.message("old_message", "viewer", "динозавр", 120.0).await;
    f.message("old_message", "viewer", "hello", 10.0).await;
    f.assert_filtered(300, messages("динозавр", 60), &["eligible"])
        .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_reward_window_is_independent_and_keeps_alias_status_channel_boundaries() {
    let f = Fixture::new().await;
    let r = reward(&f, "secret_case").await;
    let unrelated = reward(&f, "other").await;
    for user in [
        "eligible",
        "expired",
        "wrong_status",
        "wrong_alias",
        "wrong_channel",
        "future",
    ] {
        f.message(user, "viewer", "hello", 10.0).await;
    }
    for age in [2.0 * 86400.0, 3.0 * 86400.0] {
        redemption(&f, r, "eligible", age, "COMPLETED").await;
    }
    for _ in 0..2 {
        redemption(&f, r, "expired", 8.0 * 86400.0, "COMPLETED").await;
        redemption(&f, r, "wrong_status", 86400.0, "PENDING").await;
        redemption(&f, unrelated, "wrong_alias", 86400.0, "COMPLETED").await;
        redemption(&f, r, "future", -86400.0, "COMPLETED").await;
    }
    let other = Fixture::new().await;
    let other_reward = reward(&other, "secret_case").await;
    for _ in 0..2 {
        redemption(&other, other_reward, "wrong_channel", 86400.0, "COMPLETED").await;
    }
    f.assert_filtered(
        60,
        UserFilter {
            reward: Some(reward_filter(7 * 86400)),
            ..UserFilter::default()
        },
        &["eligible"],
    )
    .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_combined_filters_use_four_independent_windows_without_join_multiplication() {
    let f = Fixture::new().await;
    let r = reward(&f, "secret_case").await;
    for user in [
        "eligible",
        "no_activity",
        "no_message",
        "no_reward",
        "not_recent",
    ] {
        f.message(
            user,
            "viewer",
            "hello",
            if user == "not_recent" { 100.0 } else { 10.0 },
        )
        .await;
        if user != "no_activity" {
            for age in [600.0, 700.0] {
                f.message(user, "viewer", "history", age).await;
            }
        }
        if user != "no_message" {
            f.message(user, "viewer", "ДИНОЗАВР", 200.0).await;
        }
        if user != "no_reward" {
            for age in [86400.0, 2.0 * 86400.0] {
                redemption(&f, r, user, age, "COMPLETED").await;
            }
        }
    }
    let mut filter = activity(3, 0, 1800);
    filter.messages = messages("динозавр", 300).messages;
    filter.reward = Some(reward_filter(7 * 86400));
    let rows = f.assert_filtered(60, filter, &["eligible"]).await;
    assert_eq!(rows[0]["messages"], 1);
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_independent_message_window_preserves_global_case_mode_and_same_message_and() {
    let f = Fixture::new().await;
    for user in ["lower", "upper", "split"] {
        f.message(user, "viewer", "hello", 10.0).await;
    }
    f.message("lower", "viewer", "зверь динозавр", 400.0).await;
    f.message("upper", "viewer", "ЗВЕРЬ ДИНОЗАВР", 400.0).await;
    f.message("split", "viewer", "зверь здесь", 400.0).await;
    f.message("split", "viewer", "динозавр", 401.0).await;
    for sensitive in [false, true] {
        let mut filter = any(MessageOperation::Contains, "динозавр")
            .clause(MessageOperation::StartsWith, "зверь")
            .unwrap();
        filter.mode = runtime::MessageMode::All;
        filter.seconds = Some(600);
        filter.case_sensitive = sensitive;
        f.assert_filtered(
            60,
            UserFilter {
                messages: Some(filter),
                ..UserFilter::default()
            },
            if sensitive {
                &["lower"]
            } else {
                &["lower", "upper"]
            },
        )
        .await;
    }
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_legacy_thresholds_keep_candidate_window_and_zero_activity_remains_known_zero() {
    let f = Fixture::new().await;
    f.message("eligible", "viewer", "hello", 120.0).await;
    f.message("eligible", "viewer", "hello", 130.0).await;
    f.assert_filtered(300, activity(0, 0, 60), &["eligible"])
        .await;
    f.assert_filtered(60, activity(0, 0, 1800), &[]).await;
    let mut mixed = activity(2, 10, 1800);
    mixed.min_messages = 2;
    mixed.min_characters = 10;
    f.assert_filtered(300, mixed, &["eligible"]).await;
    f.message("only_recent_one", "viewer", "hello", 10.0).await;
    f.message("only_recent_one", "viewer", "hello", 600.0).await;
    f.assert_filtered(
        300,
        UserFilter {
            min_messages: 2,
            min_characters: 10,
            ..UserFilter::default()
        },
        &["eligible"],
    )
    .await;
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_candidate_results_are_limited_after_nested_filtering_not_before() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO chat_messages(message_id,broadcaster_id,chatter_user_id,chatter_user_login,message_text,char_count,sent_at) SELECT $1||n,$2,'user-'||n,'viewer','hello',5,now()-interval '10 seconds' FROM generate_series(1,1001) AS n")
        .bind(Uuid::new_v4().to_string()).bind(&f.channel).execute(f.db.pool()).await.unwrap();
    let rows = service::recent_chatters(f.db.pool(), &f.channel, 60, UserFilter::default(), None)
        .await
        .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1000);
    f.message("oldest_eligible", "viewer", "hello", 20.0).await;
    f.message("oldest_eligible", "viewer", "динозавр", 400.0)
        .await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.assert_filtered(60, messages("динозавр", 600), &["oldest_eligible"]),
    )
    .await
    .expect("Candidate filtering must not repeatedly rescan the eligibility CTE");
}
#[test]
fn runtime_nested_windows_are_optional_and_all_builders_serialize_independently() {
    runtime::execute(Files::from([("main.rhai".into(), "fn on_event(ctx) { let f = UserFilter::create().min_messages(2).activity(ActivityFilter::create().min_messages(9).during(Duration::from_mins(30))).activity(ActivityFilter::create().min_messages(3).during(Duration::from_mins(5)).during(Duration::from_secs(1))).messages(MessageFilter::any().contains(\"old\").during(Duration::from_mins(10))).messages(MessageFilter::all().equals(\"ЁЖ\").case_sensitive(true).during(Duration::from_mins(5)).during(Duration::from_secs(1))); users.recent_chatters(Duration::from_mins(1), f); }".into())]),
        "on_event", json!({}), Arc::new(|_, args| {
            assert_eq!(args[1]["min_messages"], 2);
            assert_eq!(args[1]["activity"]["seconds"], 1);
            assert_eq!(args[1]["activity"]["min_messages"], 3);
            assert_eq!(args[1]["messages"]["seconds"], 1);
            assert_eq!(args[1]["messages"]["mode"], "all");
            assert_eq!(args[1]["messages"]["case_sensitive"], true);
            assert_eq!(args[1]["messages"]["clauses"].as_array().unwrap().len(), 1);
            Ok(json!([]))
        })).unwrap();
    runtime::execute(Files::from([("main.rhai".into(), "fn on_event(ctx) { let f = UserFilter::create().activity(ActivityFilter::create().min_messages(3)).messages(MessageFilter::any().contains(\"динозавр\")).reward_redemptions(RewardFilter::create().min_count(2)); users.recent_chatters(Duration::from_mins(1), f); }".into())]),
        "on_event", json!({}), Arc::new(|_, args| {
            for field in ["activity", "messages", "reward"] {
                assert!(args[1][field]["seconds"].is_null(), "{field} must not inherit candidate window");
            }
            Ok(json!([]))
        })).unwrap();
    for (expression, expected) in [
        (
            "UserFilter::create().activity(ActivityFilter::create().min_messages(-1).during(Duration::from_mins(30)))",
            "invalid_activity_filter",
        ),
        (
            "UserFilter::create().activity(ActivityFilter::create().during(Duration::from_secs(0)))",
            "Duration must be positive",
        ),
    ] {
        let error = runtime::execute(Files::from([("main.rhai".into(), format!("fn on_event(ctx) {{ users.recent_chatters(Duration::from_mins(1), {expression}); }}"))]),
            "on_event", json!({}), Arc::new(|_, _| panic!("invalid filter must not reach host"))).unwrap_err();
        assert!(error.contains(expected), "{expression}: {error}");
    }
    runtime::execute(Files::from([("main.rhai".into(), "fn on_event(ctx) { let f = UserFilter::create().activity(ActivityFilter::create().min_messages(3).min_characters(500).during(Duration::from_mins(30))).messages(MessageFilter::any().contains(\"динозавр\").during(Duration::from_mins(5))).reward_redemptions(RewardFilter::create().reward(\"secret_case\").min_count(2).during(Duration::from_days(7))); users.recent_chatters(Duration::from_mins(1), f); }".into())]),
        "on_event", json!({}), Arc::new(|method, args| {
            assert_eq!(method, "users.recent_chatters");
            assert_eq!(args[0], 60);
            assert_eq!(args[1]["activity"]["seconds"], 1800);
            assert_eq!(args[1]["activity"]["min_messages"], 3);
            assert_eq!(args[1]["activity"]["min_characters"], 500);
            assert_eq!(args[1]["messages"]["seconds"], 300);
            assert_eq!(args[1]["reward"]["seconds"], 7*86400);
            Ok(json!([]))
        })).unwrap();
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL"]
async fn postgres_omitted_windows_read_all_retained_history_without_expanding_candidates() {
    let f = Fixture::new().await;
    let r = reward(&f, "secret_case").await;
    for user in ["eligible", "not_recent", "wrong_case", "wrong_channel"] {
        f.message(
            user,
            "viewer",
            "hello",
            if user == "not_recent" { 120.0 } else { 10.0 },
        )
        .await;
    }
    // Even history older than Duration's maximum is eligible when no window is set.
    for user in ["eligible", "not_recent", "wrong_case"] {
        f.message(user, "viewer", "ДИНОЗАВР", 400.0 * 86400.0).await;
        f.message(user, "viewer", &"я".repeat(500), 401.0 * 86400.0)
            .await;
        for age in [400.0 * 86400.0, 401.0 * 86400.0] {
            redemption(&f, r, user, age, "COMPLETED").await;
        }
    }
    let mut filter = activity(3, 500, 60);
    filter.activity.as_mut().unwrap().seconds = None;
    filter.messages = messages("динозавр", 60).messages;
    filter.messages.as_mut().unwrap().seconds = None;
    filter.reward = Some(reward_filter(60));
    filter.reward.as_mut().unwrap().seconds = None;
    let rows = f
        .assert_filtered(60, filter.clone(), &["eligible", "wrong_case"])
        .await;
    assert_eq!(rows[0]["messages"], 1);
    filter.messages.as_mut().unwrap().case_sensitive = true;
    f.assert_filtered(60, filter.clone(), &[]).await;
    filter.messages.as_mut().unwrap().case_sensitive = false;
    filter.messages.as_mut().unwrap().seconds = Some(300);
    f.assert_filtered(60, filter, &[]).await;
}
#[tokio::test]
async fn service_revalidates_nonpositive_and_excessive_nested_windows_without_querying() {
    for seconds in [None, Some(1), Some(31_536_000)] {
        let mut a = activity(0, 0, 1).activity.unwrap();
        a.seconds = seconds;
        assert!(a.validate().is_ok());
        let mut m = any(MessageOperation::Equals, "ЁЖ");
        m.seconds = seconds;
        assert!(m.validate().is_ok());
    }
    let mut r = reward_filter(59);
    assert_eq!(r.validate().unwrap_err(), "invalid_reward_filter");
    r.seconds = None;
    assert!(r.validate().is_ok());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    for seconds in [Some(0), Some(-1), Some(31_536_001)] {
        let mut a = activity(0, 0, 60);
        a.activity.as_mut().unwrap().seconds = seconds;
        let mut m = messages("динозавр", 60);
        m.messages.as_mut().unwrap().seconds = seconds;
        let mut r = UserFilter {
            reward: Some(reward_filter(60)),
            ..UserFilter::default()
        };
        r.reward.as_mut().unwrap().seconds = seconds;
        for (filter, family) in [(a, "activity"), (m, "message"), (r, "reward")] {
            let error = service::recent_chatters(&pool, "channel", 60, filter, None)
                .await
                .unwrap_err();
            let expected = format!("invalid_{family}_filter");
            assert_eq!(error, expected);
        }
    }
}
