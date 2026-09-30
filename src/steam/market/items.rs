use crate::steam::market::MarketClient;
use serde::Deserialize;

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
        self.search_item_measured(api_key, item_name).await.0
    }

    pub async fn search_item_measured(
        &self,
        api_key: &str,
        item_name: &str,
    ) -> (
        Result<MarketSearchItemList, reqwest::Error>,
        super::MarketReadTimings,
    ) {
        let total = std::time::Instant::now();
        let mut timing = super::MarketReadTimings::default();
        let result = async {
            let params = [("key", api_key), ("hash_name", item_name)];

            let phase = std::time::Instant::now();
            self.limiter.until_key_ready(&api_key.to_string()).await;
            timing.limiter_ms = super::elapsed_ms(phase);

            let phase = std::time::Instant::now();
            let res = self
                .http_client
                .get(format!("{}/search-item-by-hash-name", self.api_base))
                .query(&params)
                .send()
                .await;
            timing.http_ms = super::elapsed_ms(phase);
            let res = res?;

            let status = res.status();
            let phase = std::time::Instant::now();
            let text = res.text().await;
            timing.body_ms = super::elapsed_ms(phase);
            let text = text?;
            let phase = std::time::Instant::now();
            let parsed = serde_json::from_str::<MarketSearchItemList>(&text);
            timing.parse_ms = super::elapsed_ms(phase);

            match parsed {
                Ok(data) => Ok(data),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        status = status.as_u16(),
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
        .await;
        timing.total_ms = super::elapsed_ms(total);
        (result, timing)
    }
}
