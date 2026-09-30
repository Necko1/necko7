//! Instrument only the read-only preview route, including its extractors.
//! All metric names are controlled constants and all values are durations.
use crate::steam::market::{MarketReadTimings, elapsed_ms};
use axum::{
    body::Body,
    http::{HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use parking_lot::Mutex;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

#[derive(Clone)]
pub struct PreviewTiming {
    started: Instant,
    values: Arc<Mutex<BTreeMap<String, f64>>>,
}

impl PreviewTiming {
    pub fn extracted(&self) {
        self.record("auth_extract", elapsed_ms(self.started));
    }
    pub fn record(&self, name: &str, value: f64) {
        self.values.lock().insert(name.into(), value);
    }
    pub fn market(&self, endpoint: &str, timing: MarketReadTimings) {
        for (phase, duration) in [
            ("limiter", timing.limiter_ms),
            ("http", timing.http_ms),
            ("body", timing.body_ms),
            ("parse", timing.parse_ms),
            ("total", timing.total_ms),
        ] {
            self.record(&format!("{endpoint}_{phase}"), duration);
        }
    }
}

pub async fn measure(mut request: Request<Body>, next: Next) -> Response {
    let timing = PreviewTiming {
        started: Instant::now(),
        values: Arc::default(),
    };
    let request_id = uuid::Uuid::new_v4();
    request.extensions_mut().insert(timing.clone());
    let mut response = next.run(request).await;
    let total = elapsed_ms(timing.started);
    let mut values = timing.values.lock();
    // This remainder includes validation, response construction and middleware;
    // it excludes the separately measured auth/DB and complete Market reads.
    let accounted: f64 = ["auth_extract", "settings_db", "money_total", "search_total"]
        .iter()
        .map(|name| values.get(*name).copied().unwrap_or(0.0))
        .sum();
    values.insert("local_other".into(), (total - accounted).max(0.0));
    values.insert("total".into(), total);
    tracing::info!(%request_id, status=response.status().as_u16(), duration_ms=total,
        breakdown=?*values, "Manual preview latency breakdown (milliseconds)");
    let header = values
        .iter()
        .map(|(key, value)| format!("{key};dur={value:.3}"))
        .collect::<Vec<_>>()
        .join(", ");
    response.headers_mut().insert(
        "Server-Timing",
        HeaderValue::from_str(&header).expect("controlled duration metrics"),
    );
    response.headers_mut().insert(
        "X-Preview-Request-Id",
        HeaderValue::from_str(&request_id.to_string()).unwrap(),
    );
    response
}
