use crate::steam::market::MarketClient;
use crate::steam::trade_link::TradeLink;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::{serde_as, PickFirst, TimestampSeconds};

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

fn deserialize_id_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum IdValue {
        Int(i64),
        Str(String),
        Bool(bool),
    }

    match Option::<IdValue>::deserialize(deserializer)? {
        Some(IdValue::Str(s)) => Ok(Some(s)),
        Some(IdValue::Int(i)) => Ok(Some(i.to_string())),
        Some(IdValue::Bool(_)) | None => Ok(None),
    }
}

fn deserialize_code_opt<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum CodeValue {
        Int(u32),
        Str(String),
        Bool(bool),
    }

    match Option::<CodeValue>::deserialize(deserializer)? {
        Some(CodeValue::Int(i)) => Ok(Some(i)),
        Some(CodeValue::Str(s)) => s.parse::<u32>().map(Some).map_err(serde::de::Error::custom),
        Some(CodeValue::Bool(_)) | None => Ok(None),
    }
}

fn deserialize_trade_id_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum TradeIdValue {
        Int(i64),
        Str(String),
        Bool(bool),
    }

    match Option::<TradeIdValue>::deserialize(deserializer)? {
        Some(TradeIdValue::Int(i)) if i != 0 => Ok(Some(i.to_string())),
        Some(TradeIdValue::Str(ref s)) if s != "0" && !s.trim().is_empty() => Ok(Some(s.clone())),
        _ => Ok(None),
    }
}

fn deserialize_refund_opt<'de, D>(deserializer: D) -> Result<Option<Refund>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum RefundValue {
        Refund(Refund),
        Bool(bool),
    }

    match Option::<RefundValue>::deserialize(deserializer)? {
        Some(RefundValue::Refund(r)) => Ok(Some(r)),
        Some(RefundValue::Bool(_)) | None => Ok(None),
    }
}

