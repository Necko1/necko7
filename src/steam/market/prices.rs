use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use crate::db::rewards::FilterConfig;
use crate::steam::market::MarketClient;

fn deserialize_price<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PriceVal {
        Str(String),
        Num(f64),
    }

    match PriceVal::deserialize(deserializer)? {
        PriceVal::Str(s) => s.parse::<f64>().map_err(serde::de::Error::custom),
        PriceVal::Num(n) => Ok(n),
    }
}

fn deserialize_volume<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum VolumeVal {
        Str(String),
        Num(i64),
    }

    match Option::<VolumeVal>::deserialize(deserializer)? {
        Some(VolumeVal::Str(s)) => s.parse::<i64>().map_err(serde::de::Error::custom),
        Some(VolumeVal::Num(n)) => Ok(n),
        None => Ok(0),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MarketPriceItem {
    pub market_hash_name: String,
    #[serde(default, deserialize_with = "deserialize_volume")]
    pub volume: i64,
    #[serde(deserialize_with = "deserialize_price")]
    pub price: f64,
}

#[derive(Debug, Deserialize)]
pub struct MarketPricesResponse {
    pub success: bool,
    pub time: Option<i64>,
    pub currency: Option<String>,
    pub items: Option<Vec<MarketPriceItem>>,
    pub error: Option<String>,
}

impl MarketClient {
    pub async fn get_prices(&self, currency: &str) -> Result<MarketPricesResponse, reqwest::Error> {
        let currency_upper = currency.to_uppercase();
        let url = format!("https://market.csgo.com/api/v2/prices/{}.json", currency_upper);

        let res = self.http_client.get(&url).send().await?;
        let status = res.status();
        let text = res.text().await?;

        match serde_json::from_str::<MarketPricesResponse>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    status = status.as_u16(),
                    currency = %currency_upper,
                    "Failed to deserialize Market prices response"
                );
                Ok(MarketPricesResponse {
                    success: false,
                    time: None,
                    currency: Some(currency_upper),
                    items: None,
                    error: Some(format!("HTTP {}: {}", status.as_u16(), e)),
                })
            }
        }
    }
}

/// Filter market price items by given FilterConfig criteria.
pub fn filter_prices(items: &[MarketPriceItem], filter: &FilterConfig) -> Vec<MarketPriceItem> {
    let prefix_lower = filter.name_prefix.as_deref().map(|s| s.to_lowercase());
    let suffix_lower = filter.name_suffix.as_deref().map(|s| s.to_lowercase());
    let contains_lower = filter.name_contains.as_deref().map(|s| s.to_lowercase());

    items
        .iter()
        .filter(|item| {
            if item.price < filter.min_price || item.price > filter.max_price {
                return false;
            }

            if let Some(min_vol) = filter.min_volume {
                if item.volume < min_vol {
                    return false;
                }
            }

            let name_lower = item.market_hash_name.to_lowercase();

            if let Some(ref p) = prefix_lower {
                if !name_lower.starts_with(p) {
                    return false;
                }
            }

            if let Some(ref s) = suffix_lower {
                if !name_lower.ends_with(s) {
                    return false;
                }
            }

            if let Some(ref c) = contains_lower {
                if !name_lower.contains(c) {
                    return false;
                }
            }

            true
        })
        .cloned()
        .collect()
}

pub fn calculate_average(prices: &[f64]) -> Option<f64> {
    if prices.is_empty() {
        return None;
    }
    let sum: f64 = prices.iter().sum();
    Some(sum / prices.len() as f64)
}

pub fn calculate_weighted_average(items: &[(f64, f64)]) -> Option<f64> {
    if items.is_empty() {
        return None;
    }
    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;
    for &(price, weight) in items {
        if weight > 0.0 {
            weighted_sum += price * weight;
            total_weight += weight;
        }
    }
    if total_weight <= 0.0 {
        return None;
    }
    Some(weighted_sum / total_weight)
}

pub fn calculate_median(prices: &mut [f64]) -> Option<f64> {
    if prices.is_empty() {
        return None;
    }
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = prices.len();
    if len % 2 == 1 {
        Some(prices[len / 2])
    } else {
        Some((prices[len / 2 - 1] + prices[len / 2]) / 2.0)
    }
}

