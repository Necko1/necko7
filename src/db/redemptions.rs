use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use crate::db::rewards::RewardPurchaseLimitsConfig;
use super::Db;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PurchaseLimitDecision {
    Admitted,
    GlobalRejected { count: i64, max_redemptions: i32, window_hours: Option<i32> },
    UserRejected { count: i64, max_redemptions: i32, window_hours: Option<i32> },
}

pub(crate) async fn count_reward_redemptions_on_connection(
    connection: &mut sqlx::PgConnection, reward_id: Uuid, user_id: Option<&str>,
    window_hours: Option<i32>, exclude: Option<Uuid>,
) -> DbResult<i64> {
    let since = window_hours.map(|h| Utc::now() - chrono::Duration::hours(h as i64));
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM redemptions r
         LEFT JOIN inventory_items i ON i.redemption_id = r.twitch_redemption_id
         WHERE r.twitch_reward_id = $1 AND ($2::VARCHAR IS NULL OR r.user_id = $2)
           AND (($2::VARCHAR IS NOT NULL AND $3::TIMESTAMPTZ IS NOT NULL AND i.id IS NOT NULL)
                OR (r.purchase_limit_decision->>'kind' = 'admitted'
                    AND r.status IN ('COMPLETED','ORDER_CREATED','PENDING','MANUAL_HOLD')))
           AND ($3::TIMESTAMPTZ IS NULL OR r.created_at >= $3)
           AND ($4::UUID IS NULL OR r.twitch_redemption_id != $4)"
    ).bind(reward_id).bind(user_id).bind(since).bind(exclude).fetch_one(connection).await?)
}

#[derive(Debug, Clone, Copy, sqlx::Type, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RedemptionStatus {
    Pending,
    OrderCreated,
    FailedRefund,
    FailedPenalty,
    Completed,
    ManualHold,
}

