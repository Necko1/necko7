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
pub const MSG_ORDERS_WAITING_VIEWER: &str = "orders.waiting_viewer";
pub const MSG_ORDERS_WAITING_OPERATOR: &str = "orders.waiting_operator";
pub const MSG_ORDERS_TRADE_LINK_REQUIRED: &str = "orders.trade_link_required";
pub const MSG_ORDERS_INSUFFICIENT_FUNDS: &str = "orders.insufficient_funds";
pub const MSG_ORDERS_UNAVAILABLE: &str = "orders.unavailable";
pub const MSG_ORDERS_RECONCILIATION_REQUIRED: &str = "orders.reconciliation_required";
pub const MSG_ORDERS_REFUNDED: &str = "orders.refunded";

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
pub const MSG_TRADES_FAILED_BUYER: &str = "trades.failed_buyer";
pub const MSG_TRADES_FAILED_SELLER: &str = "trades.failed_seller";
pub const MSG_TRADES_REVERTED_BUYER: &str = "trades.reverted_buyer";
pub const MSG_TRADES_REVERTED_SELLER: &str = "trades.reverted_seller";

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

// ── Call-site aliases ──────────────────────────────────────────────────────
pub const MSG_USER_PURCHASE_LIMIT_REACHED: &str = MSG_LIMITS_USER_LIMIT_REACHED;
pub const MSG_GLOBAL_PURCHASE_LIMIT_REACHED: &str = MSG_LIMITS_GLOBAL_LIMIT_REACHED;
pub const MSG_CHAT_REQ_FAILED_MESSAGES: &str = MSG_CHAT_REQ_FAILED_MESSAGES_REFUND;
pub const MSG_CHAT_REQ_FAILED_CHARACTERS: &str = MSG_CHAT_REQ_FAILED_CHARACTERS_REFUND;
pub const MSG_CHAT_REQ_FAILED_BOTH: &str = MSG_CHAT_REQ_FAILED_BOTH_REFUND;

// ── Categorized Structs ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct OrdersMessages {
    pub created: String,
    pub waiting_viewer: String,
    pub waiting_operator: String,
    pub trade_link_required: String,
    pub insufficient_funds: String,
    pub unavailable: String,
    pub reconciliation_required: String,
    pub refunded: String,
}

impl Default for OrdersMessages {
    fn default() -> Self {
        Self {
            created: "@{buyer} Market order created for {item}. Watch for the Steam trade offer; you can follow delivery in your inventory.".to_string(),
            waiting_viewer: "@{buyer} {item} is in your inventory. Your auto-buy preference is off, so no Market order was placed. Start delivery or refund your points from your inventory.".to_string(),
            waiting_operator: "@{buyer} {item} is in your inventory. Auto-buy is off for this reward, so no Market order was placed. The channel team will review delivery.".to_string(),
            trade_link_required: "@{buyer} {item} needs a valid Steam trade link. Check the link in your reward message or profile, then start delivery from your inventory; your points remain pending.".to_string(),
            insufficient_funds: "@{buyer} Market could not order {item} because the channel account has insufficient balance. Your points remain pending; try again later or request a refund from your inventory.".to_string(),
            unavailable: "@{buyer} Market could not find {item} at or below {price} with the configured transfer chance. Your points remain pending; check your inventory for available actions.".to_string(),
            reconciliation_required: "@{buyer} The order or trade status for {item} is unconfirmed. It may still deliver; your points remain pending and inventory actions are unavailable until its status is resolved.".to_string(),
            refunded: "@{buyer} Your channel points for {item} were refunded. Inventory fulfillment is closed.".to_string(),
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
            unknown: "@{buyer} Market returned an unclear response for {item}. An order may already exist; your points remain pending and inventory actions are unavailable until its status is resolved.".to_string(),
            trade_link_check_failed: "@{buyer} Market could not verify the trade link for {item}. Check the link and your inventory for available actions; your points remain pending.".to_string(),
            inventory_hidden: "@{buyer} Market could not order {item} because your Steam inventory is private. Make it public, then check your inventory for available actions; your points remain pending.".to_string(),
            steam_banned: "@{buyer} Market could not order {item} because your Steam account cannot trade. Check Steam restrictions and your inventory for available actions; your points remain pending.".to_string(),
            no_mobile_authenticator: "@{buyer} Market could not order {item} because Steam Guard Mobile Authenticator is not enabled. Check your Steam account and inventory for available actions; your points remain pending.".to_string(),
            offline_trades_disabled: "@{buyer} Market could not order {item} because offline trade offers are unavailable on your Steam account. Check Steam settings and your inventory for available actions; your points remain pending.".to_string(),
            trade_link_invalid: "@{buyer} Market rejected the trade link for {item}. Correct the link, then check your inventory for available actions; your points remain pending.".to_string(),
            trade_check_bot_banned: "@{buyer} Market could not verify the trade link for {item} because its checking bot is unavailable. Check your inventory for available actions; your points remain pending.".to_string(),
            inventory_full: "@{buyer} Market could not order {item} because your CS2 inventory is full. Free up space, then check your inventory for available actions; your points remain pending.".to_string(),
        }
    }
}