pub fn calculate_max(prices: &[f64]) -> Option<f64> {
    prices.iter().copied().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_market_prices_json_mixed_types() {
        let json_data = r#"{
            "success": true,
            "time": 1788587517,
            "currency": "RUB",
            "items": [
                {"market_hash_name": "AK-47 | Redline (Field-Tested)", "volume": "82", "price": "1200.00"},
                {"market_hash_name": "AWP | Asiimov (Field-Tested)", "volume": 60, "price": 2500.10},
                {"market_hash_name": "Glock-18 | Water Elemental (Field-Tested)", "price": "350.50"}
            ]
        }"#;

        let res: MarketPricesResponse = serde_json::from_str(json_data).unwrap();
        assert!(res.success);
        assert_eq!(res.currency.as_deref(), Some("RUB"));
        let items = res.items.unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].market_hash_name, "AK-47 | Redline (Field-Tested)");
        assert_eq!(items[0].volume, 82);
        assert!((items[0].price - 1200.00).abs() < 1e-4);
        assert_eq!(items[1].volume, 60);
        assert!((items[1].price - 2500.10).abs() < 1e-4);
        assert_eq!(items[2].volume, 0); // missing volume defaults to 0
        assert!((items[2].price - 350.50).abs() < 1e-4);
    }

    #[test]
    fn test_filter_prices_all_criteria() {
        let items = vec![
            MarketPriceItem { market_hash_name: "AK-47 | Redline (Field-Tested)".into(), volume: 100, price: 1500.0 },
            MarketPriceItem { market_hash_name: "AK-47 | Slate (Field-Tested)".into(), volume: 200, price: 500.0 },
            MarketPriceItem { market_hash_name: "StatTrak™ AK-47 | Slate (Field-Tested)".into(), volume: 50, price: 1200.0 },
            MarketPriceItem { market_hash_name: "AWP | Asiimov (Field-Tested)".into(), volume: 80, price: 3500.0 },
            MarketPriceItem { market_hash_name: "Glock-18 | Water Elemental (Factory New)".into(), volume: 30, price: 800.0 },
        ];

        // 1. Filter by price range only
        let filter1 = FilterConfig {
            min_price: 600.0,
            max_price: 2000.0,
            name_prefix: None,
            name_suffix: None,
            name_contains: None,
            min_volume: None,
        };
        let res1 = filter_prices(&items, &filter1);
        assert_eq!(res1.len(), 3); // AK-47 Redline, StatTrak AK-47 Slate, Glock-18

        // 2. Filter by prefix and min_volume
        let filter2 = FilterConfig {
            min_price: 100.0,
            max_price: 5000.0,
            name_prefix: Some("AK-47".into()),
            name_suffix: None,
            name_contains: None,
            min_volume: Some(150),
        };
        let res2 = filter_prices(&items, &filter2);
        assert_eq!(res2.len(), 1);
        assert_eq!(res2[0].market_hash_name, "AK-47 | Slate (Field-Tested)");

        // 3. Filter by suffix
        let filter3 = FilterConfig {
            min_price: 0.0,
            max_price: 10000.0,
            name_prefix: None,
            name_suffix: Some("(Factory New)".into()),
            name_contains: None,
            min_volume: None,
        };
        let res3 = filter_prices(&items, &filter3);
        assert_eq!(res3.len(), 1);
        assert_eq!(res3[0].market_hash_name, "Glock-18 | Water Elemental (Factory New)");

        // 4. Filter by contains
        let filter4 = FilterConfig {
            min_price: 0.0,
            max_price: 10000.0,
            name_prefix: None,
            name_suffix: None,
            name_contains: Some("Slate".into()),
            min_volume: None,
        };
        let res4 = filter_prices(&items, &filter4);
        assert_eq!(res4.len(), 2);
    }

    #[test]
    fn test_statistical_calculations() {
        let prices = vec![100.0, 200.0, 300.0, 400.0];

        // Average
        assert_eq!(calculate_average(&prices), Some(250.0));
        assert_eq!(calculate_average(&[]), None);

        // Max
        assert_eq!(calculate_max(&prices), Some(400.0));
        assert_eq!(calculate_max(&[]), None);

        // Median odd
        let mut odd_prices = vec![50.0, 300.0, 10.0];
        assert_eq!(calculate_median(&mut odd_prices), Some(50.0));

        // Median even
        let mut even_prices = vec![100.0, 400.0, 200.0, 300.0];
        assert_eq!(calculate_median(&mut even_prices), Some(250.0));

        // Weighted average
        let weighted = vec![(100.0, 1.0), (200.0, 3.0)]; // (100*1 + 200*3) / 4 = 700 / 4 = 175
        assert_eq!(calculate_weighted_average(&weighted), Some(175.0));
        assert_eq!(calculate_weighted_average(&[]), None);
    }
}
