use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::steam::market::MarketClient;
use crate::steam::trade_link::TradeLink;
use serde_with::{serde_as, TimestampSeconds, DisplayFromStr};

#[derive(Debug, Deserialize)]
pub struct MarketBuyForResponse {
    pub success: bool,
    pub id: Option<String>,
    pub error: Option<String>,
    pub code: Option<u32>,
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
    pub causer: String,
    pub paid: f64,
    pub refund: Refund,
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
    pub data: GetBuyInfoData,

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

        self.http_client
            .get("https://market.csgo.com/api/v2/buy-for")
            .query(&params)
            .send()
            .await?
            .json::<MarketBuyForResponse>()
            .await
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

        self.http_client
            .get("https://market.csgo.com/api/v2/get-buy-info-by-custom-id")
            .query(&params)
            .send()
            .await?
            .json::<GetBuyInfoResponse>()
            .await
    }
}