//! Uses only a disposable TEST_DATABASE_URL, never the application's DATABASE_URL.
use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ed25519_dalek::{Signer, SigningKey};
use tower::ServiceExt;

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    cookie: Option<Uuid>,
    body: Vec<u8>,
    key: Option<&SigningKey>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("Content-Type", "application/json");
    if let Some(cookie) = cookie {
        req = req.header("Cookie", format!("session_id={cookie}"));
    }
    if let Some(key) = key {
        req = req.header(
            "X-Necko7-Signature",
            STANDARD.encode(key.sign(&body).to_bytes()),
        );
    }
    router
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap()
}
async fn json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and dummy AppState environment"]
async fn database_api_security() {
    let db = crate::db::Db::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::from_env(db.clone()).await.unwrap();
    let channel = format!("cs2-test-{}", Uuid::new_v4());
    let editor = format!("cs2-editor-{}", Uuid::new_v4());
    for id in [&channel, &editor] {
        sqlx::query("INSERT INTO users(twitch_id,login) VALUES($1,$1)")
            .bind(id)
            .execute(db.pool())
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO broadcasters(channel_id,channel_login,user_access_token,refresh_token,created_at,updated_at) VALUES($1,$1,'test','test',now(),now())").bind(&channel).execute(db.pool()).await.unwrap();
    let owner_cookie = Uuid::new_v4();
    let editor_cookie = Uuid::new_v4();
    for (id, role, cookie) in [
        (&channel, "OWNER", owner_cookie),
        (&editor, "EDITOR", editor_cookie),
    ] {
        sqlx::query("INSERT INTO channel_permissions(channel_id,user_id,role,granted_by) VALUES($1,$2,$3,$1)").bind(&channel).bind(id).bind(role).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO sessions(session_id,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')").bind(cookie).bind(id).execute(db.pool()).await.unwrap();
    }
    let router = router().with_state(state.clone());
    let base = format!("/broadcasters/{channel}/cs2");
    let code_path = format!("{base}/pairing");
    assert_eq!(
        request(&router, "POST", &code_path, None, vec![], None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            &code_path,
            Some(editor_cookie),
            vec![],
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "DELETE", &base, Some(editor_cookie), vec![], None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let initial = request(
        &router,
        "POST",
        &code_path,
        Some(owner_cookie),
        vec![],
        None,
    )
    .await;
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = json(initial).await;
    assert_eq!(
        request(
            &router,
            "POST",
            &code_path,
            Some(owner_cookie),
            vec![],
            None
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query(
        "UPDATE cs2_pairing_codes SET created_at=now()-interval '1 minute' WHERE channel_id=$1",
    )
    .bind(&channel)
    .execute(db.pool())
    .await
    .unwrap();
    let replacement = json(
        request(
            &router,
            "POST",
            &code_path,
            Some(owner_cookie),
            vec![],
            None,
        )
        .await,
    )
    .await;
    let key = SigningKey::from_bytes(&[42; 32]);
    let pair_body = |code: &serde_json::Value| {
        serde_json::to_vec(&serde_json::json!({"code":code,"public_key":STANDARD.encode(key.verifying_key().as_bytes()),"app_version":"test"})).unwrap()
    };
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            None,
            pair_body(&initial["code"]),
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query(
        "UPDATE cs2_pairing_codes SET expires_at=now()-interval '1 second' WHERE channel_id=$1",
    )
    .bind(&channel)
    .execute(db.pool())
    .await
    .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            None,
            pair_body(&replacement["code"]),
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query(
        "UPDATE cs2_pairing_codes SET expires_at=now()+interval '5 minutes' WHERE channel_id=$1",
    )
    .bind(&channel)
    .execute(db.pool())
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            None,
            pair_body(&replacement["code"]),
            None
        ),
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            None,
            pair_body(&replacement["code"]),
            None
        )
    );
    assert_eq!(
        usize::from(a.status() == StatusCode::OK) + usize::from(b.status() == StatusCode::OK),
        1
    );
    let paired = json(if a.status() == StatusCode::OK { a } else { b }).await;
    assert_eq!(paired["channel"]["twitch_id"], channel);
    assert_eq!(
        request(
            &router,
            "POST",
            &code_path,
            Some(owner_cookie),
            vec![],
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let session = Uuid::new_v4();
    let body = |seq, session, sent_at| {
        serde_json::to_vec(&serde_json::json!({"device_id":paired["device_id"],"session_id":session,"seq":seq,"sent_at":sent_at,"gsi":{"provider":{"appid":730}}})).unwrap()
    };
    let original = body(1, session, Utc::now());
    assert_eq!(
        request(&router, "POST", "/cs2/gsi", None, original.clone(), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // Verify that whitespace is part of the signature, including through Axum's raw Bytes extractor.
    let sig = STANDARD.encode(key.sign(&original).to_bytes());
    let mut tampered = original.clone();
    tampered.push(b' ');
    let req = Request::builder()
        .method("POST")
        .uri("/cs2/gsi")
        .header("Content-Type", "application/json")
        .header("X-Necko7-Signature", sig)
        .body(Body::from(tampered))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let (a, b) = tokio::join!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            original.clone(),
            Some(&key)
        ),
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            original.clone(),
            Some(&key)
        )
    );
    assert_eq!(
        usize::from(a.status() == StatusCode::NO_CONTENT)
            + usize::from(b.status() == StatusCode::NO_CONTENT),
        1
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            body(3, session, Utc::now()),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            body(2, session, Utc::now()),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            body(4, session, Utc::now() - chrono::Duration::minutes(6)),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            body(1, Uuid::new_v4(), Utc::now()),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            original.clone(),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/unpair",
            None,
            body(5, session, Utc::now()),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let revoke_body=serde_json::to_vec(&serde_json::json!({"device_id":paired["device_id"],"session_id":session,"seq":6,"sent_at":Utc::now(),"action":"unpair"})).unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/devices/unpair",
            None,
            revoke_body,
            Some(&key)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/cs2/gsi",
            None,
            body(7, session, Utc::now()),
            Some(&key)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(json(request(&router,"GET",&base,Some(owner_cookie),vec![],None).await).await["device"].is_null());
    assert_eq!(
        request(&router, "DELETE", &base, Some(owner_cookie), vec![], None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    state.shutdown_token.cancel();
    // Re-pair, then verify owner revocation rejects the new device immediately.
    let replacement = json(
        request(
            &router,
            "POST",
            &code_path,
            Some(owner_cookie),
            vec![],
            None,
        )
        .await,
    )
    .await;
    let paired = json(
        request(
            &router,
            "POST",
            "/cs2/devices/pair",
            None,
            pair_body(&replacement["code"]),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(
        request(&router, "DELETE", &base, Some(owner_cookie), vec![], None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let body = serde_json::to_vec(&serde_json::json!({"device_id":paired["device_id"],"session_id":Uuid::new_v4(),"seq":1,"sent_at":Utc::now(),"gsi":{}})).unwrap();
    assert_eq!(
        request(&router, "POST", "/cs2/gsi", None, body, Some(&key))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("DELETE FROM users WHERE twitch_id=$1 OR twitch_id=$2")
        .bind(channel)
        .bind(editor)
        .execute(db.pool())
        .await
        .unwrap();
}
