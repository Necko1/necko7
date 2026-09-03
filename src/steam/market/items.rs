use serde::{Deserialize};
use crate::steam::market::MarketClient;

#[derive(Debug, Deserialize)]
pub struct MarketSearchItemList {
    pub success: bool,
    pub currency: Option<String>,
    pub data: Option<Vec<MarketItemShort>>,

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

        let res = self.http_client
            .get("https://market.csgo.com/api/v2/search-item-by-hash-name")
            .query(&params)
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;

        match serde_json::from_str::<MarketSearchItemList>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    status = status.as_u16(),
                    raw_body = %text,
                    item_name = %item_name,
                    "Failed to deserialize Market search-item response"
                );
                Ok(MarketSearchItemList {
                    success: false,
                    currency: None,
                    data: None,
                    error: Some(format!("HTTP {}: {}", status.as_u16(), text)),
                })
            }
        }
    }
}