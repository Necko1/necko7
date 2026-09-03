use crate::steam::market::MarketClient;
use crate::steam::trade_link::TradeLink;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::{serde_as, TimestampSeconds};
use uuid::Uuid;

fn deserialize_price_opt<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PriceValue {
        Int(i64),
        Float(f64),
        Str(String),
    }

    match Option::<PriceValue>::deserialize(deserializer)? {
        Some(PriceValue::Int(i)) => Ok(Some(i)),
        Some(PriceValue::Float(f)) => Ok(Some(f as i64)),
        Some(PriceValue::Str(s)) => s.parse::<i64>().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
pub struct MarketBuyForResponse {
    pub success: bool,
    pub id: Option<String>,
    pub error: Option<String>,
    pub code: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_price_opt")]
    pub price: Option<i64>, // in minor
}

#[derive(Deserialize)]
pub struct RefundInfo {
    pub amount: f64,
    pub currency: String,
    pub refund_id: i64,
}

#[derive(Deserialize)]
pub struct Refund {
    pub seller: RefundInfo,
    pub market: RefundInfo,
}

#[serde_as]
#[derive(Deserialize)]
pub struct GetBuyInfoData {
    pub item_id: String,
    pub market_hash_name: String,
    pub classid: String,
    pub instance: String,
    pub time: String,
    #[serde_as(as = "Option<TimestampSeconds<String>>")]
    pub settlement: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<TimestampSeconds<String>>")]
    pub send_until: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<TimestampSeconds<String>>")]
    pub receive_until: Option<DateTime<Utc>>,
    pub stage: String,
    pub causer: Option<String>,
    pub paid: f64,
    pub refund: Option<Refund>,
    pub currency: String,
    #[serde(rename = "for")]
    pub r#for: String,
    pub bot_id: Option<String>,
    pub trade_id: Option<String>,
    pub asset_id: Option<String>,
}

#[derive(Deserialize)]
pub struct GetBuyInfoResponse {
    pub success: bool,
    pub data: Option<GetBuyInfoData>,

    pub error: Option<String>,
}

impl MarketClient {
    pub async fn buy_for(
        &self,
        api_key: &str,
        item_name: &str,
        max_price: i32,
        chance_to_transfer: i16,
        trade_link: TradeLink,
        custom_id: &Uuid,
    ) -> Result<MarketBuyForResponse, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("hash_name", item_name),
            ("price", &max_price.to_string()),
            ("chance_to_transfer", &chance_to_transfer.to_string()),
            ("partner", &trade_link.partner),
            ("token", &trade_link.token),
            ("custom_id", &custom_id.to_string()),
        ];

        self.limiter.until_key_ready(&api_key.to_string()).await;

        let res = self.http_client
            .get("https://market.csgo.com/api/v2/buy-for")
            .query(&params)
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;

        match serde_json::from_str::<MarketBuyForResponse>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    status = status.as_u16(),
                    raw_body = %text,
                    custom_id = %custom_id,
                    "Failed to deserialize Market buy-for response"
                );
                Ok(MarketBuyForResponse {
                    id: None,
                    price: None,
                    success: false,
                    error: Some(format!("HTTP {}: {}", status.as_u16(), text)),
                    code: Some(status.as_u16() as u32),
                })
            }
        }
    }

    pub async fn get_buy_info(
        &self,
        api_key: &str,
        custom_id: &Uuid,
    ) -> Result<GetBuyInfoResponse, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("custom_id", &custom_id.to_string()),
        ];

        self.limiter.until_key_ready(&api_key.to_string()).await;

        let res = self.http_client
            .get("https://market.csgo.com/api/v2/get-buy-info-by-custom-id")
            .query(&params)
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;

        match serde_json::from_str::<GetBuyInfoResponse>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    status = status.as_u16(),
                    raw_body = %text,
                    custom_id = %custom_id,
                    "Failed to deserialize Market get-buy-info response"
                );
                Ok(GetBuyInfoResponse {
                    data: None,
                    success: false,
                    error: Some(format!("HTTP {}: {}", status.as_u16(), text)),
                })
            }
        }
    }
}