use serde::Deserialize;
use uuid::Uuid;
use crate::steam::market::MarketClient;
use crate::steam::trade_link::TradeLink;

#[derive(Debug, Deserialize)]
pub struct MarketBuyForResponse {
    pub success: bool,
    pub id: Option<String>,
    pub error: Option<String>,
    pub code: Option<u32>,
}

impl MarketClient {
    pub async fn buy_for(
        &self,
        api_key: &str,
        item_name: &str,
        max_price: i32,
        trade_link: TradeLink,
        custom_id: &Uuid,
    ) -> Result<MarketBuyForResponse, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("hash_name", item_name),
            ("price", &max_price.to_string()),
            ("chance_to_transfer", "85"), // fixme shouldn't be hardcoded
            ("partner", &trade_link.partner),
            ("token", &trade_link.token),
            ("custom_id", &custom_id.to_string()),
        ];

        self.http_client
            .get("https://market.csgo.com/api/v2/buy-for")
            .query(&params)
            .send()
            .await?
            .json::<MarketBuyForResponse>()
            .await
    }
}