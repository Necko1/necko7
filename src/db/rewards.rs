use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct Reward {
    pub twitch_id: Uuid,
    pub is_paused: bool,
    pub is_deleted: bool,
    pub streamer_id: String,
    pub market_item_name: String,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewReward {
    pub twitch_id: Uuid,
    pub is_paused: bool,
    pub streamer_id: String,
    pub market_item_name: String,
    pub twitch_title: String,
    pub twitch_description: String,
    pub current_market_price: i32,
    pub permissible_market_price_deviation: i32,
    pub twitch_price_markup_percentage: i16,
    pub global_cooldown_seconds: i32,
    pub max_redemptions_per_stream: i16,
    pub max_redemptions_per_user_per_stream: i16,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReward {
    pub is_paused: Option<bool>,
    pub is_deleted: Option<bool>,
    pub market_item_name: Option<String>,
    pub twitch_title: Option<String>,
    pub twitch_description: Option<String>,
    pub current_market_price: Option<i32>,
    pub permissible_market_price_deviation: Option<i32>,
    pub twitch_price_markup_percentage: Option<i16>,
    pub global_cooldown_seconds: Option<i32>,
    pub max_redemptions_per_stream: Option<i16>,
    pub max_redemptions_per_user_per_stream: Option<i16>,
}

impl Db {
    pub async fn get_reward_by_twitch_id(&self, twitch_id: Uuid) -> DbResult<Option<Reward>> {
        let reward = sqlx::query_as::<_, Reward>(
            "SELECT twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at FROM rewards WHERE twitch_id = $1"
        )
        .bind(twitch_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(reward)
    }

    pub async fn get_rewards_by_streamer_id(&self, streamer_id: &str) -> DbResult<Vec<Reward>> {
        let rewards = sqlx::query_as::<_, Reward>(
            "SELECT twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at FROM rewards WHERE streamer_id = $1"
        )
        .bind(streamer_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rewards)
    }

    pub async fn get_active_rewards_by_streamer_id(&self, streamer_id: &str) -> DbResult<Vec<Reward>> {
        let rewards = sqlx::query_as::<_, Reward>(
            "SELECT twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at FROM rewards WHERE streamer_id = $1 AND NOT is_deleted AND NOT is_paused"
        )
        .bind(streamer_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rewards)
    }

    pub async fn create_reward(&self, new: &NewReward) -> DbResult<Reward> {
        let reward = sqlx::query_as::<_, Reward>(
            "INSERT INTO rewards (twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at) VALUES ($1, $2, FALSE, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), NOW()) RETURNING twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at"
        )
        .bind(new.twitch_id)
        .bind(new.is_paused)
        .bind(&new.streamer_id)
        .bind(&new.market_item_name)
        .bind(&new.twitch_title)
        .bind(&new.twitch_description)
        .bind(new.current_market_price)
        .bind(new.permissible_market_price_deviation)
        .bind(new.twitch_price_markup_percentage)
        .bind(new.global_cooldown_seconds)
        .bind(new.max_redemptions_per_stream)
        .bind(new.max_redemptions_per_user_per_stream)
        .fetch_one(&self.pool)
        .await?;
        Ok(reward)
    }

    pub async fn upsert_reward(&self, new: &NewReward) -> DbResult<Reward> {
        let reward = sqlx::query_as::<_, Reward>(
            "INSERT INTO rewards (twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at) VALUES ($1, $2, FALSE, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), NOW()) ON CONFLICT (twitch_id) DO UPDATE SET is_paused = EXCLUDED.is_paused, is_deleted = FALSE, streamer_id = EXCLUDED.streamer_id, market_item_name = EXCLUDED.market_item_name, twitch_title = EXCLUDED.twitch_title, twitch_description = EXCLUDED.twitch_description, current_market_price = EXCLUDED.current_market_price, permissible_market_price_deviation = EXCLUDED.permissible_market_price_deviation, twitch_price_markup_percentage = EXCLUDED.twitch_price_markup_percentage, global_cooldown_seconds = EXCLUDED.global_cooldown_seconds, max_redemptions_per_stream = EXCLUDED.max_redemptions_per_stream, max_redemptions_per_user_per_stream = EXCLUDED.max_redemptions_per_user_per_stream, updated_at = NOW() RETURNING twitch_id, is_paused, is_deleted, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at"
        )
        .bind(new.twitch_id)
        .bind(new.is_paused)
        .bind(&new.streamer_id)
        .bind(&new.market_item_name)
        .bind(&new.twitch_title)
        .bind(&new.twitch_description)
        .bind(new.current_market_price)
        .bind(new.permissible_market_price_deviation)
        .bind(new.twitch_price_markup_percentage)
        .bind(new.global_cooldown_seconds)
        .bind(new.max_redemptions_per_stream)
        .bind(new.max_redemptions_per_user_per_stream)
        .fetch_one(&self.pool)
        .await?;
        Ok(reward)
    }

    pub async fn update_reward(&self, twitch_id: Uuid, patch: &UpdateReward) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET is_paused = COALESCE($2, is_paused), is_deleted = COALESCE($3, is_deleted), market_item_name = COALESCE($4, market_item_name), twitch_title = COALESCE($5, twitch_title), twitch_description = COALESCE($6, twitch_description), current_market_price = COALESCE($7, current_market_price), permissible_market_price_deviation = COALESCE($8, permissible_market_price_deviation), twitch_price_markup_percentage = COALESCE($9, twitch_price_markup_percentage), global_cooldown_seconds = COALESCE($10, global_cooldown_seconds), max_redemptions_per_stream = COALESCE($11, max_redemptions_per_stream), max_redemptions_per_user_per_stream = COALESCE($12, max_redemptions_per_user_per_stream), updated_at = NOW() WHERE twitch_id = $1"
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_reward_paused(&self, twitch_id: Uuid, is_paused: bool) -> DbResult<()> {
        sqlx::query(
            "UPDATE rewards SET is_paused = $1, updated_at = NOW() WHERE twitch_id = $2"
        )
        .bind(is_paused)
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
