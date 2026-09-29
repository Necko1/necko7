use crate::{db::Db, state::AppState};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    session: Option<Uuid>,
    value: Value,
    key: Option<&SigningKey>,
) -> (StatusCode, Value) {
    let body = if value.is_null() {
        vec![]
    } else {
        serde_json::to_vec(&value).unwrap()
    };
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("Content-Type", "application/json");
    if let Some(session) = session {
        req = req.header("Cookie", format!("session_id={session}"));
    }
    if let Some(key) = key {
        req = req.header(
            "X-Necko7-Signature",
            STANDARD.encode(key.sign(&body).to_bytes()),
        );
    }
    let response = router
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 512 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn editor_scripts_access_and_owner_only_device_authority_through_http() {
    let db = Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::from_env(db.clone()).await.unwrap();
    let channel = format!("productization-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
        .bind(&channel)
        .execute(db.pool())
        .await
        .unwrap();
    let mut sessions = Vec::new();
    for role in ["OWNER", "EDITOR", "VIEWER", "UNRELATED"] {
        let user = format!("{channel}-{role}");
        sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
            .bind(&user)
            .execute(db.pool())
            .await
            .unwrap();
        if role == "OWNER" {
            sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(db.pool()).await.unwrap();
        }
        if matches!(role, "OWNER" | "EDITOR") {
            sqlx::query("INSERT INTO channel_permissions(channel_id,user_id,role,granted_by) VALUES($1,$2,$3,$2)").bind(&channel).bind(&user).bind(role).execute(db.pool()).await.unwrap();
        }
        let session = Uuid::new_v4();
        sqlx::query("INSERT INTO sessions(session_id,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')").bind(session).bind(user).execute(db.pool()).await.unwrap();
        sessions.push(session);
    }
    let owner = Some(sessions[0]);
    let editor = Some(sessions[1]);
    let router = super::api::router()
        .merge(crate::api::v1::cs2::router())
        .with_state(state);
    let scripts = format!("/broadcasters/{channel}/scripts");
    let cs2 = format!("/broadcasters/{channel}/cs2");
    for session in [Some(sessions[2]), Some(sessions[3]), None] {
        for (method, path, body) in [
            ("GET", scripts.clone(), Value::Null),
            (
                "POST",
                scripts.clone(),
                json!({"action":"create","name":"denied"}),
            ),
            ("GET", cs2.clone(), Value::Null),
        ] {
            let status = request(&router, method, &path, session, body, None).await.0;
            assert_eq!(
                status,
                if session.is_some() {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
    }
    assert_eq!(
        request(&router, "GET", &cs2, editor, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    for session in [editor, Some(sessions[2]), Some(sessions[3])] {
        assert_eq!(
            request(
                &router,
                "POST",
                &format!("{cs2}/pairing"),
                session,
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&router, "DELETE", &cs2, session, Value::Null, None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let (status, created) = request(
        &router,
        "POST",
        &scripts,
        editor,
        json!({"action":"create","name":"Editor project"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project = created["id"].clone();
    let files = json!({"main.rhai":"fn on_event(ctx) { log.info(ctx.event.kind); } fn on_timer(ctx) { log.info(ctx.timer.key); }"});
    for body in [
        json!({"action":"save","project_id":project,"version":1,"files":files}),
        json!({"action":"validate","project_id":project}),
        json!({"action":"publish","project_id":project,"version":2}),
        json!({"action":"enable","project_id":project,"enabled":true}),
        json!({"action":"test","project_id":project,"entry":"on_event","context":{"event":{"kind":"player_kill"}}}),
        json!({"action":"storage_set","project_id":project,"key":"editor","value":1}),
        json!({"action":"revision","project_id":project,"revision":1}),
        json!({"action":"rollback","project_id":project,"revision":1}),
        json!({"action":"rename","project_id":project,"name":"Renamed by Editor"}),
    ] {
        let (status, response) =
            request(&router, "POST", &scripts, editor, body.clone(), None).await;
        assert_eq!(status, StatusCode::OK, "{body}: {response}");
    }
    let (status, overview) = request(&router, "GET", &scripts, editor, Value::Null, None).await;
    assert_eq!(status, StatusCode::OK);
    for key in [
        "projects",
        "matches",
        "jobs",
        "storage",
        "executions",
        "revisions",
        "snapshots",
    ] {
        assert!(overview[key].is_array(), "Missing accessible {key}");
    }
    assert_eq!(overview["storage"][0]["value"], 1);
    let empty_stats = json!({"main.rhai":"fn on_event(ctx) { let stats = chat.user_stats(\"not-observed\", Duration::from_mins(5)); if stats.messages != 0 || stats.characters != 0 || stats.first_activity != () || stats.last_activity != () || stats.redemptions.total != 0 { throw \"Unexpected empty activity\"; } debug(log, \"Known zero activity\"); }"});
    assert_eq!(
        request(
            &router,
            "POST",
            &scripts,
            editor,
            json!({"action":"save","project_id":project,"version":2,"files":empty_stats}),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, report) = request(&router,"POST",&scripts,editor,json!({"action":"test","project_id":project,"entry":"on_event","context":{"event":{"kind":"player_kill"}}}),None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(report["error"].is_null(), "{report}");
    assert_eq!(report["logs"][0]["level"], "debug");
    assert_eq!(report["logs"][0]["message"], "Known zero activity");
    let (status, code) = request(
        &router,
        "POST",
        &format!("{cs2}/pairing"),
        owner,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let key = SigningKey::from_bytes(&[72; 32]);
    let pair = json!({"code":code["code"],"public_key":STANDARD.encode(key.verifying_key().as_bytes()),"app_version":"qa"});
    // Even possession of a code cannot be exercised through an Editor web session.
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            editor,
            pair.clone(),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // No code is obtainable as Editor; unsigned/invalid-code requests cannot claim authority.
    let mut invalid = pair.clone();
    invalid["code"] = json!("AAAA-AAAA");
    assert_eq!(
        request(&router, "POST", "/cs2/devices/pair", editor, invalid, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Native desktop has no browser session: the owner's one-shot code is its authorization.
    let (status, device) = request(&router, "POST", "/cs2/devices/pair", None, pair, None).await;
    assert_eq!(status, StatusCode::OK);
    let signed = json!({"device_id":device["device_id"],"session_id":Uuid::new_v4(),"seq":1,"sent_at":chrono::Utc::now(),"action":"unpair"});
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/unpair",
            editor,
            signed.clone(),
            Some(&key)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/unpair",
            None,
            signed.clone(),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/unpair",
            None,
            signed,
            Some(&key)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&router, "DELETE", &cs2, owner, Value::Null, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &router,
            "POST",
            &scripts,
            editor,
            json!({"action":"delete","project_id":project,"confirmation":"DELETE"}),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
}
