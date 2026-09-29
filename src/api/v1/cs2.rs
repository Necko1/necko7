//! Device trust boundary. Authenticated snapshots enter the backend semantic pipeline.
#![deny(clippy::all)]
#[cfg(test)]
#[path = "cs2_tests.rs"]
mod integration_tests;
use crate::{
    api::{
        error::ApiError,
        extractor::{authorized_channel::AuthorizedChannel, json::JsonArg},
    },
    state::AppState,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::HeaderMap,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use rand::{Rng, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};
use uuid::Uuid;

const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
static PAIR_LIMIT: LazyLock<DefaultDirectRateLimiter> =
    LazyLock::new(|| RateLimiter::direct(Quota::per_minute(NonZeroU32::new(60).unwrap())));
static GSI_LIMIT: LazyLock<DefaultDirectRateLimiter> =
    LazyLock::new(|| RateLimiter::direct(Quota::per_second(NonZeroU32::new(1000).unwrap())));
fn bad(message: &str) -> ApiError {
    ApiError::BadRequest {
        message: message.into(),
        param: "cs2".into(),
    }
}
fn denied() -> ApiError {
    ApiError::Unauthorized {
        message: "Unknown, revoked device or invalid signature".into(),
    }
}
fn limited() -> ApiError {
    ApiError::TooManyRequests {
        message: "Please wait before trying again".into(),
    }
}
fn db(e: sqlx::Error) -> ApiError {
    crate::db::error::DbError::from(e).into()
}
pub fn normalize(code: &str) -> Result<String, ApiError> {
    let code = code.trim().to_ascii_uppercase();
    let raw = if code.len() == 9 && code.as_bytes()[4] == b'-' {
        code.replace('-', "")
    } else {
        code
    };
    if raw.len() != 8 || !raw.bytes().all(|c| ALPHABET.contains(&c)) {
        return Err(bad("Invalid pairing code"));
    }
    Ok(raw)
}
fn hash(code: &str) -> Vec<u8> {
    Sha256::digest(code.as_bytes()).to_vec()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/broadcasters/{channel_id}/cs2", get(status).delete(revoke))
        .route("/broadcasters/{channel_id}/cs2/pairing", post(create_code))
        .route("/cs2/devices/pair", post(pair))
        .route("/cs2/gsi", post(ingest))
        .route("/cs2/devices/unpair", post(unpair))
        .route("/cs2/devices/heartbeat", post(heartbeat))
        .layer(DefaultBodyLimit::max(256 * 1024))
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct Status {
    device: Option<crate::db::cs2::Device>,
}
#[utoipa::path(get, path="/api/v1/broadcasters/{channel_id}/cs2", responses((status=200, body=Status)), tag="CS2")]
pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthorizedChannel,
) -> Result<Json<Status>, ApiError> {
    auth.require_editor()?;
    Ok(Json(Status {
        device: state.db.cs2_device(&auth.channel_id).await?,
    }))
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct PairingCode {
    code: String,
    expires_at: DateTime<Utc>,
}
#[utoipa::path(post, path="/api/v1/broadcasters/{channel_id}/cs2/pairing", responses((status=200, body=PairingCode)), tag="CS2")]
pub async fn create_code(
    State(state): State<Arc<AppState>>,
    auth: AuthorizedChannel,
) -> Result<Json<PairingCode>, ApiError> {
    auth.require_owner()?;
    let mut tx = state.db.pool().begin().await.map_err(db)?;
    sqlx::query("SELECT channel_id FROM broadcasters WHERE channel_id=$1 FOR UPDATE")
        .bind(&auth.channel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM cs2_devices WHERE channel_id=$1 AND revoked_at IS NULL)",
    )
    .bind(&auth.channel_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?
    {
        return Err(bad("Channel already paired; unpair first"));
    }
    let recent: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cs2_pairing_codes WHERE channel_id=$1 AND created_at > now()-interval '10 seconds')").bind(&auth.channel_id).fetch_one(&mut *tx).await.map_err(db)?;
    if recent {
        return Err(limited());
    }
    let raw: String = (0..8)
        .map(|_| ALPHABET[OsRng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    let expires_at = Utc::now() + chrono::Duration::minutes(5);
    sqlx::query("INSERT INTO cs2_pairing_codes(channel_id,code_hash,expires_at) VALUES($1,$2,$3) ON CONFLICT(channel_id) DO UPDATE SET code_hash=$2, expires_at=$3, created_at=now()")
        .bind(&auth.channel_id).bind(hash(&raw)).bind(expires_at).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(PairingCode {
        code: format!("{}-{}", &raw[..4], &raw[4..]),
        expires_at,
    }))
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    code: String,
    public_key: String,
    app_version: String,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct Channel {
    twitch_id: String,
    username: String,
    display_name: String,
    avatar_url: Option<String>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct PairResponse {
    device_id: Uuid,
    channel: Channel,
}
#[utoipa::path(post, path="/api/v1/cs2/devices/pair", request_body=PairRequest, responses((status=200, body=PairResponse)), security(), tag="CS2")]
pub async fn pair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    JsonArg(req): JsonArg<PairRequest>,
) -> Result<Json<PairResponse>, ApiError> {
    PAIR_LIMIT.check().map_err(|_| limited())?;
    let code = normalize(&req.code)?;
    let key = STANDARD
        .decode(&req.public_key)
        .map_err(|_| bad("Invalid public key"))?;
    let key_bytes: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| bad("Invalid public key length"))?;
    let public = VerifyingKey::from_bytes(&key_bytes).map_err(|_| bad("Invalid Ed25519 key"))?;
    if public.is_weak() || req.app_version.is_empty() || req.app_version.len() > 64 {
        return Err(bad("Invalid device metadata"));
    }
    let mut tx = state.db.pool().begin().await.map_err(db)?;
    // Lock broadcaster before code, in the same order as generation.
    let channel: Option<String> =
        sqlx::query_scalar("SELECT channel_id FROM cs2_pairing_codes WHERE code_hash=$1")
            .bind(hash(&code))
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    let channel = channel.ok_or_else(|| bad("Invalid or expired pairing code"))?;
    require_owner_if_session(&state, &channel, &headers).await?;
    sqlx::query("SELECT channel_id FROM broadcasters WHERE channel_id=$1 FOR UPDATE")
        .bind(&channel)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let consumed = sqlx::query(
        "DELETE FROM cs2_pairing_codes WHERE channel_id=$1 AND code_hash=$2 AND expires_at > clock_timestamp()",
    )
    .bind(&channel)
    .bind(hash(&code))
    .execute(&mut *tx)
    .await
    .map_err(db)?
    .rows_affected();
    if consumed != 1 {
        return Err(bad("Invalid or expired pairing code"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO cs2_devices(id,channel_id,public_key,app_version) VALUES($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(&channel)
    .bind(key)
    .bind(req.app_version)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let (username, avatar_url): (String, Option<String>) = sqlx::query_as("SELECT b.channel_login,u.avatar_url FROM broadcasters b LEFT JOIN users u ON u.twitch_id=b.channel_id WHERE b.channel_id=$1").bind(&channel).fetch_one(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    // The dashboard already hydrates this existing Twitch profile cache.
    // Pairing must not depend on Twitch availability after consuming the code.
    let profile = state
        .twitch_user_cache
        .read()
        .get(&channel)
        .map(|(_, user)| user.clone());
    Ok(Json(PairResponse {
        device_id: id,
        channel: Channel {
            twitch_id: channel,
            display_name: profile
                .as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_else(|| username.clone()),
            username,
            avatar_url: profile.map(|u| u.profile_image_url.clone()).or(avatar_url),
        },
    }))
}
// A cookie cannot substitute Editor access for the owner's one-shot device credential.
// Native desktop clients have no web session and prove the owner grant with the code/key.
async fn require_owner_if_session(state: &AppState, channel: &str, headers: &HeaderMap) -> Result<(), ApiError> {
    let cookie = headers.get(axum::http::header::COOKIE).and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(';').find_map(|part| part.trim().strip_prefix("session_id=")));
    let Some(cookie) = cookie else { return Ok(()); };
    let session = Uuid::parse_str(cookie).map_err(|_| denied())?;
    let user = state.db.get_valid_session(session).await?.ok_or_else(denied)?.user_id;
    let permission = state.db.get_permission(channel, &user).await?;
    if !permission.is_some_and(|p| p.role == crate::db::channel_permissions::ChannelRole::Owner) {
        return Err(ApiError::Forbidden { message: "Owner access required".into() });
    }
    Ok(())
}
#[utoipa::path(delete, path="/api/v1/broadcasters/{channel_id}/cs2", responses((status=204)), tag="CS2")]
pub async fn revoke(
    State(state): State<Arc<AppState>>,
    auth: AuthorizedChannel,
) -> Result<axum::http::StatusCode, ApiError> {
    auth.require_owner()?;
    let mut tx = state.db.pool().begin().await.map_err(db)?;
    sqlx::query("SELECT channel_id FROM broadcasters WHERE channel_id=$1 FOR UPDATE")
        .bind(&auth.channel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let device: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM cs2_devices WHERE channel_id=$1 AND revoked_at IS NULL")
            .bind(&auth.channel_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    // Same lock order as signed ingestion: process stripe before device row.
    let _serial = if let Some(id) = device {
        Some(state.cs2.serial(id).await)
    } else {
        None
    };
    sqlx::query("DELETE FROM cs2_pairing_codes WHERE channel_id=$1")
        .bind(&auth.channel_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    sqlx::query("UPDATE cs2_devices SET revoked_at=now(),updated_at=now() WHERE channel_id=$1 AND revoked_at IS NULL").bind(auth.channel_id).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    if let Some(id) = device {
        state.cs2.remove(id);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    device_id: Uuid,
    session_id: Uuid,
    seq: i64,
    sent_at: DateTime<Utc>,
    #[serde(default)]
    gsi: Option<serde_json::Value>,
    #[serde(default)]
    action: Option<String>,
}
fn verify(key: &[u8], signature: &str, body: &[u8]) -> Result<(), ApiError> {
    let key: [u8; 32] = key.try_into().map_err(|_| denied())?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| denied())?;
    let signature = STANDARD.decode(signature).map_err(|_| denied())?;
    let signature = Signature::from_slice(&signature).map_err(|_| denied())?;
    key.verify_strict(body, &signature).map_err(|_| denied())
}
fn fresh(env: &Envelope) -> bool {
    env.seq > 0 && (Utc::now() - env.sent_at).num_milliseconds().abs() <= 300_000
}
async fn signed(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Bytes,
    action: Option<&str>,
) -> Result<axum::http::StatusCode, ApiError> {
    GSI_LIMIT.check().map_err(|_| limited())?;
    if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| !v.starts_with("application/json"))
    {
        return Err(bad("Expected JSON"));
    }
    let env: Envelope = serde_json::from_slice(&body).map_err(|error| {
        tracing::warn!(
            stage = "decode",
            reason = "invalid_envelope",
            bytes = body.len(),
            line = error.line(),
            column = error.column(),
            "CS2 request JSON decode failed"
        );
        bad("Invalid envelope")
    })?;
    let _serial = state.cs2.serial(env.device_id).await;
    let mut tx = state.db.pool().begin().await.map_err(db)?;
    let device: Option<(Vec<u8>,String)> = sqlx::query_as("SELECT public_key,channel_id FROM cs2_devices WHERE id=$1 AND revoked_at IS NULL FOR UPDATE")
        .bind(env.device_id).fetch_optional(&mut *tx).await.map_err(db)?;
    let (key, channel) = device.ok_or_else(denied)?;
    if action == Some("unpair") {
        require_owner_if_session(&state, &channel, &headers).await?;
    }
    verify(
        &key,
        headers
            .get("x-necko7-signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(denied)?,
        &body,
    )?;
    if !fresh(&env) {
        return Err(bad("Stale timestamp or invalid sequence"));
    }
    if let Some(action) = action {
        if env.action.as_deref() != Some(action) || env.gsi.is_some() {
            return Err(bad("Expected signed device action without GSI"));
        }
    } else if env.action.is_some() || !env.gsi.as_ref().is_some_and(|g| g.is_object()) {
        tracing::warn!(channel_id=%channel, device_id=%env.device_id, session_id=%env.session_id, seq=env.seq, stage="normalize", reason="expected_gsi_object", "Signed CS2 body has no usable GSI object");
        return Err(bad("Expected GSI object"));
    }
    // Global expiry cleanup bounds storage even for devices that no longer send.
    sqlx::query("DELETE FROM cs2_sessions WHERE expires_at < clock_timestamp()")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let previous: Option<i64> = sqlx::query_scalar(
        "SELECT highest_seq FROM cs2_sessions WHERE device_id=$1 AND session_id=$2",
    )
    .bind(env.device_id)
    .bind(env.session_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    if previous.is_some_and(|seq| env.seq <= seq) {
        return Err(bad("Replayed sequence"));
    }
    if previous.is_none() {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cs2_sessions WHERE device_id=$1")
            .bind(env.device_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        if count >= 32 {
            return Err(limited());
        }
    }
    sqlx::query("INSERT INTO cs2_sessions(device_id,session_id,highest_seq,expires_at) VALUES($1,$2,$3,clock_timestamp()+interval '11 minutes') ON CONFLICT(device_id,session_id) DO UPDATE SET highest_seq=$3,expires_at=clock_timestamp()+interval '11 minutes'")
        .bind(env.device_id).bind(env.session_id).bind(env.seq).execute(&mut *tx).await.map_err(db)?;
    let revoking = action == Some("unpair");
    sqlx::query("UPDATE cs2_devices SET last_seen_at=CASE WHEN $3 THEN clock_timestamp() ELSE last_seen_at END,last_heartbeat_at=CASE WHEN $4 THEN clock_timestamp() ELSE last_heartbeat_at END,updated_at=clock_timestamp(),revoked_at=CASE WHEN $2 THEN clock_timestamp() ELSE revoked_at END WHERE id=$1")
        .bind(env.device_id).bind(revoking).bind(action.is_none()).bind(action == Some("heartbeat")).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    tracing::debug!(device_id=%env.device_id, channel_id=%channel, session_id=%env.session_id, seq=env.seq, bytes=body.len(), revoking, "Accepted CS2 device message");
    if revoking {
        state.cs2.remove(env.device_id);
    }
    if let Some(gsi) = env.gsi {
        let gsi = crate::cs2::sanitize(gsi);
        let source = crate::cs2::model::Source {
            device_id: env.device_id,
            channel_id: channel,
            session_id: env.session_id,
            source_seq: env.seq,
            timestamp: env.sent_at,
        };
        let transition = state.cs2.process(source.clone(), &gsi);
        crate::cs2::log_transition(&source, &gsi, &transition);
        if let Err(error) =
            crate::scripting::matches::persist(state.db.pool(), &source, &transition).await
        {
            tracing::error!(%error, channel_id=%source.channel_id, device_id=%source.device_id, session_id=%source.session_id, seq=source.source_seq, stage="persist", "Could not persist scripting context; GSI transport remains available");
        }
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
#[utoipa::path(post, path="/api/v1/cs2/gsi", request_body=Envelope, responses((status=204)), security(), tag="CS2")]
pub async fn ingest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::http::StatusCode, ApiError> {
    signed(state, headers, body, None).await
}
#[utoipa::path(post, path="/api/v1/cs2/devices/unpair", request_body=Envelope, responses((status=204)), security(), tag="CS2")]
pub async fn unpair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::http::StatusCode, ApiError> {
    signed(state, headers, body, Some("unpair")).await
}

#[utoipa::path(post, path="/api/v1/cs2/devices/heartbeat", request_body=Envelope, responses((status=204), (status=401)), security(), tag="CS2")]
pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::http::StatusCode, ApiError> {
    signed(state, headers, body, Some("heartbeat")).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn debug_payload_never_contains_auth() {
        let cleaned = crate::cs2::sanitize(
            serde_json::json!({"auth":{"token":"secret"},"player":{"name":"Test"},"previously":{"auth":{"token":"old"}}}),
        );
        assert_eq!(cleaned["player"]["name"], "Test");
        assert!(!cleaned.to_string().contains("secret"));
        assert!(!cleaned.to_string().contains("old"));
        assert!(!cleaned.to_string().contains("auth"));
    }
    #[test]
    fn codes() {
        assert_eq!(normalize(" abcd-2345 ").unwrap(), "ABCD2345");
        for code in ["ABCD0123", "AB-CD2345", "ABCD23456", "ÄBCD2345"] {
            assert!(normalize(code).is_err());
        }
    }
    #[test]
    fn signature_raw_bytes_and_tampering() {
        let key = SigningKey::from_bytes(&[42; 32]);
        let body = br#"{ "seq": 1 }"#;
        let sig = STANDARD.encode(key.sign(body).to_bytes());
        assert!(verify(key.verifying_key().as_bytes(), &sig, body).is_ok());
        assert!(verify(key.verifying_key().as_bytes(), &sig, br#"{"seq":1}"#).is_err());
        assert!(verify(key.verifying_key().as_bytes(), &sig, br#"{ "seq": 2 }"#).is_err());
    }
    #[test]
    fn timestamps() {
        let mut env = Envelope {
            device_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            seq: 1,
            sent_at: Utc::now(),
            gsi: None,
            action: None,
        };
        assert!(fresh(&env));
        env.sent_at -= chrono::Duration::minutes(6);
        assert!(!fresh(&env));
        env.sent_at = Utc::now() + chrono::Duration::minutes(6);
        assert!(!fresh(&env));
        env.sent_at = Utc::now();
        env.seq = 0;
        assert!(!fresh(&env));
    }
}
