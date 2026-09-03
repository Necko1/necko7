use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MSG_TRADE_LINK_INVALID: &str = "trade_link_invalid";
pub const MSG_ORDER_CREATED: &str = "order_created";
pub const MSG_ORDER_FAILED: &str = "order_failed";
pub const MSG_MARKET_ERROR: &str = "market_error";
pub const MSG_TRADE_CREATED: &str = "trade_created";
pub const MSG_TRADE_ACCEPTED: &str = "trade_accepted";
pub const MSG_TRADE_FAILED_BUYER_REFUND: &str = "trade_failed_buyer_refund";
pub const MSG_TRADE_FAILED_BUYER_PENALTY: &str = "trade_failed_buyer_penalty";
pub const MSG_TRADE_FAILED_SELLER_REFUND: &str = "trade_failed_seller_refund";
pub const MSG_TRADE_TIMEOUT: &str = "trade_timeout";

pub const ALL_MESSAGE_KEYS: [&str; 10] = [
    MSG_TRADE_LINK_INVALID,
    MSG_ORDER_CREATED,
    MSG_ORDER_FAILED,
    MSG_MARKET_ERROR,
    MSG_TRADE_CREATED,
    MSG_TRADE_ACCEPTED,
    MSG_TRADE_FAILED_BUYER_REFUND,
    MSG_TRADE_FAILED_BUYER_PENALTY,
    MSG_TRADE_FAILED_SELLER_REFUND,
    MSG_TRADE_TIMEOUT,
];

/// Default templates for all Twitch bot chat messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ChatMessageTemplates {
    pub trade_link_invalid: String,
    pub order_created: String,
    pub order_failed: String,
    pub market_error: String,
    pub trade_created: String,
    pub trade_accepted: String,
    pub trade_failed_buyer_refund: String,
    pub trade_failed_buyer_penalty: String,
    pub trade_failed_seller_refund: String,
    pub trade_timeout: String,
}

impl Default for ChatMessageTemplates {
    fn default() -> Self {
        Self {
            trade_link_invalid: "@{buyer} не смог спарсить трейд ссылку, вернул баллы.".to_string(),
            order_created: "@{buyer} создал ордер на маркете, ожидай трейда в скорем времени (до 5-и минут) или другого сообщения от меня в чате".to_string(),
            order_failed: "@{buyer} не удалось создать ордер на маркете, вернул баллы. ошибка {code}: {error}".to_string(),
            market_error: "@{buyer} произошла внутренняя ошибка при отправке запроса на маркет. ничего трогать не буду, подробности в логах.".to_string(),
            trade_created: "@{buyer}, трейд был создан, у тебя есть {remaining} чтобы его принять - {tradeoffer}".to_string(),
            trade_accepted: "@{buyer} щекочет мой мозг, видимо трейд принял. не забудь об отзыве - @(ладно пока не надо отзывов на эту хуйню)".to_string(),
            trade_failed_buyer_refund: "@{buyer} въебал трейд? красавчик. повезло, что стример сказал возвращать баллы в таких случаях.".to_string(),
            trade_failed_buyer_penalty: "@{buyer} въебал трейд? красавчик. какое счастье, что стример сказал мне нихуя не возвращать в таких случаях. в следующий раз будь аккуратнее 😁😁😁😁".to_string(),
            trade_failed_seller_refund: "@{buyer} сорянчик, продавец долбоёб кажется решил нихуя не отправлять. ну или другая причина, крч возвращаю баллы, можешь попробовать ещё раз купить".to_string(),
            trade_timeout: "@{buyer} трейд превысил максимальное время ожидания (30 минут). баллы возвращать не буду во избежение потери денег.".to_string(),
        }
    }
}

