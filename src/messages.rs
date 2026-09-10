use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── Categories ─────────────────────────────────────────────────────────────
pub const CAT_ORDERS: &str = "orders";
pub const CAT_MARKET_ERRORS: &str = "market_errors";
pub const CAT_TRADES: &str = "trades";
pub const CAT_CHAT_REQUIREMENTS: &str = "chat_requirements";
pub const CAT_LIMITS: &str = "limits";

pub const ALL_CATEGORIES: [&str; 5] = [
    CAT_ORDERS,
    CAT_MARKET_ERRORS,
    CAT_TRADES,
    CAT_CHAT_REQUIREMENTS,
    CAT_LIMITS,
];

// ── Orders Message Keys ────────────────────────────────────────────────────
pub const MSG_ORDERS_CREATED: &str = "orders.created";
pub const MSG_ORDERS_POOL_CREATED: &str = "orders.pool_created";
pub const MSG_ORDERS_FAILED: &str = "orders.failed";
pub const MSG_ORDERS_FAILED_NO_MONEY_REFUND: &str = "orders.failed_no_money_refund";
pub const MSG_ORDERS_FAILED_NO_MONEY_PENALTY: &str = "orders.failed_no_money_penalty";
pub const MSG_ORDERS_FAILED_FILTER_EXHAUSTED: &str = "orders.failed_filter_exhausted";
pub const MSG_ORDERS_MARKET_ERROR: &str = "orders.market_error";
pub const MSG_ORDERS_TRADE_LINK_INVALID: &str = "orders.trade_link_invalid";
pub const MSG_ORDERS_RETRYING: &str = "orders.retrying";
pub const MSG_ORDERS_MANUAL_HOLD: &str = "orders.manual_hold";

// ── Market Errors Message Keys (buy-for) ───────────────────────────────────
pub const MSG_MARKET_ERR_UNKNOWN: &str = "market_errors.unknown";
pub const MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED: &str = "market_errors.trade_link_check_failed";
pub const MSG_MARKET_ERR_INVENTORY_HIDDEN: &str = "market_errors.inventory_hidden";
pub const MSG_MARKET_ERR_STEAM_BANNED: &str = "market_errors.steam_banned";
pub const MSG_MARKET_ERR_NO_MOBILE_AUTH: &str = "market_errors.no_mobile_authenticator";
pub const MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED: &str = "market_errors.offline_trades_disabled";
pub const MSG_MARKET_ERR_TRADE_LINK_INVALID: &str = "market_errors.trade_link_invalid";
pub const MSG_MARKET_ERR_BOT_BANNED: &str = "market_errors.trade_check_bot_banned";
pub const MSG_MARKET_ERR_INVENTORY_FULL: &str = "market_errors.inventory_full";

// ── Trades Message Keys ────────────────────────────────────────────────────
pub const MSG_TRADES_CREATED: &str = "trades.created";
pub const MSG_TRADES_ACCEPTED: &str = "trades.accepted";
pub const MSG_TRADES_FAILED_BUYER_REFUND: &str = "trades.failed_buyer_refund";
pub const MSG_TRADES_FAILED_BUYER_PENALTY: &str = "trades.failed_buyer_penalty";
pub const MSG_TRADES_FAILED_SELLER_REFUND: &str = "trades.failed_seller_refund";
pub const MSG_TRADES_TIMEOUT: &str = "trades.timeout";

// ── Chat Requirements Message Keys ─────────────────────────────────────────
pub const MSG_CHAT_REQ_FAILED_MESSAGES_REFUND: &str = "chat_requirements.messages_refund";
pub const MSG_CHAT_REQ_FAILED_MESSAGES_PENALTY: &str = "chat_requirements.messages_penalty";
pub const MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND: &str = "chat_requirements.characters_refund";
pub const MSG_CHAT_REQ_FAILED_CHARACTERS_PENALTY: &str = "chat_requirements.characters_penalty";
pub const MSG_CHAT_REQ_FAILED_BOTH_REFUND: &str = "chat_requirements.both_refund";
pub const MSG_CHAT_REQ_FAILED_BOTH_PENALTY: &str = "chat_requirements.both_penalty";

// ── Limits Message Keys ────────────────────────────────────────────────────
pub const MSG_LIMITS_USER_LIMIT_REACHED: &str = "limits.user_limit_reached";
pub const MSG_LIMITS_GLOBAL_LIMIT_REACHED: &str = "limits.global_limit_reached";

