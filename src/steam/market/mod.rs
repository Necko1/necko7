use std::time::Duration;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

pub mod sell_buy;
pub mod items;
pub mod account;
pub mod prices;
pub mod errors;

/// Read-only request timing. Contains no URL, API key, recipient or response body.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarketReadTimings {
    pub limiter_ms: f64,
    pub http_ms: f64,
    pub body_ms: f64,
    pub parse_ms: f64,
    pub total_ms: f64,
}

pub(crate) fn elapsed_ms(start: std::time::Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub struct MarketClient {
    http_client: reqwest::Client,
    api_base: String,
    limiter: DefaultKeyedRateLimiter<String>,
}

impl MarketClient {
    pub fn new() -> Self {
        let quota = Quota::with_period(Duration::from_millis(250)).unwrap();

        Self {
            http_client: reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("Market HTTP client"),
            api_base: "https://market.csgo.com/api/v2".into(),
            limiter: RateLimiter::keyed(quota),
        }
    }
    #[cfg(test)]
    pub fn for_test(base: String) -> Self {
        let mut client = Self::new();
        client.api_base = base;
        client
    }
}

pub fn minor_to_major(amount: i64, currency: &str) -> f64 {
    let div = if currency.eq_ignore_ascii_case("usd")
        || currency.eq_ignore_ascii_case("eur")
    {
        1000.0
    } else { 100.0 };

    amount as f64 / div
}

pub fn major_to_minor(amount: f64, currency: &str) -> i64 {
    let mul = if currency.eq_ignore_ascii_case("usd")
        || currency.eq_ignore_ascii_case("eur")
    {
        1000.0
    } else { 100.0 };

    (amount * mul).round() as i64
}
