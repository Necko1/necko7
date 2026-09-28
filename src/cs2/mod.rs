//! Backend-only semantic boundary, deliberately independent of HTTP and SQL.
#![deny(clippy::all)]
mod diagnostics;
pub mod model;
pub use diagnostics::{log_diagnostic, payload_issues, transition_issues};
mod normalize;
mod parse;

use model::{Source, Transition};
pub use normalize::Normalizer;
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use uuid::Uuid;

const CAPACITY: usize = 2048;
const TTL: Duration = Duration::from_secs(10 * 60);
struct Entry {
    normalizer: Normalizer,
    last_seen: Instant,
}
pub struct Pipeline {
    // Fixed stripes bound even unauthenticated callers' lock memory. They also
    // preserve DB acceptance -> normalization order and serialize revocation.
    serial: [AsyncMutex<()>; 128],
    entries: Mutex<HashMap<Uuid, Entry>>,
}
impl Default for Pipeline {
    fn default() -> Self {
        Self {
            serial: std::array::from_fn(|_| AsyncMutex::new(())),
            entries: Mutex::new(HashMap::new()),
        }
    }
}
impl Pipeline {
    pub async fn serial(&self, device_id: Uuid) -> MutexGuard<'_, ()> {
        let stripe = device_id.as_bytes().iter().fold(0usize, |hash, b| {
            hash.wrapping_mul(31).wrapping_add(usize::from(*b))
        }) % self.serial.len();
        self.serial[stripe].lock().await
    }
    /// Call only after successful signature/replay verification AND transaction
    /// commit, while holding serial(device_id). No raw JSON is retained.
    pub fn process(&self, source: Source, payload: &Value) -> Transition {
        self.process_at(source, payload, Instant::now())
    }
    fn process_at(&self, source: Source, payload: &Value, now: Instant) -> Transition {
        let mut entries = self.entries.lock();
        if entries
            .get(&source.device_id)
            .is_some_and(|e| now.duration_since(e.last_seen) >= TTL)
        {
            entries.remove(&source.device_id);
        }
        if !entries.contains_key(&source.device_id)
            && entries.len() >= CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, e)| e.last_seen)
                .map(|(id, _)| *id)
        {
            entries.remove(&oldest);
        }
        let entry = entries.entry(source.device_id).or_insert_with(|| Entry {
            normalizer: Normalizer::default(),
            last_seen: now,
        });
        entry.last_seen = now;
        entry.normalizer.apply(source, payload)
    }
    pub fn remove(&self, device_id: Uuid) {
        self.entries.lock().remove(&device_id);
    }
    pub fn expire(&self) {
        self.expire_at(Instant::now());
    }
    fn expire_at(&self, now: Instant) {
        self.entries
            .lock()
            .retain(|_, e| now.duration_since(e.last_seen) < TTL);
    }
    #[cfg(test)]
    pub fn snapshot(&self, id: Uuid) -> Option<(Source, model::Cs2State)> {
        self.entries
            .lock()
            .get(&id)
            .and_then(|entry| entry.normalizer.snapshot())
    }
    #[cfg(test)]
    pub fn contains(&self, id: Uuid) -> bool {
        self.entries.lock().contains_key(&id)
    }
}

/// Correlation fields are identical on raw/state/event records, including an
/// empty event list. Event-only DEBUG works without the verbose flag.
pub fn log_transition(source: &Source, payload: &Value, transition: &Transition) {
    for field in payload_issues(payload) {
        log_diagnostic(source, transition, "normalize", field);
    }
    for reason in transition_issues(transition) {
        log_diagnostic(source, transition, "derive", reason);
    }
    let pipeline =
        std::env::var("CS2_LOG_GSI_PIPELINE").is_ok_and(|v| v.eq_ignore_ascii_case("true"));
    let raw = std::env::var("CS2_LOG_GSI_PAYLOADS").is_ok_and(|v| v.eq_ignore_ascii_case("true"));
    log_with_options(source, payload, transition, pipeline, raw);
}
fn log_with_options(
    source: &Source,
    payload: &Value,
    transition: &Transition,
    pipeline: bool,
    raw: bool,
) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let span = tracing::debug_span!("cs2_pipeline", device_id=%source.device_id, channel_id=%source.channel_id, session_id=%source.session_id, seq=source.source_seq);
    let _entered = span.enter();
    if pipeline || raw {
        let text = serde_json::to_string_pretty(&sanitize(payload.clone())).unwrap_or_default();
        let bounded: String = text.chars().take(16_384).collect();
        tracing::debug!(payload=%bounded, truncated=text.len()>bounded.len(), "RAW GSI");
    }
    if pipeline {
        tracing::debug!(previous=%serde_json::to_string_pretty(&transition.previous).unwrap_or_default(), current=%serde_json::to_string_pretty(&transition.current).unwrap_or_default(), resets=?transition.resets, "NORMALIZED STATE");
        tracing::debug!(events=%serde_json::to_string_pretty(&transition.events).unwrap_or_default(), "NORMALIZED EVENTS");
    }
    for event in &transition.events {
        tracing::debug!(details=%serde_json::to_string(&event.event).unwrap_or_default(), "CS2 event");
    }
}

/// Defense in depth after signature verification; never normalize/log auth,
/// even if a signed client accidentally forwarded it inside delta sections.
pub fn sanitize(mut value: Value) -> Value {
    match &mut value {
        Value::Object(map) => {
            map.remove("auth");
            for child in map.values_mut() {
                *child = sanitize(child.take());
            }
        }
        Value::Array(items) => {
            for child in items {
                *child = sanitize(child.take());
            }
        }
        _ => {}
    }
    value
}

#[cfg(test)]
mod tests;