// ── Backwards-Compatibility Aliases ─────────────────────────────────────────
pub const MSG_TRADE_LINK_INVALID: &str = MSG_ORDERS_TRADE_LINK_INVALID;
pub const MSG_ORDER_CREATED: &str = MSG_ORDERS_CREATED;
pub const MSG_ORDER_POOL_CREATED: &str = MSG_ORDERS_POOL_CREATED;
pub const MSG_ORDER_FAILED: &str = MSG_ORDERS_FAILED;
pub const MSG_ORDER_FAILED_NO_MONEY_REFUND: &str = MSG_ORDERS_FAILED_NO_MONEY_REFUND;
pub const MSG_ORDER_FAILED_NO_MONEY_PENALTY: &str = MSG_ORDERS_FAILED_NO_MONEY_PENALTY;
pub const MSG_ORDER_FAILED_FILTER_EXHAUSTED: &str = MSG_ORDERS_FAILED_FILTER_EXHAUSTED;
pub const MSG_ORDER_RETRYING: &str = MSG_ORDERS_RETRYING;
pub const MSG_ORDER_MANUAL_HOLD: &str = MSG_ORDERS_MANUAL_HOLD;
pub const MSG_MARKET_ERROR: &str = MSG_ORDERS_MARKET_ERROR;
pub const MSG_TRADE_CREATED: &str = MSG_TRADES_CREATED;
pub const MSG_TRADE_ACCEPTED: &str = MSG_TRADES_ACCEPTED;
pub const MSG_TRADE_FAILED_BUYER_REFUND: &str = MSG_TRADES_FAILED_BUYER_REFUND;
pub const MSG_TRADE_FAILED_BUYER_PENALTY: &str = MSG_TRADES_FAILED_BUYER_PENALTY;
pub const MSG_TRADE_FAILED_SELLER_REFUND: &str = MSG_TRADES_FAILED_SELLER_REFUND;
pub const MSG_TRADE_TIMEOUT: &str = MSG_TRADES_TIMEOUT;
pub const MSG_USER_PURCHASE_LIMIT_REACHED: &str = MSG_LIMITS_USER_LIMIT_REACHED;
pub const MSG_GLOBAL_PURCHASE_LIMIT_REACHED: &str = MSG_LIMITS_GLOBAL_LIMIT_REACHED;
pub const MSG_CHAT_REQ_FAILED_MESSAGES: &str = MSG_CHAT_REQ_FAILED_MESSAGES_REFUND;
pub const MSG_CHAT_REQ_FAILED_CHARACTERS: &str = MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND;
pub const MSG_CHAT_REQ_FAILED_BOTH: &str = MSG_CHAT_REQ_FAILED_BOTH_REFUND;

// ── Categorized Structs ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct OrdersMessages {
    pub created: String,
    pub pool_created: String,
    pub failed: String,
    pub failed_no_money_refund: String,
    pub failed_no_money_penalty: String,
    pub failed_filter_exhausted: String,
    pub market_error: String,
    pub trade_link_invalid: String,
    pub retrying: String,
    pub manual_hold: String,
}

