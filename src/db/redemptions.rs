use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, Copy, sqlx::Type, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
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
    pub currency: String,
    pub status: RedemptionStatus,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
    pub retry_count: i32,
    pub market_item_name: Option<String>,
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
    pub currency: String,
    pub status: RedemptionStatus,
    pub market_item_name: Option<String>,
}

macro_rules! redemption_select {
    ($tail:expr) => {
        concat!(
            "SELECT twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, created_at, updated_at FROM redemptions ",
            $tail
        )
    };
}

macro_rules! redemption_insert_returning {
    ($insert_stmt:expr) => {
        concat!(
            $insert_stmt,
            " RETURNING twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, created_at, updated_at"
        )
    };
}

impl Db {
    pub async fn get_redemption(&self, twitch_redemption_id: Uuid) -> DbResult<Option<Redemption>> {
        let redemption = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE twitch_redemption_id = $1"))
            .bind(twitch_redemption_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(redemption)
    }

    pub async fn get_redemptions_by_reward(&self, twitch_reward_id: Uuid) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE twitch_reward_id = $1"))
            .bind(twitch_reward_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(redemptions)
    }

    pub async fn get_redemptions_by_user(&self, user_id: &str) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE user_id = $1"))
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(redemptions)
    }

    pub async fn get_pending_redemptions(&self) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE status = 'PENDING'"))
            .fetch_all(&self.pool)
            .await?;
        Ok(redemptions)
    }

    pub async fn get_pending_redemptions_by_reward(&self, twitch_reward_id: Uuid) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE twitch_reward_id = $1 AND status = 'PENDING'"))
            .bind(twitch_reward_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(redemptions)
    }

    pub async fn get_active_orders(&self) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(redemption_select!("WHERE status = 'ORDER_CREATED'"))
            .fetch_all(&self.pool)
            .await?;
        Ok(redemptions)
    }

    pub async fn create_redemption(&self, new: &NewRedemption) -> DbResult<Redemption> {
        let redemption = sqlx::query_as::<_, Redemption>(redemption_insert_returning!(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, NULL, NULL, 0, $9, NOW(), NOW())"
        ))
        .bind(new.twitch_redemption_id)
        .bind(new.twitch_reward_id)
        .bind(&new.user_id)
        .bind(&new.user_login)
        .bind(&new.user_trade_link)
        .bind(new.twitch_points_cost)
        .bind(&new.currency)
        .bind(&new.status)
        .bind(&new.market_item_name)
        .fetch_one(&self.pool)
        .await?;
        Ok(redemption)
    }

    pub async fn insert_redemption_if_new(&self, new: &NewRedemption) -> DbResult<Option<Redemption>> {
        let redemption = sqlx::query_as::<_, Redemption>(redemption_insert_returning!(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, NULL, NULL, 0, $9, NOW(), NOW()) ON CONFLICT (twitch_redemption_id) DO NOTHING"
        ))
        .bind(new.twitch_redemption_id)
        .bind(new.twitch_reward_id)
        .bind(&new.user_id)
        .bind(&new.user_login)
        .bind(&new.user_trade_link)
        .bind(new.twitch_points_cost)
        .bind(&new.currency)
        .bind(&new.status)
        .bind(&new.market_item_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(redemption)
    }

    pub async fn upsert_redemption(&self, new: &NewRedemption) -> DbResult<Redemption> {
        let redemption = sqlx::query_as::<_, Redemption>(redemption_insert_returning!(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, NULL, NULL, 0, $9, NOW(), NOW()) ON CONFLICT (twitch_redemption_id) DO UPDATE SET twitch_reward_id = EXCLUDED.twitch_reward_id, user_id = EXCLUDED.user_id, user_login = EXCLUDED.user_login, user_trade_link = EXCLUDED.user_trade_link, twitch_points_cost = EXCLUDED.twitch_points_cost, currency = EXCLUDED.currency, status = EXCLUDED.status, market_item_name = COALESCE(EXCLUDED.market_item_name, redemptions.market_item_name), updated_at = NOW()"
        ))
        .bind(new.twitch_redemption_id)
        .bind(new.twitch_reward_id)
        .bind(&new.user_id)
        .bind(&new.user_login)
        .bind(&new.user_trade_link)
        .bind(new.twitch_points_cost)
        .bind(&new.currency)
        .bind(&new.status)
        .bind(&new.market_item_name)
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

    pub async fn set_redemption_order_created(
        &self,
        twitch_redemption_id: Uuid,
        market_paid_price: i64,
        market_item_name: Option<&str>,
        retry_count: i32,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = 'ORDER_CREATED', market_paid_price = $1, market_item_name = COALESCE($2, market_item_name), retry_count = $3, updated_at = NOW() WHERE twitch_redemption_id = $4"
        )
        .bind(market_paid_price)
        .bind(market_item_name)
        .bind(retry_count)
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

    pub async fn get_redemptions_by_broadcaster(
        &self,
        broadcaster_id: &str,
        status_filter: Option<&str>,
        reward_id_filter: Option<Uuid>,
        user_id_filter: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> DbResult<Vec<Redemption>> {
        let redemptions = sqlx::query_as::<_, Redemption>(
            "SELECT r.twitch_redemption_id, r.twitch_reward_id, r.user_id, r.user_login, r.user_trade_link, r.twitch_points_cost, r.market_paid_price, r.currency, r.status, r.fail_cause, r.fail_description, r.retry_count, r.market_item_name, r.created_at, r.updated_at
             FROM redemptions r
             INNER JOIN rewards rw ON r.twitch_reward_id = rw.twitch_id
             WHERE rw.streamer_id = $1
             AND ($2::VARCHAR IS NULL OR UPPER(r.status) = UPPER($2))
             AND ($3::UUID IS NULL OR r.twitch_reward_id = $3)
             AND ($4::VARCHAR IS NULL OR r.user_id = $4)
             ORDER BY r.created_at DESC
             OFFSET $5 LIMIT $6"
        )
        .bind(broadcaster_id)
        .bind(status_filter)
        .bind(reward_id_filter)
        .bind(user_id_filter)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(redemptions)
    }

    pub async fn count_redemptions_by_broadcaster(
        &self,
        broadcaster_id: &str,
        status_filter: Option<&str>,
        reward_id_filter: Option<Uuid>,
        user_id_filter: Option<&str>,
    ) -> DbResult<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT
             FROM redemptions r
             INNER JOIN rewards rw ON r.twitch_reward_id = rw.twitch_id
             WHERE rw.streamer_id = $1
             AND ($2::VARCHAR IS NULL OR UPPER(r.status) = UPPER($2))
             AND ($3::UUID IS NULL OR r.twitch_reward_id = $3)
             AND ($4::VARCHAR IS NULL OR r.user_id = $4)"
        )
        .bind(broadcaster_id)
        .bind(status_filter)
        .bind(reward_id_filter)
        .bind(user_id_filter)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn get_redemption_stats(
        &self,
        broadcaster_id: &str,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> DbResult<RedemptionStats> {
        let stats = sqlx::query_as::<_, RedemptionStats>(
            "SELECT
                COUNT(*)::BIGINT AS total_redemptions,
                COUNT(*) FILTER (WHERE r.status = 'COMPLETED')::BIGINT AS completed,
                COUNT(*) FILTER (WHERE r.status IN ('FAILED_REFUND', 'FAILED_PENALTY'))::BIGINT AS failed,
                COALESCE(SUM(r.market_paid_price) FILTER (WHERE r.status = 'COMPLETED'), 0)::BIGINT AS total_spent,
                COALESCE(SUM(r.twitch_points_cost) FILTER (WHERE r.status = 'COMPLETED'), 0)::BIGINT AS total_points_earned
             FROM redemptions r
             INNER JOIN rewards rw ON r.twitch_reward_id = rw.twitch_id
             WHERE rw.streamer_id = $1
             AND r.created_at >= $2
             AND r.created_at < $3"
        )
        .bind(broadcaster_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(stats)
    }

    pub async fn increment_retry_count(&self, twitch_redemption_id: Uuid) -> DbResult<i32> {
        let new_count = sqlx::query_scalar::<_, i32>(
            "UPDATE redemptions SET retry_count = retry_count + 1, updated_at = NOW() WHERE twitch_redemption_id = $1 RETURNING retry_count"
        )
        .bind(twitch_redemption_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(new_count)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RedemptionStats {
    pub total_redemptions: i64,
    pub completed: i64,
    pub failed: i64,
    pub total_spent: i64,
    pub total_points_earned: i64,
}
