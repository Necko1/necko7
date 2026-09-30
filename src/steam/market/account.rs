use crate::steam::market::MarketClient;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct MarketGetMoney {
    #[serde(default)]
    pub money: Option<f64>,
    #[serde(default)]
    pub money_settlement: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

impl MarketClient {
    pub async fn get_money(&self, api_key: &str) -> Result<MarketGetMoney, reqwest::Error> {
        self.get_money_measured(api_key).await.0
    }

    pub async fn get_money_measured(
        &self,
        api_key: &str,
    ) -> (
        Result<MarketGetMoney, reqwest::Error>,
        super::MarketReadTimings,
    ) {
        let total = std::time::Instant::now();
        let mut timing = super::MarketReadTimings::default();
        let result = async {
            let phase = std::time::Instant::now();
            self.limiter.until_key_ready(&api_key.to_string()).await;
            timing.limiter_ms = super::elapsed_ms(phase);

            let phase = std::time::Instant::now();
            let res = self
                .http_client
                .get(format!("{}/get-money", self.api_base))
                .query(&[("key", api_key)])
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
            let parsed = serde_json::from_str::<MarketGetMoney>(&text);
            timing.parse_ms = super::elapsed_ms(phase);

            match parsed {
                Ok(data) => Ok(data),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        status = status.as_u16(),
                        "Failed to deserialize Market get-money response"
                    );
                    Ok(MarketGetMoney {
                        money: None,
                        money_settlement: None,
                        currency: None,
                        success: false,
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