impl Default for OrdersMessages {
    fn default() -> Self {
        Self {
            created: "@{buyer} Market order created. Please wait for the trade offer (up to 5 minutes).".to_string(),
            pool_created: "@{buyer} Rolled skin {item} (chance: {chance})! Market order created. Please wait for the trade offer (up to 5 minutes).".to_string(),
            failed: "@{buyer} Failed to create market order. Channel points refunded. Error {code}: {error}".to_string(),
            failed_no_money_refund: "@{buyer} Insufficient bot balance to purchase the item. Channel points refunded.".to_string(),
            failed_no_money_penalty: "@{buyer} Insufficient bot balance to purchase the item. Channel points are not refunded per streamer settings.".to_string(),
            failed_filter_exhausted: "@{buyer} No items found matching the reward filters (attempts exhausted). Channel points refunded.".to_string(),
            market_error: "@{buyer} An internal market error occurred. Please check logs for details.".to_string(),
            trade_link_invalid: "@{buyer} Invalid Steam trade URL. Channel points refunded.".to_string(),
            retrying: "@{buyer} Initial market order attempt failed, retrying automatically. This may take up to 30 minutes.".to_string(),
            manual_hold: "@{buyer} Item purchase could not be completed automatically. Your request is on hold for manual streamer review. Channel points are preserved.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct MarketErrorsMessages {
    pub unknown: String,
    pub trade_link_check_failed: String,
    pub inventory_hidden: String,
    pub steam_banned: String,
    pub no_mobile_authenticator: String,
    pub offline_trades_disabled: String,
    pub trade_link_invalid: String,
    pub trade_check_bot_banned: String,
    pub inventory_full: String,
}

impl Default for MarketErrorsMessages {
    fn default() -> Self {
        Self {
            unknown: "@{buyer} Market error: an unknown error occurred. Channel points refunded.".to_string(),
            trade_link_check_failed: "@{buyer} Market failed to verify your trade link. Channel points refunded.".to_string(),
            inventory_hidden: "@{buyer} Your Steam inventory is private. Please set your inventory to public and try again. Channel points refunded.".to_string(),
            steam_banned: "@{buyer} Your Steam account is banned or cannot trade. Channel points refunded.".to_string(),
            no_mobile_authenticator: "@{buyer} Steam Guard Mobile Authenticator is not enabled on your account. Channel points refunded.".to_string(),
            offline_trades_disabled: "@{buyer} Error verifying trade link. Please enable offline trade offers in your Steam settings. Channel points refunded.".to_string(),
            trade_link_invalid: "@{buyer} Your Steam trade link is invalid. Channel points refunded.".to_string(),
            trade_check_bot_banned: "@{buyer} Market verification bot is currently unavailable. Please try again later. Channel points refunded.".to_string(),
            inventory_full: "@{buyer} Your CS2 inventory is full. Please free up space and try again. Channel points refunded.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct TradesMessages {
    pub created: String,
    pub accepted: String,
    pub failed_buyer_refund: String,
    pub failed_buyer_penalty: String,
    pub failed_seller_refund: String,
    pub timeout: String,
}

impl Default for TradesMessages {
    fn default() -> Self {
        Self {
            created: "@{buyer} Trade offer created. You have {remaining} to accept it: {tradeoffer}".to_string(),
            accepted: "@{buyer} Trade offer accepted. Enjoy your skin!".to_string(),
            failed_buyer_refund: "@{buyer} Trade offer failed or was declined. Channel points refunded.".to_string(),
            failed_buyer_penalty: "@{buyer} Trade offer failed or was declined. Channel points are not refunded per streamer settings.".to_string(),
            failed_seller_refund: "@{buyer} Seller failed to send the item. Channel points refunded.".to_string(),
            timeout: "@{buyer} Trade offer timed out. Channel points are not refunded.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ChatRequirementsMessages {
    pub messages_refund: String,
    pub messages_penalty: String,
    pub characters_refund: String,
    pub characters_penalty: String,
    pub both_refund: String,
    pub both_penalty: String,
}

impl Default for ChatRequirementsMessages {
    fn default() -> Self {
        Self {
            messages_refund: "@{buyer} Not enough chat messages: you have {user_messages}, required {min_messages} ({period}). Channel points refunded.".to_string(),
            messages_penalty: "@{buyer} Not enough chat messages: you have {user_messages}, required {min_messages} ({period}). Channel points are not refunded.".to_string(),
            characters_refund: "@{buyer} Not enough chat characters: you have {user_characters}, required {min_characters} ({period}). Channel points refunded.".to_string(),
            characters_penalty: "@{buyer} Not enough chat characters: you have {user_characters}, required {min_characters} ({period}). Channel points are not refunded.".to_string(),
            both_refund: "@{buyer} Not enough chat activity: required {min_messages} messages {operator} {min_characters} characters ({period}). Channel points refunded.".to_string(),
            both_penalty: "@{buyer} Not enough chat activity: required {min_messages} messages {operator} {min_characters} characters ({period}). Channel points are not refunded.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct LimitsMessages {
    pub user_limit_reached: String,
    pub global_limit_reached: String,
}

impl Default for LimitsMessages {
    fn default() -> Self {
        Self {
            user_limit_reached: "@{buyer} You have reached the purchase limit for this reward ({limit} / {period}). Channel points refunded.".to_string(),
            global_limit_reached: "@{buyer} Global purchase limit for this reward has been reached ({limit} / {period}). Reward paused, channel points refunded.".to_string(),
        }
    }
}

/// All Twitch bot chat message templates grouped into 5 logical categories.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
#[serde(from = "serde_json::Value")]
pub struct CategorizedChatMessages {
    pub orders: OrdersMessages,
    pub market_errors: MarketErrorsMessages,
    pub trades: TradesMessages,
    pub chat_requirements: ChatRequirementsMessages,
    pub limits: LimitsMessages,
}

impl From<serde_json::Value> for CategorizedChatMessages {
    fn from(value: serde_json::Value) -> Self {
        let mut result = Self::default();

        let obj = match value.as_object() {
            Some(o) => o,
            None => return result,
        };

        // If new nested format is present (at least one category is a JSON object):
        let is_nested = obj.iter().any(|(_, v)| v.is_object());

        if is_nested {
            if let Some(orders_val) = obj.get("orders").and_then(|v| v.as_object()) {
                if let Some(v) = orders_val.get("created").and_then(|s| s.as_str()) { result.orders.created = v.to_string(); }
                if let Some(v) = orders_val.get("pool_created").and_then(|s| s.as_str()) { result.orders.pool_created = v.to_string(); }
                if let Some(v) = orders_val.get("failed").and_then(|s| s.as_str()) { result.orders.failed = v.to_string(); }
                if let Some(v) = orders_val.get("failed_no_money_refund").and_then(|s| s.as_str()) { result.orders.failed_no_money_refund = v.to_string(); }
                if let Some(v) = orders_val.get("failed_no_money_penalty").and_then(|s| s.as_str()) { result.orders.failed_no_money_penalty = v.to_string(); }
                if let Some(v) = orders_val.get("failed_filter_exhausted").and_then(|s| s.as_str()) { result.orders.failed_filter_exhausted = v.to_string(); }
                if let Some(v) = orders_val.get("market_error").and_then(|s| s.as_str()) { result.orders.market_error = v.to_string(); }
                if let Some(v) = orders_val.get("trade_link_invalid").and_then(|s| s.as_str()) { result.orders.trade_link_invalid = v.to_string(); }
                if let Some(v) = orders_val.get("retrying").and_then(|s| s.as_str()) { result.orders.retrying = v.to_string(); }
                if let Some(v) = orders_val.get("manual_hold").and_then(|s| s.as_str()) { result.orders.manual_hold = v.to_string(); }
            }
            if let Some(m_val) = obj.get("market_errors").and_then(|v| v.as_object()) {
                if let Some(v) = m_val.get("unknown").and_then(|s| s.as_str()) { result.market_errors.unknown = v.to_string(); }
                if let Some(v) = m_val.get("trade_link_check_failed").and_then(|s| s.as_str()) { result.market_errors.trade_link_check_failed = v.to_string(); }
                if let Some(v) = m_val.get("inventory_hidden").and_then(|s| s.as_str()) { result.market_errors.inventory_hidden = v.to_string(); }
                if let Some(v) = m_val.get("steam_banned").and_then(|s| s.as_str()) { result.market_errors.steam_banned = v.to_string(); }
                if let Some(v) = m_val.get("no_mobile_authenticator").and_then(|s| s.as_str()) { result.market_errors.no_mobile_authenticator = v.to_string(); }
                if let Some(v) = m_val.get("offline_trades_disabled").and_then(|s| s.as_str()) { result.market_errors.offline_trades_disabled = v.to_string(); }
                if let Some(v) = m_val.get("trade_link_invalid").and_then(|s| s.as_str()) { result.market_errors.trade_link_invalid = v.to_string(); }
                if let Some(v) = m_val.get("trade_check_bot_banned").and_then(|s| s.as_str()) { result.market_errors.trade_check_bot_banned = v.to_string(); }
                if let Some(v) = m_val.get("inventory_full").and_then(|s| s.as_str()) { result.market_errors.inventory_full = v.to_string(); }
            }
            if let Some(t_val) = obj.get("trades").and_then(|v| v.as_object()) {
                if let Some(v) = t_val.get("created").and_then(|s| s.as_str()) { result.trades.created = v.to_string(); }
                if let Some(v) = t_val.get("accepted").and_then(|s| s.as_str()) { result.trades.accepted = v.to_string(); }
                if let Some(v) = t_val.get("failed_buyer_refund").and_then(|s| s.as_str()) { result.trades.failed_buyer_refund = v.to_string(); }
                if let Some(v) = t_val.get("failed_buyer_penalty").and_then(|s| s.as_str()) { result.trades.failed_buyer_penalty = v.to_string(); }
                if let Some(v) = t_val.get("failed_seller_refund").and_then(|s| s.as_str()) { result.trades.failed_seller_refund = v.to_string(); }
                if let Some(v) = t_val.get("timeout").and_then(|s| s.as_str()) { result.trades.timeout = v.to_string(); }
            }
            if let Some(c_val) = obj.get("chat_requirements").and_then(|v| v.as_object()) {
                if let Some(v) = c_val.get("messages_refund").and_then(|s| s.as_str()) { result.chat_requirements.messages_refund = v.to_string(); }
                if let Some(v) = c_val.get("messages_penalty").and_then(|s| s.as_str()) { result.chat_requirements.messages_penalty = v.to_string(); }
                if let Some(v) = c_val.get("characters_refund").and_then(|s| s.as_str()) { result.chat_requirements.characters_refund = v.to_string(); }
                if let Some(v) = c_val.get("characters_penalty").and_then(|s| s.as_str()) { result.chat_requirements.characters_penalty = v.to_string(); }
                if let Some(v) = c_val.get("both_refund").and_then(|s| s.as_str()) { result.chat_requirements.both_refund = v.to_string(); }
                if let Some(v) = c_val.get("both_penalty").and_then(|s| s.as_str()) { result.chat_requirements.both_penalty = v.to_string(); }
            }
            if let Some(l_val) = obj.get("limits").and_then(|v| v.as_object()) {
                if let Some(v) = l_val.get("user_limit_reached").and_then(|s| s.as_str()) { result.limits.user_limit_reached = v.to_string(); }
                if let Some(v) = l_val.get("global_limit_reached").and_then(|s| s.as_str()) { result.limits.global_limit_reached = v.to_string(); }
            }
            return result;
        }

        // Otherwise handle legacy flat format:
        for (k, v) in obj {
            let val_str = match v.as_str() {
                Some(s) if !s.trim().is_empty() => s.to_string(),
                _ => continue,
            };
            if let Some((cat, key)) = resolve_category_and_key(k) {
                match (cat, key) {
                    ("orders", "created") => result.orders.created = val_str,
                    ("orders", "pool_created") => result.orders.pool_created = val_str,
                    ("orders", "failed") => result.orders.failed = val_str,
                    ("orders", "failed_no_money_refund") => result.orders.failed_no_money_refund = val_str,
                    ("orders", "failed_no_money_penalty") => result.orders.failed_no_money_penalty = val_str,
                    ("orders", "failed_filter_exhausted") => result.orders.failed_filter_exhausted = val_str,
                    ("orders", "market_error") => result.orders.market_error = val_str,
                    ("orders", "trade_link_invalid") => result.orders.trade_link_invalid = val_str,
                    ("orders", "retrying") => result.orders.retrying = val_str,
                    ("orders", "manual_hold") => result.orders.manual_hold = val_str,

                    ("market_errors", "unknown") => result.market_errors.unknown = val_str,
                    ("market_errors", "trade_link_check_failed") => result.market_errors.trade_link_check_failed = val_str,
                    ("market_errors", "inventory_hidden") => result.market_errors.inventory_hidden = val_str,
                    ("market_errors", "steam_banned") => result.market_errors.steam_banned = val_str,
                    ("market_errors", "no_mobile_authenticator") => result.market_errors.no_mobile_authenticator = val_str,
                    ("market_errors", "offline_trades_disabled") => result.market_errors.offline_trades_disabled = val_str,
                    ("market_errors", "trade_link_invalid") => result.market_errors.trade_link_invalid = val_str,
                    ("market_errors", "trade_check_bot_banned") => result.market_errors.trade_check_bot_banned = val_str,
                    ("market_errors", "inventory_full") => result.market_errors.inventory_full = val_str,

                    ("trades", "created") => result.trades.created = val_str,
                    ("trades", "accepted") => result.trades.accepted = val_str,
                    ("trades", "failed_buyer_refund") => result.trades.failed_buyer_refund = val_str,
                    ("trades", "failed_buyer_penalty") => result.trades.failed_buyer_penalty = val_str,
                    ("trades", "failed_seller_refund") => result.trades.failed_seller_refund = val_str,
                    ("trades", "timeout") => result.trades.timeout = val_str,

                    ("chat_requirements", "messages_refund") => result.chat_requirements.messages_refund = val_str,
                    ("chat_requirements", "messages_penalty") => result.chat_requirements.messages_penalty = val_str,
                    ("chat_requirements", "characters_refund") => result.chat_requirements.characters_refund = val_str,
                    ("chat_requirements", "characters_penalty") => result.chat_requirements.characters_penalty = val_str,
                    ("chat_requirements", "both_refund") => result.chat_requirements.both_refund = val_str,
                    ("chat_requirements", "both_penalty") => result.chat_requirements.both_penalty = val_str,

                    ("limits", "user_limit_reached") => result.limits.user_limit_reached = val_str,
                    ("limits", "global_limit_reached") => result.limits.global_limit_reached = val_str,
                    _ => {}
                }
            }
        }

        result
    }
}

// ── Placeholders Struct ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct CategorizedPlaceholders {
    pub orders: HashMap<String, Vec<String>>,
    pub market_errors: HashMap<String, Vec<String>>,
    pub trades: HashMap<String, Vec<String>>,
    pub chat_requirements: HashMap<String, Vec<String>>,
    pub limits: HashMap<String, Vec<String>>,
}

impl CategorizedChatMessages {
    /// Retrieve template by either dot-notation ("category.key") or legacy flat key.
    pub fn get_message(&self, message_id: &str) -> Option<&str> {
        let (cat, key) = resolve_category_and_key(message_id)?;
        match cat {
            "orders" => match key {
                "created" => Some(&self.orders.created),
                "pool_created" => Some(&self.orders.pool_created),
                "failed" => Some(&self.orders.failed),
                "failed_no_money_refund" => Some(&self.orders.failed_no_money_refund),
                "failed_no_money_penalty" => Some(&self.orders.failed_no_money_penalty),
                "failed_filter_exhausted" => Some(&self.orders.failed_filter_exhausted),
                "market_error" => Some(&self.orders.market_error),
                "trade_link_invalid" => Some(&self.orders.trade_link_invalid),
                "retrying" => Some(&self.orders.retrying),
                "manual_hold" => Some(&self.orders.manual_hold),
                _ => None,
            },
            "market_errors" => match key {
                "unknown" => Some(&self.market_errors.unknown),
                "trade_link_check_failed" => Some(&self.market_errors.trade_link_check_failed),
                "inventory_hidden" => Some(&self.market_errors.inventory_hidden),
                "steam_banned" => Some(&self.market_errors.steam_banned),
                "no_mobile_authenticator" => Some(&self.market_errors.no_mobile_authenticator),
                "offline_trades_disabled" => Some(&self.market_errors.offline_trades_disabled),
                "trade_link_invalid" => Some(&self.market_errors.trade_link_invalid),
                "trade_check_bot_banned" => Some(&self.market_errors.trade_check_bot_banned),
                "inventory_full" => Some(&self.market_errors.inventory_full),
                _ => None,
            },
            "trades" => match key {
                "created" => Some(&self.trades.created),
                "accepted" => Some(&self.trades.accepted),
                "failed_buyer_refund" => Some(&self.trades.failed_buyer_refund),
                "failed_buyer_penalty" => Some(&self.trades.failed_buyer_penalty),
                "failed_seller_refund" => Some(&self.trades.failed_seller_refund),
                "timeout" => Some(&self.trades.timeout),
                _ => None,
            },
            "chat_requirements" => match key {
                "messages_refund" => Some(&self.chat_requirements.messages_refund),
                "messages_penalty" => Some(&self.chat_requirements.messages_penalty),
                "characters_refund" => Some(&self.chat_requirements.characters_refund),
                "characters_penalty" => Some(&self.chat_requirements.characters_penalty),
                "both_refund" => Some(&self.chat_requirements.both_refund),
                "both_penalty" => Some(&self.chat_requirements.both_penalty),
                _ => None,
            },
            "limits" => match key {
                "user_limit_reached" => Some(&self.limits.user_limit_reached),
                "global_limit_reached" => Some(&self.limits.global_limit_reached),
                _ => None,
            },
            _ => None,
        }
    }

    /// Return default template for a given message ID.
    pub fn get_default_message(message_id: &str) -> Option<String> {
        let defaults = Self::default();
        defaults.get_message(message_id).map(|s| s.to_string())
    }

    /// Merges custom user overrides onto default templates, returning a complete CategorizedChatMessages.
    pub fn merge_with_overrides(
        defaults: &Self,
        overrides: &HashMap<String, HashMap<String, String>>,
    ) -> Self {
        let mut merged = defaults.clone();
        for (cat, map) in overrides {
            for (key, val) in map {
                let trimmed = val.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match (cat.as_str(), key.as_str()) {
                    ("orders", "created") => merged.orders.created = trimmed.to_string(),
                    ("orders", "pool_created") => merged.orders.pool_created = trimmed.to_string(),
                    ("orders", "failed") => merged.orders.failed = trimmed.to_string(),
                    ("orders", "failed_no_money_refund") => merged.orders.failed_no_money_refund = trimmed.to_string(),
                    ("orders", "failed_no_money_penalty") => merged.orders.failed_no_money_penalty = trimmed.to_string(),
                    ("orders", "failed_filter_exhausted") => merged.orders.failed_filter_exhausted = trimmed.to_string(),
                    ("orders", "market_error") => merged.orders.market_error = trimmed.to_string(),
                    ("orders", "trade_link_invalid") => merged.orders.trade_link_invalid = trimmed.to_string(),
                    ("orders", "retrying") => merged.orders.retrying = trimmed.to_string(),
                    ("orders", "manual_hold") => merged.orders.manual_hold = trimmed.to_string(),

                    ("market_errors", "unknown") => merged.market_errors.unknown = trimmed.to_string(),
                    ("market_errors", "trade_link_check_failed") => merged.market_errors.trade_link_check_failed = trimmed.to_string(),
                    ("market_errors", "inventory_hidden") => merged.market_errors.inventory_hidden = trimmed.to_string(),
                    ("market_errors", "steam_banned") => merged.market_errors.steam_banned = trimmed.to_string(),
                    ("market_errors", "no_mobile_authenticator") => merged.market_errors.no_mobile_authenticator = trimmed.to_string(),
                    ("market_errors", "offline_trades_disabled") => merged.market_errors.offline_trades_disabled = trimmed.to_string(),
                    ("market_errors", "trade_link_invalid") => merged.market_errors.trade_link_invalid = trimmed.to_string(),
                    ("market_errors", "trade_check_bot_banned") => merged.market_errors.trade_check_bot_banned = trimmed.to_string(),
                    ("market_errors", "inventory_full") => merged.market_errors.inventory_full = trimmed.to_string(),

                    ("trades", "created") => merged.trades.created = trimmed.to_string(),
                    ("trades", "accepted") => merged.trades.accepted = trimmed.to_string(),
                    ("trades", "failed_buyer_refund") => merged.trades.failed_buyer_refund = trimmed.to_string(),
                    ("trades", "failed_buyer_penalty") => merged.trades.failed_buyer_penalty = trimmed.to_string(),
                    ("trades", "failed_seller_refund") => merged.trades.failed_seller_refund = trimmed.to_string(),
                    ("trades", "timeout") => merged.trades.timeout = trimmed.to_string(),

                    ("chat_requirements", "messages_refund") => merged.chat_requirements.messages_refund = trimmed.to_string(),
                    ("chat_requirements", "messages_penalty") => merged.chat_requirements.messages_penalty = trimmed.to_string(),
                    ("chat_requirements", "characters_refund") => merged.chat_requirements.characters_refund = trimmed.to_string(),
                    ("chat_requirements", "characters_penalty") => merged.chat_requirements.characters_penalty = trimmed.to_string(),
                    ("chat_requirements", "both_refund") => merged.chat_requirements.both_refund = trimmed.to_string(),
                    ("chat_requirements", "both_penalty") => merged.chat_requirements.both_penalty = trimmed.to_string(),

                    ("limits", "user_limit_reached") => merged.limits.user_limit_reached = trimmed.to_string(),
                    ("limits", "global_limit_reached") => merged.limits.global_limit_reached = trimmed.to_string(),
                    _ => {}
                }
            }
        }
        merged
    }

    /// Return map of category -> { message_key -> [placeholders] }.
    pub fn all_placeholders() -> CategorizedPlaceholders {
        let mut orders = HashMap::new();
        orders.insert("created".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        orders.insert("pool_created".to_string(), vec!["buyer".to_string(), "item".to_string(), "chance".to_string()]);
        orders.insert("failed".to_string(), vec!["buyer".to_string(), "code".to_string(), "error".to_string(), "item".to_string()]);
        orders.insert("failed_no_money_refund".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        orders.insert("failed_no_money_penalty".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        orders.insert("failed_filter_exhausted".to_string(), vec!["buyer".to_string(), "attempts".to_string()]);
        orders.insert("market_error".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        orders.insert("trade_link_invalid".to_string(), vec!["buyer".to_string()]);
        orders.insert("retrying".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        orders.insert("manual_hold".to_string(), vec!["buyer".to_string(), "item".to_string()]);

        let mut market_errors = HashMap::new();
        market_errors.insert("unknown".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("trade_link_check_failed".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("inventory_hidden".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("steam_banned".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("no_mobile_authenticator".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("offline_trades_disabled".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("trade_link_invalid".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("trade_check_bot_banned".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        market_errors.insert("inventory_full".to_string(), vec!["buyer".to_string(), "item".to_string()]);

        let mut trades = HashMap::new();
        trades.insert("created".to_string(), vec!["buyer".to_string(), "remaining".to_string(), "tradeoffer".to_string(), "item".to_string()]);
        trades.insert("accepted".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        trades.insert("failed_buyer_refund".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        trades.insert("failed_buyer_penalty".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        trades.insert("failed_seller_refund".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        trades.insert("timeout".to_string(), vec!["buyer".to_string(), "item".to_string()]);

        let mut chat_requirements = HashMap::new();
        let chat_msg_vars = vec!["buyer".to_string(), "user_messages".to_string(), "min_messages".to_string(), "hours".to_string(), "period".to_string()];
        let chat_char_vars = vec!["buyer".to_string(), "user_characters".to_string(), "min_characters".to_string(), "hours".to_string(), "period".to_string()];
        let chat_both_vars = vec!["buyer".to_string(), "user_messages".to_string(), "min_messages".to_string(), "user_characters".to_string(), "min_characters".to_string(), "hours".to_string(), "period".to_string(), "operator".to_string()];
        chat_requirements.insert("messages_refund".to_string(), chat_msg_vars.clone());
        chat_requirements.insert("messages_penalty".to_string(), chat_msg_vars);
        chat_requirements.insert("characters_refund".to_string(), chat_char_vars.clone());
        chat_requirements.insert("characters_penalty".to_string(), chat_char_vars);
        chat_requirements.insert("both_refund".to_string(), chat_both_vars.clone());
        chat_requirements.insert("both_penalty".to_string(), chat_both_vars);

        let mut limits = HashMap::new();
        limits.insert("user_limit_reached".to_string(), vec!["buyer".to_string(), "limit".to_string(), "period".to_string(), "item".to_string()]);
        limits.insert("global_limit_reached".to_string(), vec!["buyer".to_string(), "limit".to_string(), "period".to_string(), "item".to_string()]);

        CategorizedPlaceholders {
            orders,
            market_errors,
            trades,
            chat_requirements,
            limits,
        }
    }
}

/// Resolves either a dot-notation key ("orders.created") or a legacy flat key ("order_created")
/// into a category name and key inside that category.
pub fn resolve_category_and_key(message_id: &str) -> Option<(&'static str, &'static str)> {
    match message_id {
        // Dot-notation
        "orders.created" => Some(("orders", "created")),
        "orders.pool_created" => Some(("orders", "pool_created")),
        "orders.failed" => Some(("orders", "failed")),
        "orders.failed_no_money_refund" => Some(("orders", "failed_no_money_refund")),
        "orders.failed_no_money_penalty" => Some(("orders", "failed_no_money_penalty")),
        "orders.failed_filter_exhausted" => Some(("orders", "failed_filter_exhausted")),
        "orders.market_error" => Some(("orders", "market_error")),
        "orders.trade_link_invalid" => Some(("orders", "trade_link_invalid")),
        "orders.retrying" => Some(("orders", "retrying")),
        "orders.manual_hold" => Some(("orders", "manual_hold")),

        "market_errors.unknown" => Some(("market_errors", "unknown")),
        "market_errors.trade_link_check_failed" => Some(("market_errors", "trade_link_check_failed")),
        "market_errors.inventory_hidden" => Some(("market_errors", "inventory_hidden")),
        "market_errors.steam_banned" => Some(("market_errors", "steam_banned")),
        "market_errors.no_mobile_authenticator" => Some(("market_errors", "no_mobile_authenticator")),
        "market_errors.offline_trades_disabled" => Some(("market_errors", "offline_trades_disabled")),
        "market_errors.trade_link_invalid" => Some(("market_errors", "trade_link_invalid")),
        "market_errors.trade_check_bot_banned" => Some(("market_errors", "trade_check_bot_banned")),
        "market_errors.inventory_full" => Some(("market_errors", "inventory_full")),

        "trades.created" => Some(("trades", "created")),
        "trades.accepted" => Some(("trades", "accepted")),
        "trades.failed_buyer_refund" => Some(("trades", "failed_buyer_refund")),
        "trades.failed_buyer_penalty" => Some(("trades", "failed_buyer_penalty")),
        "trades.failed_seller_refund" => Some(("trades", "failed_seller_refund")),
        "trades.timeout" => Some(("trades", "timeout")),

        "chat_requirements.messages_refund" => Some(("chat_requirements", "messages_refund")),
        "chat_requirements.messages_penalty" => Some(("chat_requirements", "messages_penalty")),
        "chat_requirements.characters_refund" => Some(("chat_requirements", "characters_refund")),
        "chat_requirements.characters_penalty" => Some(("chat_requirements", "characters_penalty")),
        "chat_requirements.both_refund" => Some(("chat_requirements", "both_refund")),
        "chat_requirements.both_penalty" => Some(("chat_requirements", "both_penalty")),

        "limits.user_limit_reached" => Some(("limits", "user_limit_reached")),
        "limits.global_limit_reached" => Some(("limits", "global_limit_reached")),

        // Legacy flat keys
        "order_created" => Some(("orders", "created")),
        "order_pool_created" => Some(("orders", "pool_created")),
        "order_failed" => Some(("orders", "failed")),
        "order_failed_no_money_refund" => Some(("orders", "failed_no_money_refund")),
        "order_failed_no_money_penalty" => Some(("orders", "failed_no_money_penalty")),
        "order_failed_filter_exhausted" => Some(("orders", "failed_filter_exhausted")),
        "order_retrying" => Some(("orders", "retrying")),
        "order_manual_hold" => Some(("orders", "manual_hold")),
        "market_error" => Some(("orders", "market_error")),
        "trade_link_invalid" => Some(("orders", "trade_link_invalid")),

        "trade_created" => Some(("trades", "created")),
        "trade_accepted" => Some(("trades", "accepted")),
        "trade_failed_buyer_refund" => Some(("trades", "failed_buyer_refund")),
        "trade_failed_buyer_penalty" => Some(("trades", "failed_buyer_penalty")),
        "trade_failed_seller_refund" => Some(("trades", "failed_seller_refund")),
        "trade_timeout" => Some(("trades", "timeout")),

        "chat_req_failed_messages_refund" | "chat_req_failed_messages" => Some(("chat_requirements", "messages_refund")),
        "chat_req_failed_messages_penalty" => Some(("chat_requirements", "messages_penalty")),
        "chat_req_failed_characters_refund" | "chat_req_failed_characters" => Some(("chat_requirements", "characters_refund")),
        "chat_req_failed_characters_penalty" => Some(("chat_requirements", "characters_penalty")),
        "chat_req_failed_both_refund" | "chat_req_failed_both" => Some(("chat_requirements", "both_refund")),
        "chat_req_failed_both_penalty" => Some(("chat_requirements", "both_penalty")),

        "user_purchase_limit_reached" => Some(("limits", "user_limit_reached")),
        "global_purchase_limit_reached" => Some(("limits", "global_limit_reached")),

        _ => None,
    }
}

/// Parses a JSON Value representing custom overrides into HashMap<category, HashMap<key, val>>.
/// Handles both new nested category objects and legacy flat key maps.
pub fn parse_custom_messages(val: serde_json::Value) -> HashMap<String, HashMap<String, String>> {
    let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();

    let obj = match val.as_object() {
        Some(o) => o,
        None => return result,
    };

    let is_nested = obj.iter().any(|(_, v)| v.is_object());

    if is_nested {
        for (cat, sub) in obj {
            if let Some(sub_obj) = sub.as_object() {
                let cat_map = result.entry(cat.clone()).or_default();
                for (k, v) in sub_obj {
                    if let Some(s) = v.as_str() {
                        if !s.trim().is_empty() {
                            cat_map.insert(k.clone(), s.to_string());
                        }
                    }
                }
            }
        }
    } else {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    if let Some((cat, subkey)) = resolve_category_and_key(k) {
                        result.entry(cat.to_string()).or_default().insert(subkey.to_string(), s.to_string());
                    }
                }
            }
        }
    }

    result
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

// ── Backwards compatibility adapter ────────────────────────────────────────
pub type ChatMessageTemplates = CategorizedChatMessages;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_template() {
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
    fn test_categorized_messages_defaults() {
        let defaults = CategorizedChatMessages::default();
        assert_eq!(defaults.get_message(MSG_ORDERS_CREATED).unwrap(), &defaults.orders.created);
        assert_eq!(defaults.get_message("order_created").unwrap(), &defaults.orders.created);
        assert_eq!(defaults.get_message(MSG_MARKET_ERR_INVENTORY_HIDDEN).unwrap(), &defaults.market_errors.inventory_hidden);
        assert_eq!(defaults.get_message(MSG_TRADES_ACCEPTED).unwrap(), &defaults.trades.accepted);
        assert_eq!(defaults.get_message(MSG_CHAT_REQ_FAILED_MESSAGES_REFUND).unwrap(), &defaults.chat_requirements.messages_refund);
        assert_eq!(defaults.get_message(MSG_LIMITS_USER_LIMIT_REACHED).unwrap(), &defaults.limits.user_limit_reached);
    }

    #[test]
    fn test_deserialize_nested_and_legacy_flat_json() {
        // Legacy flat format
        let flat_json = serde_json::json!({
            "order_created": "Custom flat order created",
            "trade_accepted": "Custom flat trade accepted"
        });
        let from_flat: CategorizedChatMessages = serde_json::from_value(flat_json).unwrap();
        assert_eq!(from_flat.orders.created, "Custom flat order created");
        assert_eq!(from_flat.trades.accepted, "Custom flat trade accepted");
        // default filled for rest
        assert_eq!(from_flat.market_errors.inventory_hidden, CategorizedChatMessages::default().market_errors.inventory_hidden);

        // Nested category format
        let nested_json = serde_json::json!({
            "orders": {
                "created": "Custom nested created"
            },
            "market_errors": {
                "inventory_hidden": "Custom nested hidden"
            }
        });
        let from_nested: CategorizedChatMessages = serde_json::from_value(nested_json).unwrap();
        assert_eq!(from_nested.orders.created, "Custom nested created");
        assert_eq!(from_nested.market_errors.inventory_hidden, "Custom nested hidden");
        assert_eq!(from_nested.trades.created, CategorizedChatMessages::default().trades.created);
    }

    #[test]
    fn test_merge_with_overrides() {
        let defaults = CategorizedChatMessages::default();
        let mut custom = HashMap::new();
        let mut orders_map = HashMap::new();
        orders_map.insert("created".to_string(), "Overridden order created".to_string());
        orders_map.insert("failed".to_string(), "".to_string()); // empty ignored
        custom.insert("orders".to_string(), orders_map);

        let merged = CategorizedChatMessages::merge_with_overrides(&defaults, &custom);
        assert_eq!(merged.orders.created, "Overridden order created");
        assert_eq!(merged.orders.failed, defaults.orders.failed);
    }

    #[test]
    fn test_parse_custom_messages_both_formats() {
        let flat = serde_json::json!({
            "order_created": "My order",
            "trade_timeout": "My timeout"
        });
        let parsed_flat = parse_custom_messages(flat);
        assert_eq!(parsed_flat.get("orders").unwrap().get("created").unwrap(), "My order");
        assert_eq!(parsed_flat.get("trades").unwrap().get("timeout").unwrap(), "My timeout");

        let nested = serde_json::json!({
            "market_errors": {
                "inventory_hidden": "Custom hidden"
            }
        });
        let parsed_nested = parse_custom_messages(nested);
        assert_eq!(parsed_nested.get("market_errors").unwrap().get("inventory_hidden").unwrap(), "Custom hidden");
    }

    #[test]
    fn test_orders_pool_created_template() {
        let defaults = CategorizedChatMessages::default();
        let tpl = defaults.get_message(MSG_ORDERS_POOL_CREATED).expect("pool_created template must exist");
        let rendered = render_template(tpl, &[
            ("buyer", "viewer1"),
            ("item", "MAC-10 | Bronzer"),
            ("chance", "0.5%"),
        ]);
        assert_eq!(rendered, "@viewer1 Rolled skin MAC-10 | Bronzer (chance: 0.5%)! Market order created. Please wait for the trade offer (up to 5 minutes).");

        let placeholders = CategorizedChatMessages::all_placeholders();
        let pool_vars = placeholders.orders.get("pool_created").expect("orders.pool_created placeholders must exist");
        assert!(pool_vars.contains(&"chance".to_string()));
        assert!(pool_vars.contains(&"item".to_string()));
        assert!(pool_vars.contains(&"buyer".to_string()));
    }
}
