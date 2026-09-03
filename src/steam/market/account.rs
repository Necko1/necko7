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
    pub async fn get_money(
        &self,
        api_key: &str,
    ) -> Result<MarketGetMoney, reqwest::Error> {
        self.limiter.until_key_ready(&api_key.to_string()).await;

        let res = self.http_client
            .get("https://market.csgo.com/api/v2/get-money")
            .query(&[("key", api_key)])
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;

        match serde_json::from_str::<MarketGetMoney>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    status = status.as_u16(),
                    raw_body = %text,
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
}