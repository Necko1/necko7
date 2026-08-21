use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, sqlx::Type, PartialEq)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RedemptionStatus {
    Pending,
    OrderCreated,
    FailedRefund,
    FailedPenalty,
    Completed,
}

#[derive(Debug, Clone, FromRow)]
pub struct Redemption {
    pub twitch_redemption_id: Uuid,
    pub twitch_reward_id: Uuid,
    pub user_id: String,
    pub user_login: String,
    pub user_trade_link: String,
    pub twitch_points_cost: i64,
    pub market_paid_price: Option<i64>,
    pub status: RedemptionStatus,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewRedemption {
    pub twitch_redemption_id: Uuid,
    pub twitch_reward_id: Uuid,
    pub user_id: String,
    pub user_login: String,
    pub user_trade_link: String,
    pub twitch_points_cost: i64,
    pub status: RedemptionStatus,
}

impl Db {
    pub async fn get_redemption(&self, twitch_redemption_id: Uuid) -> DbResult<Option<Redemption>> {
        let redemption = sqlx::query_as::<_, Redemption>(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at FROM redemptions WHERE twitch_redemption_id = $1"
        )
        .bind(twitch_redemption_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(redemption)
    }

    pub async fn get_redemptions_by_reward(&self, twitch_reward_id: Uuid) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at FROM redemptions WHERE twitch_reward_id = $1"
        )
        .bind(twitch_reward_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(redemptions)
    }

    pub async fn get_redemptions_by_user(&self, user_id: &str) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at FROM redemptions WHERE user_id = $1"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(redemptions)
    }

    pub async fn get_pending_redemptions(&self) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at FROM redemptions WHERE status = 'PENDING'"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(redemptions)
    }

    pub async fn get_pending_redemptions_by_reward(&self, twitch_reward_id: Uuid) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at FROM redemptions WHERE twitch_reward_id = $1 AND status = 'PENDING'"
        )
        .bind(twitch_reward_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(redemptions)
    }

    pub async fn create_redemption(&self, new: &NewRedemption) -> DbResult<Redemption> {
        let redemption = sqlx::query_as::<_, Redemption>(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, NULL, NULL, NOW(), NOW()) RETURNING twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at"
        )
        .bind(new.twitch_redemption_id)
        .bind(new.twitch_reward_id)
        .bind(&new.user_id)
        .bind(&new.user_login)
        .bind(&new.user_trade_link)
        .bind(new.twitch_points_cost)
        .bind(&new.status)
        .fetch_one(&self.pool)
        .await?;
        Ok(redemption)
    }

    pub async fn upsert_redemption(&self, new: &NewRedemption) -> DbResult<Redemption> {
        let redemption = sqlx::query_as::<_, Redemption>(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, NULL, NULL, NOW(), NOW()) ON CONFLICT (twitch_redemption_id) DO UPDATE SET twitch_reward_id = EXCLUDED.twitch_reward_id, user_id = EXCLUDED.user_id, user_login = EXCLUDED.user_login, user_trade_link = EXCLUDED.user_trade_link, twitch_points_cost = EXCLUDED.twitch_points_cost, status = EXCLUDED.status, updated_at = NOW() RETURNING twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, status, fail_cause, fail_description, created_at, updated_at"
        )
        .bind(new.twitch_redemption_id)
        .bind(new.twitch_reward_id)
        .bind(&new.user_id)
        .bind(&new.user_login)
        .bind(&new.user_trade_link)
        .bind(new.twitch_points_cost)
        .bind(&new.status)
        .fetch_one(&self.pool)
        .await?;
        Ok(redemption)
    }

    pub async fn update_redemption_status(
        &self,
        twitch_redemption_id: Uuid,
        status: RedemptionStatus,
        fail_cause: Option<&str>,
        fail_description: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = $1, fail_cause = $2, fail_description = $3, updated_at = NOW() WHERE twitch_redemption_id = $4"
        )
        .bind(&status)
        .bind(fail_cause)
        .bind(fail_description)
        .bind(twitch_redemption_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_redemption_order_created(&self, twitch_redemption_id: Uuid, market_paid_price: i64) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = 'ORDER_CREATED', market_paid_price = $1, updated_at = NOW() WHERE twitch_redemption_id = $2"
        )
        .bind(market_paid_price)
        .bind(twitch_redemption_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_redemption_completed(&self, twitch_redemption_id: Uuid) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = 'COMPLETED', updated_at = NOW() WHERE twitch_redemption_id = $1"
        )
        .bind(twitch_redemption_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_redemption_failed(
        &self,
        twitch_redemption_id: Uuid,
        fail_cause: &str,
        fail_description: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = 'FAILED_REFUND', fail_cause = $1, fail_description = $2, updated_at = NOW() WHERE twitch_redemption_id = $3"
        )
        .bind(fail_cause)
        .bind(fail_description)
        .bind(twitch_redemption_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_redemption(&self, twitch_redemption_id: Uuid) -> DbResult<()> {
        sqlx::query("DELETE FROM redemptions WHERE twitch_redemption_id = $1")
            .bind(twitch_redemption_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
