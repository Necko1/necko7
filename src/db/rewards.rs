use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

pub const PAUSE_REASON_MANUAL: &str = "MANUAL";
pub const PAUSE_REASON_NO_MONEY: &str = "NO_MONEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PauseReason {
    /// Paused manually by the broadcaster or channel editor
    Manual,
    /// Automatically paused due to insufficient broadcaster balance
    NoMoney,
    /// Automatically paused due to market price exceeding configured min/max limits
    PriceLimit,
}

impl PauseReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::NoMoney => "NO_MONEY",
            Self::PriceLimit => "PRICE_LIMIT",
        }
    }
}

pub fn is_paused_due_to_price_limit(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some("PRICE_LIMIT" | "price_limit")
    )
}

pub fn is_paused_due_to_no_money(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some("NO_MONEY" | "INSUFFICIENT_FUNDS" | "INSUFFICIENT_BALANCE" | "no_money" | "insufficient_funds" | "insufficient_balance")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RewardType {
    Fixed,
    Pool,
    Filter,
}

impl Default for RewardType {
    fn default() -> Self {
        Self::Fixed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PricingMode {
    Auto,
    Manual,
}

impl Default for PricingMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceStrategy {
    Average,
    Median,
    Max,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct FilterConfig {
    pub min_price: f64,
    pub max_price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_volume: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct PoolItemConfig {
    pub market_hash_name: String,
    pub weight: f64,
    pub permissible_market_price_deviation: i32,
    #[serde(default)]
    pub current_market_price: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct Reward {
    pub twitch_id: Uuid,
    pub is_paused: bool,
    pub pause_reason: Option<PauseReason>,
    pub is_deleted: bool,
    pub streamer_id: String,
    pub reward_type: RewardType,
    pub pricing_mode: PricingMode,
    pub price_strategy: Option<PriceStrategy>,
    pub market_item_name: Option<String>,
    pub filter_config: Option<sqlx::types::Json<FilterConfig>>,
    pub pool_items: Option<sqlx::types::Json<Vec<PoolItemConfig>>>,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
    pub market_autobuy: bool,
    pub currency: String,
    pub min_market_price: Option<i32>,
    pub max_market_price: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewReward {
    pub twitch_id: Uuid,
    pub is_paused: bool,
    pub pause_reason: Option<PauseReason>,
    pub streamer_id: String,
    pub reward_type: RewardType,
    pub pricing_mode: PricingMode,
    pub price_strategy: Option<PriceStrategy>,
    pub market_item_name: Option<String>,
    pub filter_config: Option<sqlx::types::Json<FilterConfig>>,
    pub pool_items: Option<sqlx::types::Json<Vec<PoolItemConfig>>>,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
    pub market_autobuy: bool,
    pub currency: String,
    pub min_market_price: Option<i32>,
    pub max_market_price: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReward {
    pub is_paused: Option<bool>,
    pub pause_reason: Option<PauseReason>,
    pub is_deleted: Option<bool>,
    pub reward_type: Option<RewardType>,
    pub pricing_mode: Option<PricingMode>,
    pub price_strategy: Option<PriceStrategy>,
    pub market_item_name: Option<String>,
    pub filter_config: Option<sqlx::types::Json<FilterConfig>>,
    pub pool_items: Option<sqlx::types::Json<Vec<PoolItemConfig>>>,
    pub twitch_title: Option<String>,
    pub twitch_description: Option<String>,
    pub current_market_price: Option<i32>,
    pub permissible_market_price_deviation: Option<i32>,
    pub twitch_price_markup_percentage: Option<i16>,
    pub global_cooldown_seconds: Option<i32>,
    pub max_redemptions_per_stream: Option<i16>,
    pub max_redemptions_per_user_per_stream: Option<i16>,
    pub market_autobuy: Option<bool>,
    pub currency: Option<String>,
    pub min_market_price: Option<i32>,
    pub max_market_price: Option<i32>,
}

macro_rules! reward_select {
    ($tail:expr) => {
        concat!(
            "SELECT twitch_id, is_paused, pause_reason, is_deleted, streamer_id, reward_type, pricing_mode, price_strategy, market_item_name, filter_config, pool_items, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, market_autobuy, currency, min_market_price, max_market_price, created_at, updated_at FROM rewards ",
            $tail
        )
    };
}

macro_rules! reward_insert_returning {
    ($insert_stmt:expr) => {
        concat!(
            $insert_stmt,
            " RETURNING twitch_id, is_paused, pause_reason, is_deleted, streamer_id, reward_type, pricing_mode, price_strategy, market_item_name, filter_config, pool_items, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, market_autobuy, currency, min_market_price, max_market_price, created_at, updated_at"
        )
    };
}

impl Db {
    pub async fn get_reward_by_twitch_id(&self, twitch_id: Uuid) -> DbResult<Option<Reward>> {
        let reward = sqlx::query_as::<_, Reward>(reward_select!("WHERE twitch_id = $1"))
            .bind(twitch_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(reward)
    }

    pub async fn get_rewards_by_streamer_id(&self, streamer_id: &str) -> DbResult<Vec<Reward>> {
        let rewards = sqlx::query_as::<_, Reward>(reward_select!("WHERE streamer_id = $1"))
            .bind(streamer_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rewards)
    }

    pub async fn get_active_rewards_by_streamer_id(&self, streamer_id: &str) -> DbResult<Vec<Reward>> {
        let rewards = sqlx::query_as::<_, Reward>(reward_select!("WHERE streamer_id = $1 AND NOT is_deleted AND NOT is_paused"))
            .bind(streamer_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rewards)
    }

    pub async fn get_rewards_by_streamer_filtered(
        &self,
        streamer_id: &str,
        is_paused: Option<bool>,
        is_deleted: Option<bool>,
        pause_reason: Option<PauseReason>,
    ) -> DbResult<Vec<Reward>> {
        let rewards = sqlx::query_as::<_, Reward>(reward_select!(
            "WHERE streamer_id = $1 AND ($2::bool IS NULL OR is_paused = $2) AND ($3::bool IS NULL OR is_deleted = $3) AND ($4::varchar IS NULL OR pause_reason = $4)"
        ))
        .bind(streamer_id)
        .bind(is_paused)
        .bind(is_deleted)
        .bind(pause_reason)
        .fetch_all(&self.pool)
        .await?;
        Ok(rewards)
    }

    pub async fn create_reward(&self, new: &NewReward) -> DbResult<Reward> {
        let reward = sqlx::query_as::<_, Reward>(reward_insert_returning!(
            "INSERT INTO rewards (twitch_id, is_paused, pause_reason, is_deleted, streamer_id, reward_type, pricing_mode, price_strategy, market_item_name, filter_config, pool_items, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, market_autobuy, currency, min_market_price, max_market_price, created_at, updated_at) VALUES ($1, $2, $3, FALSE, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, NOW(), NOW())"
        ))
        .bind(new.twitch_id)
        .bind(new.is_paused)
        .bind(new.pause_reason)
        .bind(&new.streamer_id)
        .bind(new.reward_type)
        .bind(new.pricing_mode)
        .bind(new.price_strategy)
        .bind(&new.market_item_name)
        .bind(&new.filter_config)
        .bind(&new.pool_items)
        .bind(&new.twitch_title)
        .bind(&new.twitch_description)
        .bind(new.current_market_price)
        .bind(new.permissible_market_price_deviation)
        .bind(new.twitch_price_markup_percentage)
        .bind(new.global_cooldown_seconds)
        .bind(new.max_redemptions_per_stream)
        .bind(new.max_redemptions_per_user_per_stream)
        .bind(new.market_autobuy)
        .bind(&new.currency)
        .bind(new.min_market_price)
        .bind(new.max_market_price)
        .fetch_one(&self.pool)
        .await?;
        Ok(reward)
    }

    pub async fn upsert_reward(&self, new: &NewReward) -> DbResult<Reward> {
        let reward = sqlx::query_as::<_, Reward>(reward_insert_returning!(
            "INSERT INTO rewards (twitch_id, is_paused, pause_reason, is_deleted, streamer_id, reward_type, pricing_mode, price_strategy, market_item_name, filter_config, pool_items, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, market_autobuy, currency, min_market_price, max_market_price, created_at, updated_at) VALUES ($1, $2, $3, FALSE, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, NOW(), NOW()) ON CONFLICT (twitch_id) DO UPDATE SET is_paused = EXCLUDED.is_paused, pause_reason = EXCLUDED.pause_reason, is_deleted = FALSE, streamer_id = EXCLUDED.streamer_id, reward_type = EXCLUDED.reward_type, pricing_mode = EXCLUDED.pricing_mode, price_strategy = EXCLUDED.price_strategy, market_item_name = EXCLUDED.market_item_name, filter_config = EXCLUDED.filter_config, pool_items = EXCLUDED.pool_items, twitch_title = EXCLUDED.twitch_title, twitch_description = EXCLUDED.twitch_description, current_market_price = EXCLUDED.current_market_price, permissible_market_price_deviation = EXCLUDED.permissible_market_price_deviation, twitch_price_markup_percentage = EXCLUDED.twitch_price_markup_percentage, global_cooldown_seconds = EXCLUDED.global_cooldown_seconds, max_redemptions_per_stream = EXCLUDED.max_redemptions_per_stream, max_redemptions_per_user_per_stream = EXCLUDED.max_redemptions_per_user_per_stream, market_autobuy = EXCLUDED.market_autobuy, currency = EXCLUDED.currency, min_market_price = EXCLUDED.min_market_price, max_market_price = EXCLUDED.max_market_price, updated_at = NOW()"
        ))
            .bind(new.twitch_id)
            .bind(new.is_paused)
            .bind(new.pause_reason)
            .bind(&new.streamer_id)
            .bind(new.reward_type)
            .bind(new.pricing_mode)
            .bind(new.price_strategy)
            .bind(&new.market_item_name)
            .bind(&new.filter_config)
            .bind(&new.pool_items)
            .bind(&new.twitch_title)
            .bind(&new.twitch_description)
            .bind(new.current_market_price)
            .bind(new.permissible_market_price_deviation)
            .bind(new.twitch_price_markup_percentage)
            .bind(new.global_cooldown_seconds)
            .bind(new.max_redemptions_per_stream)
            .bind(new.max_redemptions_per_user_per_stream)
            .bind(new.market_autobuy)
            .bind(&new.currency)
            .bind(new.min_market_price)
            .bind(new.max_market_price)
            .fetch_one(&self.pool)
            .await?;
        Ok(reward)
    }

    pub async fn update_reward(&self, twitch_id: Uuid, patch: &UpdateReward) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET is_paused = COALESCE($2, is_paused), pause_reason = CASE WHEN $2 = FALSE THEN NULL WHEN $15::varchar IS NOT NULL THEN $15 ELSE pause_reason END, is_deleted = COALESCE($3, is_deleted), market_item_name = COALESCE($4, market_item_name), twitch_title = COALESCE($5, twitch_title), twitch_description = COALESCE($6, twitch_description), current_market_price = COALESCE($7, current_market_price), permissible_market_price_deviation = COALESCE($8, permissible_market_price_deviation), twitch_price_markup_percentage = COALESCE($9, twitch_price_markup_percentage), global_cooldown_seconds = COALESCE($10, global_cooldown_seconds), max_redemptions_per_stream = COALESCE($11, max_redemptions_per_stream), max_redemptions_per_user_per_stream = COALESCE($12, max_redemptions_per_user_per_stream), market_autobuy = COALESCE($13, market_autobuy), currency = COALESCE($14, currency), reward_type = COALESCE($16, reward_type), pricing_mode = COALESCE($17, pricing_mode), price_strategy = COALESCE($18, price_strategy), filter_config = COALESCE($19, filter_config), pool_items = COALESCE($20, pool_items), min_market_price = COALESCE($21, min_market_price), max_market_price = COALESCE($22, max_market_price), updated_at = NOW() WHERE twitch_id = $1"
        )
        .bind(twitch_id)
        .bind(patch.is_paused)
        .bind(patch.is_deleted)
        .bind(&patch.market_item_name)
        .bind(&patch.twitch_title)
        .bind(&patch.twitch_description)
        .bind(patch.current_market_price)
        .bind(patch.permissible_market_price_deviation)
        .bind(patch.twitch_price_markup_percentage)
        .bind(patch.global_cooldown_seconds)
        .bind(patch.max_redemptions_per_stream)
        .bind(patch.max_redemptions_per_user_per_stream)
        .bind(patch.market_autobuy)
        .bind(&patch.currency)
        .bind(patch.pause_reason)
        .bind(patch.reward_type)
        .bind(patch.pricing_mode)
        .bind(patch.price_strategy)
        .bind(&patch.filter_config)
        .bind(&patch.pool_items)
        .bind(patch.min_market_price)
        .bind(patch.max_market_price)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_reward_paused(
        &self,
        twitch_id: Uuid,
        is_paused: bool,
        pause_reason: Option<PauseReason>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET is_paused = $1, pause_reason = $2, updated_at = NOW() WHERE twitch_id = $3"
        )
        .bind(is_paused)
        .bind(pause_reason)
        .bind(twitch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_reward_deleted(&self, twitch_id: Uuid) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET is_deleted = TRUE, updated_at = NOW() WHERE twitch_id = $1"
        )
        .bind(twitch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_reward_market_price(&self, twitch_id: Uuid, price: i32) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET current_market_price = $1, updated_at = NOW() WHERE twitch_id = $2"
        )
        .bind(price)
        .bind(twitch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_reward(&self, twitch_id: Uuid) -> DbResult<()> {
        sqlx::query("DELETE FROM rewards WHERE twitch_id = $1")
            .bind(twitch_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

