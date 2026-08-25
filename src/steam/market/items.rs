use serde::{Deserialize};
use crate::steam::market::MarketClient;

#[derive(Debug, Deserialize)]
pub struct MarketSearchItemList {
    pub success: bool,
    pub currency: String,
    pub data: Vec<MarketItemShort>,

    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MarketItemShort {
    pub market_hash_name: String,
    pub price: i64,
    pub class: i64,
    pub instance: i64,
    pub count: i64,
}

impl MarketClient {
    pub async fn search_item(
        &self,
        api_key: &str,
        item_name: &str,
    ) -> Result<MarketSearchItemList, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("hash_name", item_name),
        ];

        self.limiter.until_key_ready(&api_key.to_string()).await;

        self.http_client
            .get("https://market.csgo.com/api/v2/search-item-by-hash-name")
            .query(&params)
            .send()
            .await?
            .json::<MarketSearchItemList>()
            .await
    }
}