use serde::{Deserialize, Serialize};
use crate::steam::market::MarketClient;

#[derive(Debug, Deserialize)]
pub struct MarketGetMoney {
    pub money: f64,
    pub money_settlement: f64,
    pub currency: String,
    pub success: bool,
    pub error: Option<String>,
}

impl MarketClient {
    pub async fn get_money(
        &self,
        api_key: &str,
    ) -> Result<MarketGetMoney, reqwest::Error> {
        self.limiter.until_key_ready(&api_key.to_string()).await;

        self.http_client
            .get("https://market.csgo.com/api/v2/search-item-by-hash-name")
            .query(&[("key", api_key), ])
            .send()
            .await?
            .json::<MarketGetMoney>()
            .await
    }
}