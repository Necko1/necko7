use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MSG_TRADE_LINK_INVALID: &str = "trade_link_invalid";
pub const MSG_ORDER_CREATED: &str = "order_created";
pub const MSG_ORDER_FAILED: &str = "order_failed";
pub const MSG_ORDER_FAILED_NO_MONEY_REFUND: &str = "order_failed_no_money_refund";
pub const MSG_ORDER_FAILED_NO_MONEY_PENALTY: &str = "order_failed_no_money_penalty";
pub const MSG_ORDER_FAILED_FILTER_EXHAUSTED: &str = "order_failed_filter_exhausted";
pub const MSG_MARKET_ERROR: &str = "market_error";
pub const MSG_TRADE_CREATED: &str = "trade_created";
pub const MSG_TRADE_ACCEPTED: &str = "trade_accepted";
pub const MSG_TRADE_FAILED_BUYER_REFUND: &str = "trade_failed_buyer_refund";
pub const MSG_TRADE_FAILED_BUYER_PENALTY: &str = "trade_failed_buyer_penalty";
pub const MSG_TRADE_FAILED_SELLER_REFUND: &str = "trade_failed_seller_refund";
pub const MSG_TRADE_TIMEOUT: &str = "trade_timeout";
pub const MSG_CHAT_REQ_FAILED_MESSAGES_REFUND: &str = "chat_req_failed_messages_refund";
pub const MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY: &str = "chat_req_failed_messages_penalty";
pub const MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND: &str = "chat_req_failed_characters_refund";
pub const MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY: &str = "chat_req_failed_characters_penalty";
pub const MSG_CHAT_REQ_FAILED_BOTH_REFUND: &str = "chat_req_failed_both_refund";
pub const MSG_CHAT_REQ_FAILED_BOTH_PENALTY: &str = "chat_req_failed_both_penalty";
pub const MSG_USER_PURCHASE_LIMIT_REACHED: &str = "user_purchase_limit_reached";
pub const MSG_GLOBAL_PURCHASE_LIMIT_REACHED: &str = "global_purchase_limit_reached";

// Legacy keys retained for backwards compatibility
pub const MSG_CHAT_REQ_FAILED_MESSAGES: &str = "chat_req_failed_messages";
pub const MSG_CHAT_REQ_FAILED_CHARACTERS: &str = "chat_req_failed_characters";
pub const MSG_CHAT_REQ_FAILED_BOTH: &str = "chat_req_failed_both";

pub const ALL_MESSAGE_KEYS: [&str; 21] = [
    MSG_TRADE_LINK_INVALID,
    MSG_ORDER_CREATED,
    MSG_ORDER_FAILED,
    MSG_ORDER_FAILED_NO_MONEY_REFUND,
    MSG_ORDER_FAILED_NO_MONEY_PENALTY,
    MSG_ORDER_FAILED_FILTER_EXHAUSTED,
    MSG_MARKET_ERROR,
    MSG_TRADE_CREATED,
    MSG_TRADE_ACCEPTED,
    MSG_TRADE_FAILED_BUYER_REFUND,
    MSG_TRADE_FAILED_BUYER_PENALTY,
    MSG_TRADE_FAILED_SELLER_REFUND,
    MSG_TRADE_TIMEOUT,
    MSG_CHAT_REQ_FAILED_MESSAGES_REFUND,
    MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY,
    MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND,
    MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY,
    MSG_CHAT_REQ_FAILED_BOTH_REFUND,
    MSG_CHAT_REQ_FAILED_BOTH_PENALTY,
    MSG_USER_PURCHASE_LIMIT_REACHED,
    MSG_GLOBAL_PURCHASE_LIMIT_REACHED,
];

