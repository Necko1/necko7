pub mod sell_buy;

pub struct MarketClient {
    http_client: reqwest::Client,
}

impl MarketClient {
    pub fn new() -> Self {
        Self { http_client: reqwest::Client::new() }
    }
}