fn deserialize_data_opt<'de, D>(deserializer: D) -> Result<Option<GetBuyInfoData>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum DataValue {
        Data(GetBuyInfoData),
        Bool(bool),
    }

    match Option::<DataValue>::deserialize(deserializer)? {
        Some(DataValue::Data(d)) => Ok(Some(d)),
        Some(DataValue::Bool(_)) | None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
pub struct MarketBuyForResponse {
    pub success: bool,
    #[serde(default, deserialize_with = "deserialize_id_opt")]
    pub id: Option<String>,
    pub error: Option<String>,
    #[serde(default, deserialize_with = "deserialize_code_opt")]
    pub code: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_price_opt")]
    pub price: Option<i64>, // in minor
}

#[derive(Debug, Deserialize)]
pub struct RefundInfo {
    pub amount: f64,
    pub currency: String,
    pub refund_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct Refund {
    pub seller: RefundInfo,
    pub market: RefundInfo,
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct GetBuyInfoData {
    pub item_id: String,
    pub market_hash_name: String,
    pub classid: String,
    pub instance: String,
    pub time: String,
    #[serde_as(as = "Option<PickFirst<(TimestampSeconds<String>, TimestampSeconds<i64>)>>")]
    pub settlement: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<PickFirst<(TimestampSeconds<String>, TimestampSeconds<i64>)>>")]
    pub send_until: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<PickFirst<(TimestampSeconds<String>, TimestampSeconds<i64>)>>")]
    pub receive_until: Option<DateTime<Utc>>,
    pub stage: String,
    pub causer: Option<String>,
    pub paid: f64,
    #[serde(default, deserialize_with = "deserialize_refund_opt")]
    pub refund: Option<Refund>,
    pub currency: String,
    #[serde(default, rename = "for")]
    pub r#for: Option<String>,
    pub bot_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_trade_id_opt")]
    pub trade_id: Option<String>,
    #[serde(default, alias = "assetid")]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub cancellation_reason: Option<String>,
}

impl GetBuyInfoData {
    pub fn is_claimed(&self) -> bool {
        self.settlement.is_some_and(|s| s > DateTime::UNIX_EPOCH) || self.stage == "2"
    }

    pub fn is_failed(&self) -> bool {
        self.stage == "5" || self.causer.is_some()
    }

    pub fn has_active_trade(&self) -> bool {
        self.receive_until.is_some_and(|r| r > DateTime::UNIX_EPOCH) && self.trade_id.is_some()
    }
}

#[derive(Debug, Deserialize)]
pub struct GetBuyInfoResponse {
    pub success: bool,
    #[serde(default, deserialize_with = "deserialize_data_opt")]
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
        custom_id: &str,
    ) -> Result<MarketBuyForResponse, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("hash_name", item_name),
            ("price", &max_price.to_string()),
            ("chance_to_transfer", &chance_to_transfer.to_string()),
            ("partner", &trade_link.partner),
            ("token", &trade_link.token),
            ("custom_id", custom_id),
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
        custom_id: &str,
    ) -> Result<GetBuyInfoResponse, reqwest::Error> {
        let params = [
            ("key", api_key),
            ("custom_id", custom_id),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_market_buy_for_failure() {
        let json = r#"{"success":false,"id":false,"price":null,"error":"Неверная ссылка для обмена","code":12}"#;
        let res: MarketBuyForResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(!res.success);
        assert_eq!(res.id, None);
        assert_eq!(res.price, None);
        assert_eq!(res.code, Some(12));
        assert_eq!(res.error.as_deref(), Some("Неверная ссылка для обмена"));
    }

    #[test]
    fn test_deserialize_market_buy_for_success_string_id() {
        let json = r#"{"success":true,"id":"11392691554","price":50}"#;
        let res: MarketBuyForResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(res.success);
        assert_eq!(res.id.as_deref(), Some("11392691554"));
        assert_eq!(res.price, Some(50));
    }

    #[test]
    fn test_deserialize_market_buy_for_success_numeric_id() {
        let json = r#"{"success":true,"id":11392691554,"price":50}"#;
        let res: MarketBuyForResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(res.success);
        assert_eq!(res.id.as_deref(), Some("11392691554"));
        assert_eq!(res.price, Some(50));
    }

    #[test]
    fn test_deserialize_get_buy_info_real_log() {
        let json = r#"{"success":true,"data":{"item_id":"11392691554","market_hash_name":"R8 Revolver | Mauve Aside (Field-Tested)","classid":"7993065983","instance":"302028390","time":"1788524591","settlement":"0","send_until":"1788524891","receive_until":"0","stage":"1","causer":null,"cancellation_reason":null,"paid":0.5,"currency":"RUB","for":null,"trade_id":0,"bot_id":null,"assetid":"52333278297"}}"#;
        let res: GetBuyInfoResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(res.success);
        let data = res.data.expect("data should be Some");
        assert_eq!(data.item_id, "11392691554");
        assert_eq!(data.market_hash_name, "R8 Revolver | Mauve Aside (Field-Tested)");
        assert_eq!(data.paid, 0.5);
        assert_eq!(data.r#for, None);
        assert_eq!(data.trade_id, None); // trade_id 0 should be mapped to None
        assert_eq!(data.asset_id.as_deref(), Some("52333278297"));
        assert_eq!(data.cancellation_reason, None);
        assert_eq!(data.stage, "1");
    }

    #[test]
    fn test_deserialize_get_buy_info_active_trade() {
        let json = r#"{"success":true,"data":{"item_id":"11392691554","market_hash_name":"R8 Revolver | Mauve Aside (Field-Tested)","classid":"7993065983","instance":"302028390","time":"1788524591","settlement":"0","send_until":"1788524891","receive_until":"1788525000","stage":"2","causer":null,"cancellation_reason":null,"paid":0.5,"currency":"RUB","for":"test","trade_id":9876543210,"bot_id":"bot1","assetid":"52333278297"}}"#;
        let res: GetBuyInfoResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(res.success);
        let data = res.data.expect("data should be Some");
        assert_eq!(data.trade_id.as_deref(), Some("9876543210"));
        assert_eq!(data.r#for.as_deref(), Some("test"));
    }

    #[test]
    fn test_deserialize_get_buy_info_data_false() {
        let json = r#"{"success":false,"data":false,"error":"Order not found"}"#;
        let res: GetBuyInfoResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(!res.success);
        assert!(res.data.is_none());
        assert_eq!(res.error.as_deref(), Some("Order not found"));
    }

    #[test]
    fn test_deserialize_get_buy_info_user_claimed_edge_case() {
        let json = r#"{"success":true,"data":{"item_id":"11451620303","market_hash_name":"R8 Revolver | Mauve Aside (Field-Tested)","classid":"7993065983","instance":"8800025210","time":"1788535172","settlement":"1789142400","send_until":"1788535472","receive_until":"1788535256","stage":"1","causer":null,"cancellation_reason":null,"paid":0.54,"currency":"RUB","for":"1299088345","trade_id":0,"bot_id":null,"assetid":"53523211386"}}"#;
        let res: GetBuyInfoResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(res.success);
        let data = res.data.expect("data should be Some");
        assert_eq!(data.trade_id, None);
        assert!(data.is_claimed(), "Trade with settlement timestamp should be recognized as claimed");
        assert!(!data.is_failed());
    }

    #[test]
    fn test_get_buy_info_helpers_initial_order() {
        let json = r#"{"success":true,"data":{"item_id":"1","market_hash_name":"item","classid":"1","instance":"1","time":"100","settlement":"0","send_until":"200","receive_until":"0","stage":"1","causer":null,"cancellation_reason":null,"paid":0.5,"currency":"RUB","for":null,"trade_id":0,"bot_id":null,"assetid":"1"}}"#;
        let res: GetBuyInfoResponse = serde_json::from_str(json).unwrap();
        let data = res.data.unwrap();
        assert!(!data.is_claimed(), "settlement 0 must not be considered claimed");
        assert!(!data.is_failed());
        assert!(!data.has_active_trade());
    }
}