impl ChatMessageTemplates {
    /// Convert templates to a flat HashMap<message_id, template_string>.
    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::with_capacity(10);
        map.insert(MSG_TRADE_LINK_INVALID.to_string(), self.trade_link_invalid.clone());
        map.insert(MSG_ORDER_CREATED.to_string(), self.order_created.clone());
        map.insert(MSG_ORDER_FAILED.to_string(), self.order_failed.clone());
        map.insert(MSG_MARKET_ERROR.to_string(), self.market_error.clone());
        map.insert(MSG_TRADE_CREATED.to_string(), self.trade_created.clone());
        map.insert(MSG_TRADE_ACCEPTED.to_string(), self.trade_accepted.clone());
        map.insert(MSG_TRADE_FAILED_BUYER_REFUND.to_string(), self.trade_failed_buyer_refund.clone());
        map.insert(MSG_TRADE_FAILED_BUYER_PENALTY.to_string(), self.trade_failed_buyer_penalty.clone());
        map.insert(MSG_TRADE_FAILED_SELLER_REFUND.to_string(), self.trade_failed_seller_refund.clone());
        map.insert(MSG_TRADE_TIMEOUT.to_string(), self.trade_timeout.clone());
        map
    }

    /// Construct templates from a flat map, falling back to defaults for any missing key.
    pub fn from_map(map: &HashMap<String, String>) -> Self {
        let default = Self::default();
        Self {
            trade_link_invalid: map.get(MSG_TRADE_LINK_INVALID).cloned().unwrap_or(default.trade_link_invalid),
            order_created: map.get(MSG_ORDER_CREATED).cloned().unwrap_or(default.order_created),
            order_failed: map.get(MSG_ORDER_FAILED).cloned().unwrap_or(default.order_failed),
            market_error: map.get(MSG_MARKET_ERROR).cloned().unwrap_or(default.market_error),
            trade_created: map.get(MSG_TRADE_CREATED).cloned().unwrap_or(default.trade_created),
            trade_accepted: map.get(MSG_TRADE_ACCEPTED).cloned().unwrap_or(default.trade_accepted),
            trade_failed_buyer_refund: map.get(MSG_TRADE_FAILED_BUYER_REFUND).cloned().unwrap_or(default.trade_failed_buyer_refund),
            trade_failed_buyer_penalty: map.get(MSG_TRADE_FAILED_BUYER_PENALTY).cloned().unwrap_or(default.trade_failed_buyer_penalty),
            trade_failed_seller_refund: map.get(MSG_TRADE_FAILED_SELLER_REFUND).cloned().unwrap_or(default.trade_failed_seller_refund),
            trade_timeout: map.get(MSG_TRADE_TIMEOUT).cloned().unwrap_or(default.trade_timeout),
        }
    }

    /// Return default template string for a specific message ID.
    pub fn get_default_message(message_id: &str) -> Option<String> {
        let default = Self::default();
        match message_id {
            MSG_TRADE_LINK_INVALID => Some(default.trade_link_invalid),
            MSG_ORDER_CREATED => Some(default.order_created),
            MSG_ORDER_FAILED => Some(default.order_failed),
            MSG_MARKET_ERROR => Some(default.market_error),
            MSG_TRADE_CREATED => Some(default.trade_created),
            MSG_TRADE_ACCEPTED => Some(default.trade_accepted),
            MSG_TRADE_FAILED_BUYER_REFUND => Some(default.trade_failed_buyer_refund),
            MSG_TRADE_FAILED_BUYER_PENALTY => Some(default.trade_failed_buyer_penalty),
            MSG_TRADE_FAILED_SELLER_REFUND => Some(default.trade_failed_seller_refund),
            MSG_TRADE_TIMEOUT => Some(default.trade_timeout),
            _ => None,
        }
    }

    /// List placeholders available for a given message ID.
    pub fn placeholders_for_message(message_id: &str) -> Vec<&'static str> {
        match message_id {
            MSG_TRADE_LINK_INVALID => vec!["buyer"],
            MSG_ORDER_CREATED => vec!["buyer"],
            MSG_ORDER_FAILED => vec!["buyer", "code", "error"],
            MSG_MARKET_ERROR => vec!["buyer"],
            MSG_TRADE_CREATED => vec!["buyer", "remaining", "tradeoffer"],
            MSG_TRADE_ACCEPTED => vec!["buyer"],
            MSG_TRADE_FAILED_BUYER_REFUND => vec!["buyer"],
            MSG_TRADE_FAILED_BUYER_PENALTY => vec!["buyer"],
            MSG_TRADE_FAILED_SELLER_REFUND => vec!["buyer"],
            MSG_TRADE_TIMEOUT => vec!["buyer"],
            _ => vec!["buyer"],
        }
    }

    /// Return map of all message keys to their supported placeholder lists.
    pub fn all_placeholders() -> HashMap<String, Vec<String>> {
        let mut map = HashMap::new();
        for key in ALL_MESSAGE_KEYS {
            map.insert(
                key.to_string(),
                Self::placeholders_for_message(key)
                    .into_iter()
                    .map(String::from)
                    .collect(),
            );
        }
        map
    }

    /// Merges custom user overrides onto default templates, returning a complete flat map.
    pub fn merge_with_defaults(custom: &HashMap<String, String>) -> HashMap<String, String> {
        let mut result = Self::default().to_map();
        for (k, v) in custom {
            if !v.trim().is_empty() {
                result.insert(k.clone(), v.clone());
            }
        }
        result
    }
}

/// Replace `{placeholder}` occurrences in `template` with values provided in `vars`.
/// This replacement is safe, fast, and does not panic if placeholders are unknown or omitted.
pub fn render_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, val) in vars {
        let pattern = format!("{{{}}}", key);
        result = result.replace(&pattern, val);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_template_all_vars() {
        let tpl = "@{buyer}, трейд создан! Ссылка: {tradeoffer} ({remaining})";
        let rendered = render_template(
            tpl,
            &[
                ("buyer", "alice"),
                ("remaining", "4m 30s"),
                ("tradeoffer", "https://steamcommunity.com/tradeoffer/123"),
            ],
        );
        assert_eq!(
            rendered,
            "@alice, трейд создан! Ссылка: https://steamcommunity.com/tradeoffer/123 (4m 30s)"
        );
    }

    #[test]
    fn test_render_template_missing_and_extra_vars() {
        let tpl = "@{buyer}: ошибка {code}!";
        let rendered = render_template(
            tpl,
            &[
                ("buyer", "bob"),
                ("extra", "ignore_me"),
            ],
        );
        assert_eq!(rendered, "@bob: ошибка {code}!");
    }

    #[test]
    fn test_default_message_templates_roundtrip() {
        let defaults = ChatMessageTemplates::default();
        let map = defaults.to_map();
        assert_eq!(map.len(), 10);
        let restored = ChatMessageTemplates::from_map(&map);
        assert_eq!(defaults, restored);
    }

    #[test]
    fn test_merge_with_defaults() {
        let mut custom = HashMap::new();
        custom.insert(
            MSG_ORDER_CREATED.to_string(),
            "Custom order message for {buyer}".to_string(),
        );
        // empty string should not override default
        custom.insert(MSG_TRADE_ACCEPTED.to_string(), "".to_string());

        let merged = ChatMessageTemplates::merge_with_defaults(&custom);
        assert_eq!(merged.get(MSG_ORDER_CREATED).unwrap(), "Custom order message for {buyer}");
        assert_eq!(
            merged.get(MSG_TRADE_ACCEPTED).unwrap(),
            &ChatMessageTemplates::default().trade_accepted
        );
    }
}