// Older settings saves copied every displayed default into the override JSON.
// Ignore only those exact obsolete defaults; separately authored messages stay intact.
fn is_legacy_market_default(key: &str, value: &str) -> bool {
    matches!((key, value),
        ("unknown", "@{buyer} Market error: an unknown error occurred. Channel points refunded.") |
        ("trade_link_check_failed", "@{buyer} Market failed to verify your trade link. Channel points refunded.") |
        ("inventory_hidden", "@{buyer} Your Steam inventory is private. Please set your inventory to public and try again. Channel points refunded.") |
        ("steam_banned", "@{buyer} Your Steam account is banned or cannot trade. Channel points refunded.") |
        ("no_mobile_authenticator", "@{buyer} Steam Guard Mobile Authenticator is not enabled on your account. Channel points refunded.") |
        ("offline_trades_disabled", "@{buyer} Error verifying trade link. Please enable offline trade offers in your Steam settings. Channel points refunded.") |
        ("trade_link_invalid", "@{buyer} Your Steam trade link is invalid. Channel points refunded.") |
        ("trade_check_bot_banned", "@{buyer} Market verification bot is currently unavailable. Please try again later. Channel points refunded.") |
        ("inventory_full", "@{buyer} Your CS2 inventory is full. Please free up space and try again. Channel points refunded.")
    )
}

