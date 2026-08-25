use std::time::Duration;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

pub mod sell_buy;
pub mod items;
pub mod account;

pub struct MarketClient {
    http_client: reqwest::Client,
    limiter: DefaultKeyedRateLimiter<String>,
}

impl MarketClient {
    pub fn new() -> Self {
        let quota = Quota::with_period(Duration::from_millis(250)).unwrap();

        Self {
            http_client: reqwest::Client::new(),
            limiter: RateLimiter::keyed(quota),
        }
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