/// Default templates for all Twitch bot chat messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ChatMessageTemplates {
    pub trade_link_invalid: String,
    pub order_created: String,
    pub order_failed: String,
    pub order_failed_no_money_refund: String,
    pub order_failed_no_money_penalty: String,
    pub order_failed_filter_exhausted: String,
    pub market_error: String,
    pub trade_created: String,
    pub trade_accepted: String,
    pub trade_failed_buyer_refund: String,
    pub trade_failed_buyer_penalty: String,
    pub trade_failed_seller_refund: String,
    pub trade_timeout: String,
    pub chat_req_failed_messages_refund: String,
    pub chat_req_failed_messages_penalty: String,
    pub chat_req_failed_characters_refund: String,
    pub chat_req_failed_characters_penalty: String,
    pub chat_req_failed_both_refund: String,
    pub chat_req_failed_both_penalty: String,
    pub user_purchase_limit_reached: String,
    pub global_purchase_limit_reached: String,
}

impl Default for ChatMessageTemplates {
    fn default() -> Self {
        Self {
            trade_link_invalid: "@{buyer} Invalid Steam trade URL. Channel points refunded.".to_string(),
            order_created: "@{buyer} Market order created. Please wait for the trade offer (up to 5 minutes).".to_string(),
            order_failed: "@{buyer} Failed to create market order. Channel points refunded. Error {code}: {error}".to_string(),
            order_failed_no_money_refund: "@{buyer} Insufficient bot balance to purchase the item. Channel points refunded.".to_string(),
            order_failed_no_money_penalty: "@{buyer} Insufficient bot balance to purchase the item. Channel points are not refunded per streamer settings.".to_string(),
            order_failed_filter_exhausted: "@{buyer} No items found matching the reward filters (attempts exhausted). Channel points refunded.".to_string(),
            market_error: "@{buyer} An internal market error occurred. Please check logs for details.".to_string(),
            trade_created: "@{buyer} Trade offer created. You have {remaining} to accept it: {tradeoffer}".to_string(),
            trade_accepted: "@{buyer} Trade offer accepted. Enjoy your skin!".to_string(),
            trade_failed_buyer_refund: "@{buyer} Trade offer failed or was declined. Channel points refunded.".to_string(),
            trade_failed_buyer_penalty: "@{buyer} Trade offer failed or was declined. Channel points are not refunded per streamer settings.".to_string(),
            trade_failed_seller_refund: "@{buyer} Seller failed to send the item. Channel points refunded.".to_string(),
            trade_timeout: "@{buyer} Trade offer timed out. Channel points are not refunded.".to_string(),
            chat_req_failed_messages_refund: "@{buyer} Not enough chat messages: you have {user_messages}, required {min_messages} ({period}). Channel points refunded.".to_string(),
            chat_req_failed_messages_penalty: "@{buyer} Not enough chat messages: you have {user_messages}, required {min_messages} ({period}). Channel points are not refunded.".to_string(),
            chat_req_failed_characters_refund: "@{buyer} Not enough chat characters: you have {user_characters}, required {min_characters} ({period}). Channel points refunded.".to_string(),
            chat_req_failed_characters_penalty: "@{buyer} Not enough chat characters: you have {user_characters}, required {min_characters} ({period}). Channel points are not refunded.".to_string(),
            chat_req_failed_both_refund: "@{buyer} Not enough chat activity: required {min_messages} messages {operator} {min_characters} characters ({period}). Channel points refunded.".to_string(),
            chat_req_failed_both_penalty: "@{buyer} Not enough chat activity: required {min_messages} messages {operator} {min_characters} characters ({period}). Channel points are not refunded.".to_string(),
            user_purchase_limit_reached: "@{buyer} You have reached the purchase limit for this reward ({limit} / {period}). Channel points refunded.".to_string(),
            global_purchase_limit_reached: "@{buyer} Global purchase limit for this reward has been reached ({limit} / {period}). Reward paused, channel points refunded.".to_string(),
        }
    }
}