impl RedemptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::OrderCreated => "ORDER_CREATED",
            Self::FailedRefund => "FAILED_REFUND",
            Self::FailedPenalty => "FAILED_PENALTY",
            Self::Completed => "COMPLETED",
            Self::ManualHold => "MANUAL_HOLD",
        }
    }
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
    /// Serializes admissions for one reward. The count and insertion commit as one
    /// transaction, so simultaneous EventSub deliveries cannot all claim the last slot.
    pub async fn insert_redemption_with_limits(
        &self,
        new: &NewRedemption,
        redeemed_at: DateTime<Utc>,
    ) -> DbResult<Option<(Redemption, PurchaseLimitDecision)>> {
        let mut tx = self.pool.begin().await?;
        let limits: Option<sqlx::types::Json<RewardPurchaseLimitsConfig>> = sqlx::query_scalar(
            "SELECT purchase_limits FROM rewards WHERE twitch_id = $1 FOR UPDATE"
        ).bind(new.twitch_reward_id).fetch_one(&mut *tx).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM redemptions WHERE twitch_redemption_id = $1)"
        ).bind(new.twitch_redemption_id).fetch_one(&mut *tx).await?;
        if exists {
            tx.commit().await?;
            return Ok(None);
        }

        let mut decision = PurchaseLimitDecision::Admitted;
        if let Some(limits) = limits.as_ref().map(|j| &j.0) {
            for rule in &limits.global {
                let count = count_reward_redemptions_on_connection(&mut tx,
                    new.twitch_reward_id, None, rule.window_hours, None).await?;
                if count >= rule.max_redemptions as i64 {
                    decision = PurchaseLimitDecision::GlobalRejected {
                        count, max_redemptions: rule.max_redemptions, window_hours: rule.window_hours,
                    };
                    break;
                }
            }
            if decision == PurchaseLimitDecision::Admitted {
                for rule in &limits.user {
                    let count = count_reward_redemptions_on_connection(&mut tx,
                        new.twitch_reward_id, Some(&new.user_id), rule.window_hours, None).await?;
                    if count >= rule.max_redemptions as i64 {
                        decision = PurchaseLimitDecision::UserRejected {
                            count, max_redemptions: rule.max_redemptions, window_hours: rule.window_hours,
                        };
                        break;
                    }
                }
            }
        }

        let redemption = sqlx::query_as::<_, Redemption>(redemption_insert_returning!(
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, purchase_limit_decision, inventory_resolution_claimed_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, NULL, NULL, 0, $9, $10, NOW(), $11, NOW()) ON CONFLICT (twitch_redemption_id) DO NOTHING"
        ))
        .bind(new.twitch_redemption_id).bind(new.twitch_reward_id)
        .bind(&new.user_id).bind(&new.user_login).bind(&new.user_trade_link)
        .bind(new.twitch_points_cost).bind(&new.currency).bind(&new.status)
        .bind(&new.market_item_name).bind(sqlx::types::Json(&decision)).bind(redeemed_at)
        .fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        Ok(redemption.map(|row| (row, decision)))
    }

    pub async fn get_redemption_purchase_limit_decision(
        &self, redemption_id: Uuid,
    ) -> DbResult<PurchaseLimitDecision> {
        let decision: sqlx::types::Json<PurchaseLimitDecision> = sqlx::query_scalar(
            "SELECT purchase_limit_decision FROM redemptions WHERE twitch_redemption_id = $1"
        ).bind(redemption_id).fetch_one(&self.pool).await?;
        Ok(decision.0)
    }

    /// Only redemptions inserted by the inventory-era EventSub path are claimed.
    /// A stale claim can be resumed after a worker exits before item creation.
    pub async fn claim_pending_inventory_resolution(&self) -> DbResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar(
            "UPDATE redemptions r SET inventory_resolution_claimed_at = NOW()
             WHERE r.twitch_redemption_id IN (
                 SELECT pending.twitch_redemption_id FROM redemptions pending
                 WHERE pending.status = 'PENDING'
                   AND pending.inventory_resolution_claimed_at < NOW() - INTERVAL '5 minutes'
                   AND NOT EXISTS (SELECT 1 FROM inventory_items i WHERE i.redemption_id = pending.twitch_redemption_id)
                 ORDER BY pending.inventory_resolution_claimed_at LIMIT 100 FOR UPDATE SKIP LOCKED
             ) RETURNING r.twitch_redemption_id"
        ).fetch_all(&self.pool).await?)
    }
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
            "INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, market_paid_price, currency, status, fail_cause, fail_description, retry_count, market_item_name, inventory_resolution_claimed_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, NULL, NULL, 0, $9, NOW(), NOW(), NOW()) ON CONFLICT (twitch_redemption_id) DO NOTHING"
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
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE redemptions SET status = $1, fail_cause = $2, fail_description = $3, updated_at = NOW() WHERE twitch_redemption_id = $4"
        )
        .bind(&status)
        .bind(fail_cause)
        .bind(fail_description)
        .bind(twitch_redemption_id)
        .execute(&mut *tx)
        .await?;
        if status == RedemptionStatus::Completed {
            sqlx::query("UPDATE inventory_items SET acquired_at = COALESCE(acquired_at, NOW()) WHERE redemption_id = $1")
                .bind(twitch_redemption_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
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
            "UPDATE redemptions SET status = 'ORDER_CREATED', market_paid_price = $1, market_item_name = COALESCE($2, market_item_name), retry_count = $3, fail_cause = NULL, fail_description = NULL, updated_at = NOW()
             WHERE twitch_redemption_id = $4 AND status IN ('PENDING','ORDER_CREATED')
               AND EXISTS (SELECT 1 FROM inventory_items i WHERE i.redemption_id = $4 AND i.lifecycle_status IN ('ORDER_PENDING','TRADE_WAITING','TRADE_ACCEPTED'))"
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
            "UPDATE redemptions SET status = 'COMPLETED', fail_cause = NULL, fail_description = NULL, updated_at = NOW() WHERE twitch_redemption_id = $1"
        )
        .bind(twitch_redemption_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE inventory_items SET acquired_at = COALESCE(acquired_at, NOW()) WHERE redemption_id = $1")
            .bind(twitch_redemption_id).execute(&self.pool).await?;
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

    pub async fn set_redemption_manual_hold(
        &self,
        twitch_redemption_id: Uuid,
        fail_cause: &str,
        fail_description: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE redemptions SET status = 'MANUAL_HOLD', fail_cause = $1, fail_description = $2, updated_at = NOW() WHERE twitch_redemption_id = $3"
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

    /// Count admitted, successful or in-progress redemptions for a reward.
    /// PENDING, ORDER_CREATED, MANUAL_HOLD and COMPLETED occupy a slot;
    /// Rolling per-user rules also count every created inventory item, even after
    /// refund/failure, within the original redemption-time window. Pre-inventory pending rows reserve
    /// capacity until eligibility fails or the reservation expires. Global/lifetime
    /// rules retain the status-based counting semantics.
    /// - If `user_id` is Some, counts only for that user. If None, counts globally.
    /// - If `window_hours` is Some, counts within the rolling window (`created_at >= NOW() - window_hours`).
    ///   If None, counts across all-time.
    /// - If `exclude_redemption_id` is Some, excludes that specific redemption.
    pub async fn count_reward_redemptions(
        &self,
        reward_id: Uuid,
        user_id: Option<&str>,
        window_hours: Option<i32>,
        exclude_redemption_id: Option<Uuid>,
    ) -> DbResult<i64> {
        let mut connection = self.pool.acquire().await?;
        count_reward_redemptions_on_connection(&mut connection, reward_id, user_id,
            window_hours, exclude_redemption_id).await
    }

    pub async fn get_viewer_redemptions_on_channel(
        &self,
        channel_id: &str,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<ViewerChannelRedemption>> {
        let redemptions = sqlx::query_as::<_, ViewerChannelRedemption>(
            "SELECT
                r.twitch_redemption_id,
                r.twitch_reward_id,
                rew.twitch_title AS reward_title,
                r.twitch_points_cost,
                r.market_paid_price,
                r.currency,
                r.status,
                i.lifecycle_status AS inventory_lifecycle_status,
                a.status AS latest_attempt_status,
                a.outcome_kind AS latest_attempt_outcome_kind,
                r.fail_cause,
                r.fail_description,
                r.market_item_name,
                r.created_at,
                r.updated_at
             FROM redemptions r
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             LEFT JOIN inventory_items i ON i.redemption_id = r.twitch_redemption_id
             LEFT JOIN LATERAL (SELECT status, outcome_kind FROM inventory_order_attempts
                                WHERE inventory_id = i.id ORDER BY attempt_id DESC LIMIT 1) a ON TRUE
             WHERE rew.streamer_id = $1 AND r.user_id = $2
             ORDER BY r.created_at DESC
             LIMIT $3 OFFSET $4"
        )
        .bind(channel_id)
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(redemptions)
    }

    pub async fn get_viewer_redemptions_global(
        &self,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<ViewerGlobalRedemption>> {
        let redemptions = sqlx::query_as::<_, ViewerGlobalRedemption>(
            "SELECT
                r.twitch_redemption_id,
                r.twitch_reward_id,
                rew.streamer_id AS channel_id,
                b.channel_login AS channel_login,
                rew.twitch_title AS reward_title,
                r.twitch_points_cost,
                r.market_paid_price,
                r.currency,
                r.status,
                i.lifecycle_status AS inventory_lifecycle_status,
                a.status AS latest_attempt_status,
                a.outcome_kind AS latest_attempt_outcome_kind,
                r.fail_cause,
                r.fail_description,
                r.market_item_name,
                r.created_at,
                r.updated_at
             FROM redemptions r
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             LEFT JOIN inventory_items i ON i.redemption_id = r.twitch_redemption_id
             LEFT JOIN LATERAL (SELECT status, outcome_kind FROM inventory_order_attempts
                                WHERE inventory_id = i.id ORDER BY attempt_id DESC LIMIT 1) a ON TRUE
             JOIN broadcasters b ON b.channel_id = rew.streamer_id
             WHERE r.user_id = $1
             ORDER BY r.created_at DESC
             LIMIT $2 OFFSET $3"
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(redemptions)
    }

    pub async fn get_viewer_redemption_stats_on_channel(
        &self,
        channel_id: &str,
        user_id: &str,
    ) -> DbResult<ViewerRedemptionStats> {
        let stats = sqlx::query_as::<_, ViewerRedemptionStats>(
            "SELECT
                COUNT(*)::BIGINT AS total_redemptions,
                COUNT(CASE WHEN r.status = 'COMPLETED' THEN 1 END)::BIGINT AS completed,
                COUNT(CASE WHEN r.status IN ('FAILED_REFUND', 'FAILED_PENALTY') THEN 1 END)::BIGINT AS failed,
                COUNT(CASE WHEN r.status IN ('PENDING', 'ORDER_CREATED', 'MANUAL_HOLD') THEN 1 END)::BIGINT AS pending,
                COALESCE(SUM(r.twitch_points_cost), 0)::BIGINT AS total_points_spent,
                COALESCE(SUM(CASE WHEN r.status = 'COMPLETED' THEN r.market_paid_price ELSE 0 END), 0)::BIGINT AS total_market_value
             FROM redemptions r
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             WHERE rew.streamer_id = $1 AND r.user_id = $2"
        )
        .bind(channel_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(stats)
    }

    pub async fn get_viewer_redemption_stats_global(
        &self,
        user_id: &str,
    ) -> DbResult<ViewerRedemptionStats> {
        let stats = sqlx::query_as::<_, ViewerRedemptionStats>(
            "SELECT
                COUNT(*)::BIGINT AS total_redemptions,
                COUNT(CASE WHEN r.status = 'COMPLETED' THEN 1 END)::BIGINT AS completed,
                COUNT(CASE WHEN r.status IN ('FAILED_REFUND', 'FAILED_PENALTY') THEN 1 END)::BIGINT AS failed,
                COUNT(CASE WHEN r.status IN ('PENDING', 'ORDER_CREATED', 'MANUAL_HOLD') THEN 1 END)::BIGINT AS pending,
                COALESCE(SUM(r.twitch_points_cost), 0)::BIGINT AS total_points_spent,
                COALESCE(SUM(CASE WHEN r.status = 'COMPLETED' THEN r.market_paid_price ELSE 0 END), 0)::BIGINT AS total_market_value
             FROM redemptions r
             WHERE r.user_id = $1"
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(stats)
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

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, utoipa::ToSchema)]
pub struct ViewerChannelRedemption {
    pub twitch_redemption_id: Uuid,
    pub twitch_reward_id: Uuid,
    pub reward_title: String,
    pub twitch_points_cost: i64,
    pub market_paid_price: Option<i64>,
    pub currency: String,
    pub status: RedemptionStatus,
    pub inventory_lifecycle_status: Option<String>,
    pub latest_attempt_status: Option<String>,
    pub latest_attempt_outcome_kind: Option<String>,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
    pub market_item_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, utoipa::ToSchema)]
pub struct ViewerGlobalRedemption {
    pub twitch_redemption_id: Uuid,
    pub twitch_reward_id: Uuid,
    pub channel_id: String,
    pub channel_login: String,
    pub reward_title: String,
    pub twitch_points_cost: i64,
    pub market_paid_price: Option<i64>,
    pub currency: String,
    pub status: RedemptionStatus,
    pub inventory_lifecycle_status: Option<String>,
    pub latest_attempt_status: Option<String>,
    pub latest_attempt_outcome_kind: Option<String>,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
    pub market_item_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, utoipa::ToSchema, Default)]
pub struct ViewerRedemptionStats {
    pub total_redemptions: i64,
    pub completed: i64,
    pub failed: i64,
    pub pending: i64,
    pub total_points_spent: i64,
    pub total_market_value: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::rewards::PurchaseLimitRule;

    async fn insert_limit_test_reward(db: &Db, channel: &str,
        limits: &RewardPurchaseLimitsConfig, reward: Uuid) {
        sqlx::query("INSERT INTO rewards (twitch_id, is_paused, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, purchase_limits, created_at, updated_at) VALUES ($1,false,$2,'AK-47 | Redline','Redline','',2500,10,0,0,0,0,$3,NOW(),NOW())")
            .bind(reward).bind(channel).bind(sqlx::types::Json(limits))
            .execute(db.pool()).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL database in TEST_DATABASE_URL"]
    async fn pending_redemptions_take_both_slots_and_concurrent_admission_is_atomic() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let db = Db { pool };
        let channel = format!("limit-test-{}", Uuid::new_v4());
        let viewer = format!("viewer-{}", Uuid::new_v4());
        let limits = RewardPurchaseLimitsConfig {
            global: vec![],
            user: vec![PurchaseLimitRule { window_hours: Some(8), max_redemptions: 2 }],
        };
        sqlx::query("INSERT INTO users (twitch_id, login) VALUES ($1, $1)")
            .bind(&channel).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO broadcasters (channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at) VALUES ($1,$1,'test','test',NOW(),NOW())")
            .bind(&channel).execute(db.pool()).await.unwrap();
        let make_redemption = |reward: Uuid| NewRedemption {
            twitch_redemption_id: Uuid::new_v4(), twitch_reward_id: reward,
            user_id: viewer.clone(), user_login: "viewer".into(),
            user_trade_link: String::new(), twitch_points_cost: 100,
            currency: "RUB".into(), status: RedemptionStatus::Pending,
            market_item_name: Some("AK-47 | Redline".into()),
        };

        let reward = Uuid::new_v4();
        insert_limit_test_reward(&db, &channel, &limits, reward).await;
        let first = make_redemption(reward);
        let second = make_redemption(reward);
        let third = make_redemption(reward);
        assert!(matches!(db.insert_redemption_with_limits(&first, Utc::now()).await.unwrap().unwrap().1,
            PurchaseLimitDecision::Admitted));
        assert!(matches!(db.insert_redemption_with_limits(&second, Utc::now()).await.unwrap().unwrap().1,
            PurchaseLimitDecision::Admitted));
        assert_eq!(db.get_redemption(first.twitch_redemption_id).await.unwrap().unwrap().status,
            RedemptionStatus::Pending);
        assert_eq!(db.get_redemption(second.twitch_redemption_id).await.unwrap().unwrap().status,
            RedemptionStatus::Pending);
        assert!(matches!(db.insert_redemption_with_limits(&third, Utc::now()).await.unwrap().unwrap().1,
            PurchaseLimitDecision::UserRejected { count: 2, max_redemptions: 2, window_hours: Some(8) }));
        assert_eq!(db.count_reward_redemptions(reward, Some(&viewer), Some(8), None).await.unwrap(), 2);
        assert!(db.insert_redemption_with_limits(&third, Utc::now()).await.unwrap().is_none(),
            "duplicate EventSub delivery must not reserve another slot");

        // A refunded pre-inventory redemption releases its provisional reservation.
        sqlx::query("UPDATE redemptions SET status = 'FAILED_REFUND' WHERE twitch_redemption_id = $1")
            .bind(first.twitch_redemption_id).execute(db.pool()).await.unwrap();
        let fourth = make_redemption(reward);
        assert!(matches!(db.insert_redemption_with_limits(&fourth, Utc::now()).await.unwrap().unwrap().1,
            PurchaseLimitDecision::Admitted));

        let concurrent_reward = Uuid::new_v4();
        insert_limit_test_reward(&db, &channel, &limits, concurrent_reward).await;
        let a = make_redemption(concurrent_reward);
        let b = make_redemption(concurrent_reward);
        let c = make_redemption(concurrent_reward);
        let (a_result, b_result, c_result) = tokio::join!(
            db.insert_redemption_with_limits(&a, Utc::now()),
            db.insert_redemption_with_limits(&b, Utc::now()),
            db.insert_redemption_with_limits(&c, Utc::now()),
        );
        let decisions = [a_result.unwrap().unwrap().1, b_result.unwrap().unwrap().1,
            c_result.unwrap().unwrap().1];
        assert_eq!(decisions.iter().filter(|d| matches!(d, PurchaseLimitDecision::Admitted)).count(), 2);
        assert_eq!(decisions.iter().filter(|d| matches!(d, PurchaseLimitDecision::UserRejected { .. })).count(), 1);
        assert_eq!(db.count_reward_redemptions(concurrent_reward, Some(&viewer), Some(8), None).await.unwrap(), 2);

        let ids = [a.twitch_redemption_id, b.twitch_redemption_id, c.twitch_redemption_id];
        let admitted_index = decisions.iter().position(|d| matches!(d, PurchaseLimitDecision::Admitted)).unwrap();
        sqlx::query("UPDATE redemptions SET status = 'MANUAL_HOLD' WHERE twitch_redemption_id = $1")
            .bind(ids[admitted_index]).execute(db.pool()).await.unwrap();
        assert_eq!(db.count_reward_redemptions(concurrent_reward, Some(&viewer), Some(8), None).await.unwrap(), 2,
            "an unresolved manual hold must still occupy its slot");

        let window_reward = Uuid::new_v4();
        insert_limit_test_reward(&db, &channel, &limits, window_reward).await;
        let old = make_redemption(window_reward);
        let old_time = Utc::now() - chrono::Duration::hours(9);
        assert!(matches!(db.insert_redemption_with_limits(&old, old_time).await.unwrap().unwrap().1,
            PurchaseLimitDecision::Admitted));
        assert!((db.get_redemption(old.twitch_redemption_id).await.unwrap().unwrap().created_at
            - old_time).num_seconds().abs() <= 1);
        let recent = make_redemption(window_reward);
        assert!(matches!(db.insert_redemption_with_limits(&recent, Utc::now()).await.unwrap().unwrap().1,
            PurchaseLimitDecision::Admitted));
        assert_eq!(db.count_reward_redemptions(window_reward, Some(&viewer), Some(8), None).await.unwrap(), 1,
            "the rolling window uses redemption creation time, not fulfillment time");
    }

    #[test]
    fn test_redemption_status_serde() {
        let statuses = vec![
            (RedemptionStatus::Pending, "\"Pending\""),
            (RedemptionStatus::OrderCreated, "\"OrderCreated\""),
            (RedemptionStatus::FailedRefund, "\"FailedRefund\""),
            (RedemptionStatus::FailedPenalty, "\"FailedPenalty\""),
            (RedemptionStatus::Completed, "\"Completed\""),
            (RedemptionStatus::ManualHold, "\"ManualHold\""),
        ];

        for (status, expected_json) in statuses {
            let serialized = serde_json::to_string(&status).unwrap();
            assert_eq!(serialized, expected_json);
            let deserialized: RedemptionStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, status);
        }
    }

    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL database in TEST_DATABASE_URL"]
    async fn inventory_rolls_survive_refund_and_are_atomic_at_creation() {
        let pool = sqlx::PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let db = Db { pool };
        let channel = format!("roll-test-{}", Uuid::new_v4());
        let viewer = format!("roll-viewer-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO users (twitch_id, login) VALUES ($1,$1)")
            .bind(&channel).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO broadcasters (channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at) VALUES ($1,$1,'test','test',NOW(),NOW())")
            .bind(&channel).execute(db.pool()).await.unwrap();
        let limits = RewardPurchaseLimitsConfig { global: vec![],
            user: vec![PurchaseLimitRule { window_hours: Some(8), max_redemptions: 2 }] };
        let reward = Uuid::new_v4();
        insert_limit_test_reward(&db, &channel, &limits, reward).await;
        let new = |reward| NewRedemption { twitch_redemption_id: Uuid::new_v4(), twitch_reward_id: reward,
            user_id: viewer.clone(), user_login: "viewer".into(), user_trade_link: String::new(),
            twitch_points_cost: 100, currency: "RUB".into(), status: RedemptionStatus::Pending,
            market_item_name: None };

        // Failed eligibility before selection consumes no lasting roll.
        let eligibility = new(reward);
        db.insert_redemption_with_limits(&eligibility, Utc::now()).await.unwrap();
        db.update_redemption_status(eligibility.twitch_redemption_id, RedemptionStatus::FailedRefund,
            Some("chat_requirements"), None).await.unwrap();
        assert_eq!(db.count_reward_redemptions(reward, Some(&viewer), Some(8), None).await.unwrap(), 0);
        assert!(!db.create_inventory_item(eligibility.twitch_redemption_id, "Not selected", 275, "VIEWER", false).await.unwrap());

        let first = new(reward);
        let second = new(reward);
        for r in [&first, &second] {
            assert!(matches!(db.insert_redemption_with_limits(r, Utc::now()).await.unwrap().unwrap().1, PurchaseLimitDecision::Admitted));
            assert!(db.create_inventory_item(r.twitch_redemption_id, "Frozen skin", 275, "VIEWER", false).await.unwrap());
            assert!(!db.create_inventory_item(r.twitch_redemption_id, "Frozen skin", 275, "VIEWER", false).await.unwrap());
        }
        let third = new(reward);
        assert!(matches!(db.insert_redemption_with_limits(&third, Utc::now()).await.unwrap().unwrap().1, PurchaseLimitDecision::UserRejected { count: 2, .. }));
        assert!(!db.create_inventory_item(third.twitch_redemption_id, "Frozen skin", 275, "VIEWER", false).await.unwrap());

        // Attempts reuse one roll; failed attempt and retry do not add slots.
        let states = db.get_redemption_inventory_states(&[first.twitch_redemption_id, second.twitch_redemption_id]).await.unwrap();
        assert!(states.iter().all(|s| s.operator_can_attempt));
        let custom = db.begin_inventory_attempt(second.twitch_redemption_id, "fixture-link", false).await.unwrap().unwrap();
        assert!(!db.get_redemption_inventory_states(&[second.twitch_redemption_id]).await.unwrap()[0].operator_can_attempt);
        db.mark_attempt_rejected(second.twitch_redemption_id, &custom, "no_money", "fixture").await.unwrap();
        sqlx::query("UPDATE inventory_items SET last_action_at = NOW() - INTERVAL '1 minute' WHERE redemption_id=$1")
            .bind(second.twitch_redemption_id).execute(db.pool()).await.unwrap();
        assert!(db.get_redemption_inventory_states(&[second.twitch_redemption_id]).await.unwrap()[0].operator_can_attempt);
        let retry = db.begin_inventory_attempt(second.twitch_redemption_id, "fixture-link", false).await.unwrap().unwrap();
        db.mark_attempt_rejected(second.twitch_redemption_id, &retry, "unavailable", "fixture").await.unwrap();
        assert_eq!(db.count_reward_redemptions(reward, Some(&viewer), Some(8), None).await.unwrap(), 2);

        assert!(db.reserve_inventory_refund(first.twitch_redemption_id, true).await.unwrap());
        db.finish_inventory_refund(first.twitch_redemption_id, true).await.unwrap();
        assert_eq!(db.count_reward_redemptions(reward, Some(&viewer), Some(8), None).await.unwrap(), 2);
        assert!(matches!(db.insert_redemption_with_limits(&new(reward), Utc::now()).await.unwrap().unwrap().1, PurchaseLimitDecision::UserRejected { .. }));
        // Global and lifetime rules keep their prior refund semantics.
        assert_eq!(db.count_reward_redemptions(reward, None, Some(8), None).await.unwrap(), 1);
        assert_eq!(db.count_reward_redemptions(reward, Some(&viewer), None, None).await.unwrap(), 1);

        // Both profile APIs expose the same persisted lifecycle and isolate ownership.
        let global = db.get_viewer_redemptions_global(&viewer, 20, 0).await.unwrap();
        let scoped = db.get_viewer_redemptions_on_channel(&channel, &viewer, 20, 0).await.unwrap();
        let row = global.iter().find(|r| r.twitch_redemption_id == first.twitch_redemption_id).unwrap();
        assert_eq!(row.inventory_lifecycle_status.as_deref(), Some("REFUNDED"));
        let row = scoped.iter().find(|r| r.twitch_redemption_id == second.twitch_redemption_id).unwrap();
        assert_eq!(row.latest_attempt_outcome_kind.as_deref(), Some("unavailable"));
        assert!(db.get_viewer_redemptions_global("other-viewer", 20, 0).await.unwrap().is_empty());
        assert!(db.get_viewer_redemptions_on_channel("other-channel", &viewer, 20, 0).await.unwrap().is_empty());

        sqlx::query("UPDATE redemptions SET created_at = NOW() - INTERVAL '9 hours' WHERE twitch_redemption_id=$1")
            .bind(first.twitch_redemption_id).execute(db.pool()).await.unwrap();
        assert!(matches!(db.insert_redemption_with_limits(&new(reward), Utc::now()).await.unwrap().unwrap().1, PurchaseLimitDecision::Admitted));

        // Concurrent admission and concrete creation allow exactly two rolls.
        let concurrent_reward = Uuid::new_v4();
        insert_limit_test_reward(&db, &channel, &limits, concurrent_reward).await;
        let concurrent = [new(concurrent_reward), new(concurrent_reward), new(concurrent_reward)];
        let (a,b,c) = tokio::join!(
            db.insert_redemption_with_limits(&concurrent[0], Utc::now()),
            db.insert_redemption_with_limits(&concurrent[1], Utc::now()),
            db.insert_redemption_with_limits(&concurrent[2], Utc::now()));
        assert_eq!([a.unwrap().unwrap().1,b.unwrap().unwrap().1,c.unwrap().unwrap().1].into_iter()
            .filter(|decision| matches!(decision, PurchaseLimitDecision::Admitted)).count(), 2);
        let (a,b,c) = tokio::join!(
            db.create_inventory_item(concurrent[0].twitch_redemption_id, "Same skin", 100, "VIEWER", false),
            db.create_inventory_item(concurrent[1].twitch_redemption_id, "Same skin", 100, "VIEWER", false),
            db.create_inventory_item(concurrent[2].twitch_redemption_id, "Same skin", 100, "VIEWER", false));
        assert_eq!([a.unwrap(), b.unwrap(), c.unwrap()].into_iter().filter(|created| *created).count(), 2);
        assert_eq!(db.count_reward_redemptions(concurrent_reward, Some(&viewer), Some(8), None).await.unwrap(), 2);
    }
}