pub(crate) fn is_obsolete_copied_default(category: &str, key: &str, value: &str) -> bool {
    if category == "market_errors" && is_legacy_market_default(key, value) {
        return true;
    }
    matches!((category, key, value),
        ("orders", "unavailable", "@{buyer} Market could not order {item} at its fixed price. Your points remain pending; you can retry later or request a refund from your inventory.") |
        ("orders", "reconciliation_required", "@{buyer} Market's status for {item} is being checked. Please do not start another order or refund until it is resolved; your points remain pending.") |
        ("market_errors", "unknown", "@{buyer} Market returned an uncertain result for {item}. We are checking whether an order exists; your points remain pending. Do not retry or refund yet.") |
        ("trades", "accepted", "@{buyer} Trade offer accepted. Enjoy your skin!")
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct TradesMessages {
    pub created: String,
    pub accepted: String,
    pub failed_buyer: String,
    pub failed_seller: String,
    pub reverted_buyer: String,
    pub reverted_seller: String,
}

impl Default for TradesMessages {
    fn default() -> Self {
        Self {
            created: "@{buyer} Trade offer created. You have {remaining} to accept it: {tradeoffer}".to_string(),
            accepted: "@{buyer} The trade for {item} was accepted. Market is still confirming its final outcome; your points remain pending.".to_string(),
            failed_buyer: "@{buyer} The Steam trade for {item} ended without delivery on the buyer side. Your points remain pending; check your inventory for available actions.".to_string(),
            failed_seller: "@{buyer} The Market trade for {item} ended without delivery. Your points remain pending; check your inventory to retry or request a refund.".to_string(),
            reverted_buyer: "@{buyer} You reverted the accepted trade for {item}. Your points remain pending; contact the channel operator for help.".to_string(),
            reverted_seller: "@{buyer} The seller reverted the accepted trade for {item}. It is available in your inventory to try delivery again; your points remain pending.".to_string(),
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
                if let Some(v) = orders_val.get("waiting_viewer").and_then(|s| s.as_str()) { result.orders.waiting_viewer = v.to_string(); }
                if let Some(v) = orders_val.get("waiting_operator").and_then(|s| s.as_str()) { result.orders.waiting_operator = v.to_string(); }
                if let Some(v) = orders_val.get("trade_link_required").and_then(|s| s.as_str()) { result.orders.trade_link_required = v.to_string(); }
                if let Some(v) = orders_val.get("insufficient_funds").and_then(|s| s.as_str()) { result.orders.insufficient_funds = v.to_string(); }
                if let Some(v) = orders_val.get("unavailable").and_then(|s| s.as_str()) { result.orders.unavailable = v.to_string(); }
                if let Some(v) = orders_val.get("reconciliation_required").and_then(|s| s.as_str()) { result.orders.reconciliation_required = v.to_string(); }
                if let Some(v) = orders_val.get("refunded").and_then(|s| s.as_str()) { result.orders.refunded = v.to_string(); }
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
                if let Some(v) = t_val.get("failed_buyer").and_then(|s| s.as_str()) { result.trades.failed_buyer = v.to_string(); }
                if let Some(v) = t_val.get("failed_seller").and_then(|s| s.as_str()) { result.trades.failed_seller = v.to_string(); }
                if let Some(v) = t_val.get("reverted_buyer").and_then(|s| s.as_str()) { result.trades.reverted_buyer = v.to_string(); }
                if let Some(v) = t_val.get("reverted_seller").and_then(|s| s.as_str()) { result.trades.reverted_seller = v.to_string(); }
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
                    ("orders", "waiting_viewer") => result.orders.waiting_viewer = val_str,
                    ("orders", "waiting_operator") => result.orders.waiting_operator = val_str,
                    ("orders", "trade_link_required") => result.orders.trade_link_required = val_str,
                    ("orders", "insufficient_funds") => result.orders.insufficient_funds = val_str,
                    ("orders", "unavailable") => result.orders.unavailable = val_str,
                    ("orders", "reconciliation_required") => result.orders.reconciliation_required = val_str,
                    ("orders", "refunded") => result.orders.refunded = val_str,

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
                    ("trades", "failed_buyer") => result.trades.failed_buyer = val_str,
                    ("trades", "failed_seller") => result.trades.failed_seller = val_str,
                    ("trades", "reverted_buyer") => result.trades.reverted_buyer = val_str,
                    ("trades", "reverted_seller") => result.trades.reverted_seller = val_str,

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
                "waiting_viewer" => Some(&self.orders.waiting_viewer),
                "waiting_operator" => Some(&self.orders.waiting_operator),
                "trade_link_required" => Some(&self.orders.trade_link_required),
                "insufficient_funds" => Some(&self.orders.insufficient_funds),
                "unavailable" => Some(&self.orders.unavailable),
                "reconciliation_required" => Some(&self.orders.reconciliation_required),
                "refunded" => Some(&self.orders.refunded),
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
                "failed_buyer" => Some(&self.trades.failed_buyer),
                "failed_seller" => Some(&self.trades.failed_seller),
                "reverted_buyer" => Some(&self.trades.reverted_buyer),
                "reverted_seller" => Some(&self.trades.reverted_seller),
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
                if is_obsolete_copied_default(cat, key, trimmed) {
                    continue;
                }
                match (cat.as_str(), key.as_str()) {
                    ("orders", "created") => merged.orders.created = trimmed.to_string(),
                    ("orders", "waiting_viewer") => merged.orders.waiting_viewer = trimmed.to_string(),
                    ("orders", "waiting_operator") => merged.orders.waiting_operator = trimmed.to_string(),
                    ("orders", "trade_link_required") => merged.orders.trade_link_required = trimmed.to_string(),
                    ("orders", "insufficient_funds") => merged.orders.insufficient_funds = trimmed.to_string(),
                    ("orders", "unavailable") => merged.orders.unavailable = trimmed.to_string(),
                    ("orders", "reconciliation_required") => merged.orders.reconciliation_required = trimmed.to_string(),
                    ("orders", "refunded") => merged.orders.refunded = trimmed.to_string(),

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
                    ("trades", "failed_buyer") => merged.trades.failed_buyer = trimmed.to_string(),
                    ("trades", "failed_seller") => merged.trades.failed_seller = trimmed.to_string(),
                    ("trades", "reverted_buyer") => merged.trades.reverted_buyer = trimmed.to_string(),
                    ("trades", "reverted_seller") => merged.trades.reverted_seller = trimmed.to_string(),

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
        for key in ["waiting_viewer", "waiting_operator", "trade_link_required", "insufficient_funds", "reconciliation_required", "refunded"] {
            orders.insert(key.to_string(), vec!["buyer".to_string(), "item".to_string()]);
        }
        orders.insert("unavailable".to_string(), vec!["buyer".to_string(), "item".to_string(), "price".to_string()]);
        let mut market_errors = HashMap::new();
        for key in ["unknown", "trade_link_check_failed", "inventory_hidden", "steam_banned", "no_mobile_authenticator", "offline_trades_disabled", "trade_link_invalid", "trade_check_bot_banned", "inventory_full"] {
            market_errors.insert(key.to_string(), vec!["buyer".to_string(), "item".to_string()]);
        }
        let mut trades = HashMap::new();
        trades.insert("created".to_string(), vec!["buyer".to_string(), "remaining".to_string(), "tradeoffer".to_string(), "item".to_string()]);
        trades.insert("accepted".to_string(), vec!["buyer".to_string(), "item".to_string()]);
        for key in ["failed_buyer", "failed_seller", "reverted_buyer", "reverted_seller"] {
            trades.insert(key.to_string(), vec!["buyer".to_string(), "item".to_string()]);
        }

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
        "orders.waiting_viewer" => Some(("orders", "waiting_viewer")),
        "orders.waiting_operator" => Some(("orders", "waiting_operator")),
        "orders.trade_link_required" => Some(("orders", "trade_link_required")),
        "orders.insufficient_funds" => Some(("orders", "insufficient_funds")),
        "orders.unavailable" => Some(("orders", "unavailable")),
        "orders.reconciliation_required" => Some(("orders", "reconciliation_required")),
        "orders.refunded" => Some(("orders", "refunded")),
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
        "trades.failed_buyer" => Some(("trades", "failed_buyer")),
        "trades.failed_seller" => Some(("trades", "failed_seller")),
        "trades.reverted_buyer" => Some(("trades", "reverted_buyer")),
        "trades.reverted_seller" => Some(("trades", "reverted_seller")),

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
        "trade_created" => Some(("trades", "created")),
        "trade_accepted" => Some(("trades", "accepted")),

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
                        if !s.trim().is_empty() && !is_obsolete_copied_default(cat, k, s) {
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
                        if !is_obsolete_copied_default(cat, subkey, s) {
                            result.entry(cat.to_string()).or_default().insert(subkey.to_string(), s.to_string());
                        }
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
    fn inventory_chat_templates_are_customizable_and_have_their_placeholders() {
        let defaults = CategorizedChatMessages::default();
        let placeholders = CategorizedChatMessages::all_placeholders();
        for key in [
            MSG_ORDERS_WAITING_VIEWER, MSG_ORDERS_WAITING_OPERATOR,
            MSG_ORDERS_TRADE_LINK_REQUIRED, MSG_ORDERS_INSUFFICIENT_FUNDS,
            MSG_ORDERS_UNAVAILABLE,
            MSG_ORDERS_RECONCILIATION_REQUIRED, MSG_ORDERS_REFUNDED,
            MSG_MARKET_ERR_UNKNOWN, MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED,
            MSG_MARKET_ERR_INVENTORY_HIDDEN, MSG_MARKET_ERR_STEAM_BANNED,
            MSG_MARKET_ERR_NO_MOBILE_AUTH, MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED,
            MSG_MARKET_ERR_TRADE_LINK_INVALID, MSG_MARKET_ERR_BOT_BANNED,
            MSG_MARKET_ERR_INVENTORY_FULL,
            MSG_TRADES_FAILED_BUYER, MSG_TRADES_FAILED_SELLER,
        ] {
            let (category, name) = resolve_category_and_key(key).unwrap();
            let template = defaults.get_message(key).unwrap();
            assert!(!template.contains("points refunded") || key == MSG_ORDERS_REFUNDED, "{key}");
            let allowed = match category {
                "orders" => placeholders.orders.get(name).unwrap(),
                "market_errors" => placeholders.market_errors.get(name).unwrap(),
                "trades" => placeholders.trades.get(name).unwrap(),
                _ => unreachable!(),
            };
            assert!(allowed.contains(&"buyer".to_string()), "{key}");
            assert!(allowed.contains(&"item".to_string()), "{key}");
        }
        assert!(placeholders.orders.get("unavailable").unwrap().contains(&"price".to_string()));
        let custom = serde_json::json!({"orders": {"waiting_viewer": "Custom {item}"}});
        let parsed: CategorizedChatMessages = serde_json::from_value(custom).unwrap();
        assert_eq!(parsed.get_message(MSG_ORDERS_WAITING_VIEWER), Some("Custom {item}"));
        let merged = CategorizedChatMessages::merge_with_overrides(
            &defaults,
            &HashMap::from([("trades".to_string(), HashMap::from([("failed_seller".to_string(), "Custom seller".to_string())]))]),
        );
        assert_eq!(merged.get_message(MSG_TRADES_FAILED_SELLER), Some("Custom seller"));
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
        // default filled for other active templates
        assert_eq!(from_flat.orders.waiting_viewer, CategorizedChatMessages::default().orders.waiting_viewer);

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
        orders_map.insert("failed_no_money_refund".to_string(), "Obsolete refund claim".to_string());
        custom.insert("orders".to_string(), orders_map);

        let merged = CategorizedChatMessages::merge_with_overrides(&defaults, &custom);
        assert_eq!(merged.orders.created, "Overridden order created");
        assert!(serde_json::to_value(&merged).unwrap()["orders"].get("failed_no_money_refund").is_none());
    }

    #[test]
    fn test_parse_custom_messages_both_formats() {
        let flat = serde_json::json!({
            "order_created": "My order",
            "trade_created": "My trade"
        });
        let parsed_flat = parse_custom_messages(flat);
        assert_eq!(parsed_flat.get("orders").unwrap().get("created").unwrap(), "My order");
        assert_eq!(parsed_flat.get("trades").unwrap().get("created").unwrap(), "My trade");

        let nested = serde_json::json!({
            "orders": {
                "waiting_viewer": "Custom wait"
            }
        });
        let parsed_nested = parse_custom_messages(nested);
        assert_eq!(parsed_nested.get("orders").unwrap().get("waiting_viewer").unwrap(), "Custom wait");
    }

    #[test]
    fn active_templates_exclude_removed_refund_and_retry_flags() {
        let defaults = CategorizedChatMessages::default();
        let visible = serde_json::to_value(&defaults).unwrap();
        assert!(visible.get("market_errors").is_some());
        for key in ["pool_created", "failed_no_money_refund", "failed_no_money_penalty", "failed_filter_exhausted", "retrying", "manual_hold", "steam_account_action", "retry_available"] {
            assert!(visible["orders"].get(key).is_none(), "{key}");
            assert!(defaults.get_message(&format!("orders.{key}")).is_none(), "{key}");
        }
        for key in ["failed_buyer_refund", "failed_buyer_penalty", "failed_seller_refund", "timeout"] {
            assert!(visible["trades"].get(key).is_none(), "{key}");
        }
        assert!(visible["chat_requirements"].get("messages_refund").is_some());
        assert!(visible["chat_requirements"].get("messages_penalty").is_some());
    }

    #[test]
    fn copied_old_acceptance_default_cannot_announce_final_delivery_at_settlement() {
        let defaults = CategorizedChatMessages::default();
        let overrides = HashMap::from([("trades".to_string(), HashMap::from([
            ("accepted".to_string(), "@{buyer} Trade offer accepted. Enjoy your skin!".to_string()),
        ]))]);
        let merged = CategorizedChatMessages::merge_with_overrides(&defaults, &overrides);
        assert_eq!(merged.trades.accepted, defaults.trades.accepted);
    }

    #[test]
    fn copied_legacy_market_defaults_do_not_claim_a_refund() {
        let old = "@{buyer} Your Steam inventory is private. Please set your inventory to public and try again. Channel points refunded.";
        let saved = serde_json::json!({"market_errors": {"inventory_hidden": old}});
        let parsed = parse_custom_messages(saved);
        assert!(parsed.get("market_errors").and_then(|m| m.get("inventory_hidden")).is_none());
        let merged = CategorizedChatMessages::merge_with_overrides(
            &CategorizedChatMessages::default(),
            &HashMap::from([("market_errors".to_string(), HashMap::from([("inventory_hidden".to_string(), old.to_string())]))]),
        );
        assert_eq!(merged.get_message(MSG_MARKET_ERR_INVENTORY_HIDDEN), Some(CategorizedChatMessages::default().market_errors.inventory_hidden.as_str()));
        let custom = "@{buyer} Please open your Steam inventory for {item}.";
        let merged_custom = CategorizedChatMessages::merge_with_overrides(
            &CategorizedChatMessages::default(),
            &HashMap::from([("market_errors".to_string(), HashMap::from([("inventory_hidden".to_string(), custom.to_string())]))]),
        );
        assert_eq!(merged_custom.get_message(MSG_MARKET_ERR_INVENTORY_HIDDEN), Some(custom));
    }

    #[test]
    fn copied_obsolete_order_defaults_yield_to_corrected_messages() {
        let old = "@{buyer} Market's status for {item} is being checked. Please do not start another order or refund until it is resolved; your points remain pending.";
        let parsed = parse_custom_messages(serde_json::json!({"orders": {"reconciliation_required": old}}));
        assert!(parsed.get("orders").and_then(|m| m.get("reconciliation_required")).is_none());
        let merged = CategorizedChatMessages::merge_with_overrides(
            &CategorizedChatMessages::default(),
            &HashMap::from([("orders".to_string(), HashMap::from([("reconciliation_required".to_string(), old.to_string())]))]),
        );
        assert!(merged.orders.reconciliation_required.contains("may still deliver"));
        let obsolete_price = "@{buyer} Market could not order {item} at its fixed price. Your points remain pending; you can retry later or request a refund from your inventory.";
        assert!(is_obsolete_copied_default("orders", "unavailable", obsolete_price));
        assert!(!is_obsolete_copied_default("orders", "unavailable", "Custom text {price}"));
    }
}