impl ChatMessageTemplates {
    /// Convert templates to a flat HashMap<message_id, template_string>.
    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::with_capacity(21);
        map.insert(MSG_TRADE_LINK_INVALID.to_string(), self.trade_link_invalid.clone());
        map.insert(MSG_ORDER_CREATED.to_string(), self.order_created.clone());
        map.insert(MSG_ORDER_FAILED.to_string(), self.order_failed.clone());
        map.insert(MSG_ORDER_FAILED_NO_MONEY_REFUND.to_string(), self.order_failed_no_money_refund.clone());
        map.insert(MSG_ORDER_FAILED_NO_MONEY_PENALTY.to_string(), self.order_failed_no_money_penalty.clone());
        map.insert(MSG_ORDER_FAILED_FILTER_EXHAUSTED.to_string(), self.order_failed_filter_exhausted.clone());
        map.insert(MSG_MARKET_ERROR.to_string(), self.market_error.clone());
        map.insert(MSG_TRADE_CREATED.to_string(), self.trade_created.clone());
        map.insert(MSG_TRADE_ACCEPTED.to_string(), self.trade_accepted.clone());
        map.insert(MSG_TRADE_FAILED_BUYER_REFUND.to_string(), self.trade_failed_buyer_refund.clone());
        map.insert(MSG_TRADE_FAILED_BUYER_PENALTY.to_string(), self.trade_failed_buyer_penalty.clone());
        map.insert(MSG_TRADE_FAILED_SELLER_REFUND.to_string(), self.trade_failed_seller_refund.clone());
        map.insert(MSG_TRADE_TIMEOUT.to_string(), self.trade_timeout.clone());
        map.insert(MSG_CHAT_REQ_FAILED_MESSAGES_REFUND.to_string(), self.chat_req_failed_messages_refund.clone());
        map.insert(MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY.to_string(), self.chat_req_failed_messages_penalty.clone());
        map.insert(MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND.to_string(), self.chat_req_failed_characters_refund.clone());
        map.insert(MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY.to_string(), self.chat_req_failed_characters_penalty.clone());
        map.insert(MSG_CHAT_REQ_FAILED_BOTH_REFUND.to_string(), self.chat_req_failed_both_refund.clone());
        map.insert(MSG_CHAT_REQ_FAILED_BOTH_PENALTY.to_string(), self.chat_req_failed_both_penalty.clone());
        map.insert(MSG_USER_PURCHASE_LIMIT_REACHED.to_string(), self.user_purchase_limit_reached.clone());
        map.insert(MSG_GLOBAL_PURCHASE_LIMIT_REACHED.to_string(), self.global_purchase_limit_reached.clone());
        map
    }

    /// Construct templates from a flat map, falling back to defaults for any missing key.
    pub fn from_map(map: &HashMap<String, String>) -> Self {
        let default = Self::default();
        Self {
            trade_link_invalid: map.get(MSG_TRADE_LINK_INVALID).cloned().unwrap_or(default.trade_link_invalid),
            order_created: map.get(MSG_ORDER_CREATED).cloned().unwrap_or(default.order_created),
            order_failed: map.get(MSG_ORDER_FAILED).cloned().unwrap_or(default.order_failed),
            order_failed_no_money_refund: map.get(MSG_ORDER_FAILED_NO_MONEY_REFUND).cloned().unwrap_or(default.order_failed_no_money_refund),
            order_failed_no_money_penalty: map.get(MSG_ORDER_FAILED_NO_MONEY_PENALTY).cloned().unwrap_or(default.order_failed_no_money_penalty),
            order_failed_filter_exhausted: map.get(MSG_ORDER_FAILED_FILTER_EXHAUSTED).cloned().unwrap_or(default.order_failed_filter_exhausted),
            market_error: map.get(MSG_MARKET_ERROR).cloned().unwrap_or(default.market_error),
            trade_created: map.get(MSG_TRADE_CREATED).cloned().unwrap_or(default.trade_created),
            trade_accepted: map.get(MSG_TRADE_ACCEPTED).cloned().unwrap_or(default.trade_accepted),
            trade_failed_buyer_refund: map.get(MSG_TRADE_FAILED_BUYER_REFUND).cloned().unwrap_or(default.trade_failed_buyer_refund),
            trade_failed_buyer_penalty: map.get(MSG_TRADE_FAILED_BUYER_PENALTY).cloned().unwrap_or(default.trade_failed_buyer_penalty),
            trade_failed_seller_refund: map.get(MSG_TRADE_FAILED_SELLER_REFUND).cloned().unwrap_or(default.trade_failed_seller_refund),
            trade_timeout: map.get(MSG_TRADE_TIMEOUT).cloned().unwrap_or(default.trade_timeout),
            chat_req_failed_messages_refund: map.get(MSG_CHAT_REQ_FAILED_MESSAGES_REFUND)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_MESSAGES))
                .cloned()
                .unwrap_or(default.chat_req_failed_messages_refund),
            chat_req_failed_messages_penalty: map.get(MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_MESSAGES))
                .cloned()
                .unwrap_or(default.chat_req_failed_messages_penalty),
            chat_req_failed_characters_refund: map.get(MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_CHARACTERS))
                .cloned()
                .unwrap_or(default.chat_req_failed_characters_refund),
            chat_req_failed_characters_penalty: map.get(MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_CHARACTERS))
                .cloned()
                .unwrap_or(default.chat_req_failed_characters_penalty),
            chat_req_failed_both_refund: map.get(MSG_CHAT_REQ_FAILED_BOTH_REFUND)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_BOTH))
                .cloned()
                .unwrap_or(default.chat_req_failed_both_refund),
            chat_req_failed_both_penalty: map.get(MSG_CHAT_REQ_FAILED_BOTH_PENALTY)
                .or_else(|| map.get(MSG_CHAT_REQ_FAILED_BOTH))
                .cloned()
                .unwrap_or(default.chat_req_failed_both_penalty),
            user_purchase_limit_reached: map.get(MSG_USER_PURCHASE_LIMIT_REACHED)
                .cloned()
                .unwrap_or(default.user_purchase_limit_reached),
            global_purchase_limit_reached: map.get(MSG_GLOBAL_PURCHASE_LIMIT_REACHED)
                .cloned()
                .unwrap_or(default.global_purchase_limit_reached),
        }
    }

    /// Return default template string for a specific message ID.
    pub fn get_default_message(message_id: &str) -> Option<String> {
        let default = Self::default();
        match message_id {
            MSG_TRADE_LINK_INVALID => Some(default.trade_link_invalid),
            MSG_ORDER_CREATED => Some(default.order_created),
            MSG_ORDER_FAILED => Some(default.order_failed),
            MSG_ORDER_FAILED_NO_MONEY_REFUND => Some(default.order_failed_no_money_refund),
            MSG_ORDER_FAILED_NO_MONEY_PENALTY => Some(default.order_failed_no_money_penalty),
            MSG_ORDER_FAILED_FILTER_EXHAUSTED => Some(default.order_failed_filter_exhausted),
            MSG_MARKET_ERROR => Some(default.market_error),
            MSG_TRADE_CREATED => Some(default.trade_created),
            MSG_TRADE_ACCEPTED => Some(default.trade_accepted),
            MSG_TRADE_FAILED_BUYER_REFUND => Some(default.trade_failed_buyer_refund),
            MSG_TRADE_FAILED_BUYER_PENALTY => Some(default.trade_failed_buyer_penalty),
            MSG_TRADE_FAILED_SELLER_REFUND => Some(default.trade_failed_seller_refund),
            MSG_TRADE_TIMEOUT => Some(default.trade_timeout),
            MSG_CHAT_REQ_FAILED_MESSAGES_REFUND | MSG_CHAT_REQ_FAILED_MESSAGES => Some(default.chat_req_failed_messages_refund),
            MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY => Some(default.chat_req_failed_messages_penalty),
            MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND | MSG_CHAT_REQ_FAILED_CHARACTERS => Some(default.chat_req_failed_characters_refund),
            MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY => Some(default.chat_req_failed_characters_penalty),
            MSG_CHAT_REQ_FAILED_BOTH_REFUND | MSG_CHAT_REQ_FAILED_BOTH => Some(default.chat_req_failed_both_refund),
            MSG_CHAT_REQ_FAILED_BOTH_PENALTY => Some(default.chat_req_failed_both_penalty),
            MSG_USER_PURCHASE_LIMIT_REACHED => Some(default.user_purchase_limit_reached),
            MSG_GLOBAL_PURCHASE_LIMIT_REACHED => Some(default.global_purchase_limit_reached),
            _ => None,
        }
    }

    /// List placeholders available for a given message ID.
    pub fn placeholders_for_message(message_id: &str) -> Vec<&'static str> {
        match message_id {
            MSG_TRADE_LINK_INVALID => vec!["buyer"],
            MSG_ORDER_CREATED => vec!["buyer", "item"],
            MSG_ORDER_FAILED => vec!["buyer", "code", "error"],
            MSG_ORDER_FAILED_NO_MONEY_REFUND => vec!["buyer"],
            MSG_ORDER_FAILED_NO_MONEY_PENALTY => vec!["buyer"],
            MSG_ORDER_FAILED_FILTER_EXHAUSTED => vec!["buyer", "attempts"],
            MSG_MARKET_ERROR => vec!["buyer"],
            MSG_TRADE_CREATED => vec!["buyer", "remaining", "tradeoffer", "item"],
            MSG_TRADE_ACCEPTED => vec!["buyer", "item"],
            MSG_TRADE_FAILED_BUYER_REFUND => vec!["buyer", "item"],
            MSG_TRADE_FAILED_BUYER_PENALTY => vec!["buyer", "item"],
            MSG_TRADE_FAILED_SELLER_REFUND => vec!["buyer", "item"],
            MSG_TRADE_TIMEOUT => vec!["buyer", "item"],
            MSG_CHAT_REQ_FAILED_MESSAGES_REFUND | MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY | MSG_CHAT_REQ_FAILED_MESSAGES => {
                vec!["buyer", "user_messages", "min_messages", "hours", "period"]
            }
            MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND | MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY | MSG_CHAT_REQ_FAILED_CHARACTERS => {
                vec!["buyer", "user_characters", "min_characters", "hours", "period"]
            }
            MSG_CHAT_REQ_FAILED_BOTH_REFUND | MSG_CHAT_REQ_FAILED_BOTH_PENALTY | MSG_CHAT_REQ_FAILED_BOTH => {
                vec!["buyer", "user_messages", "min_messages", "user_characters", "min_characters", "hours", "period", "operator"]
            }
            MSG_USER_PURCHASE_LIMIT_REACHED | MSG_GLOBAL_PURCHASE_LIMIT_REACHED => {
                vec!["buyer", "limit", "period", "item"]
            }
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
        assert_eq!(map.len(), 21);
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

    #[test]
    fn test_render_chat_requirement_templates() {
        let defaults = ChatMessageTemplates::default();

        let rendered_msgs_ref = render_template(
            &defaults.chat_req_failed_messages_refund,
            &[
                ("buyer", "alice"),
                ("user_messages", "12"),
                ("min_messages", "50"),
                ("period", "72h"),
            ],
        );
        assert_eq!(
            rendered_msgs_ref,
            "@alice Not enough chat messages: you have 12, required 50 (72h). Channel points refunded."
        );

        let rendered_msgs_pen = render_template(
            &defaults.chat_req_failed_messages_penalty,
            &[
                ("buyer", "alice"),
                ("user_messages", "12"),
                ("min_messages", "50"),
                ("period", "all-time"),
            ],
        );
        assert_eq!(
            rendered_msgs_pen,
            "@alice Not enough chat messages: you have 12, required 50 (all-time). Channel points are not refunded."
        );
    }
}
