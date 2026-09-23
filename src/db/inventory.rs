use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;
use sqlx::Row;
use crate::db::{Db, error::DbResult};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct InventoryItem {
    pub id: Uuid,
    pub redemption_id: Uuid,
    pub viewer_id: String,
    pub item_name: String,
    /// The first max_price sent to Market /buy-for, in minor currency units.
    pub fixed_price: i64,
    pub currency: String,
    pub lifecycle_status: String,
    pub fulfillment_mode: String,
    pub buyer_retry_allowed: bool,
    pub market_order_id: Option<String>,
    pub market_custom_id: Option<String>,
    pub latest_attempt_custom_id: Option<String>,
    pub latest_attempt_max_price: Option<i64>,
    pub latest_attempt_status: Option<String>,
    pub latest_attempt_outcome_kind: Option<String>,
    pub attempt_count: i64,
    pub created_at: DateTime<Utc>,
    pub acquired_at: Option<DateTime<Utc>>,
    pub channel_id: String,
    pub channel_login: String,
    pub reward_title: String,
    pub redemption_status: String,
    pub fail_cause: Option<String>,
    pub fail_description: Option<String>,
}

#[derive(Debug, FromRow)]
pub struct PendingInventoryOrder {
    pub redemption_id: Uuid,
    pub reward_id: Uuid,
    pub channel_id: String,
    pub user_login: String,
    pub user_trade_link: String,
    pub item_name: String,
    pub fixed_price: i64,
    pub currency: String,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ViewerSettings {
    pub viewer_id: String,
    pub auto_buy_enabled: bool,
    pub trade_link: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct InventoryAttempt {
    pub custom_id: String,
    pub inventory_id: Uuid,
    pub item_name: String,
    pub max_price: Option<i64>,
    pub market_order_id: Option<String>,
    pub status: String,
    pub outcome_kind: Option<String>,
    pub trade_link: Option<String>,
    pub trade_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Db {
    pub async fn get_delivered_inventory_awaiting_twitch(&self) -> DbResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT redemption_id FROM inventory_items WHERE lifecycle_status = 'DELIVERED'
             AND fulfillment_mode != 'LEGACY_REVIEW' AND twitch_fulfilled_at IS NULL"
        ).fetch_all(&self.pool).await?)
    }

    pub async fn inventory_twitch_fulfillment_pending(&self, redemption_id: Uuid) -> DbResult<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_items WHERE redemption_id = $1 AND lifecycle_status = 'DELIVERED' AND fulfillment_mode != 'LEGACY_REVIEW' AND twitch_fulfilled_at IS NULL)")
            .bind(redemption_id).fetch_one(&self.pool).await?)
    }

    pub async fn mark_inventory_twitch_fulfilled(&self, redemption_id: Uuid) -> DbResult<()> {
        sqlx::query("UPDATE inventory_items SET twitch_fulfilled_at = NOW() WHERE redemption_id = $1 AND lifecycle_status = 'DELIVERED' AND twitch_fulfilled_at IS NULL")
            .bind(redemption_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn mark_legacy_inventory_delivered(&self, redemption_id: Uuid) -> DbResult<()> {
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'DELIVERED', acquired_at = COALESCE(acquired_at, NOW()), twitch_fulfilled_at = NOW() WHERE redemption_id = $1 AND fulfillment_mode = 'LEGACY_REVIEW' AND lifecycle_status NOT IN ('REFUNDING','REFUNDED')")
            .bind(redemption_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_unresolved_inventory_attempts(&self) -> DbResult<Vec<(Uuid, String)>> {
        Ok(sqlx::query_as(
            "SELECT i.redemption_id, a.custom_id FROM inventory_order_attempts a
             JOIN inventory_items i ON i.id = a.inventory_id
             WHERE i.fulfillment_mode != 'LEGACY_REVIEW'
               AND a.status IN ('CALLING','RECONCILIATION_REQUIRED')
               AND i.lifecycle_status NOT IN ('DELIVERED','REFUNDING','REFUNDED')"
        ).fetch_all(&self.pool).await?)
    }
    pub async fn viewer_inventory_redemption(&self, inventory_id: Uuid, viewer_id: &str) -> DbResult<Option<Uuid>> {
        Ok(sqlx::query_scalar("SELECT redemption_id FROM inventory_items WHERE id = $1 AND viewer_id = $2")
            .bind(inventory_id).bind(viewer_id).fetch_optional(&self.pool).await?)
    }

    pub async fn operator_inventory_redemption(&self, inventory_id: Uuid, viewer_id: &str, channel_id: &str) -> DbResult<Option<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT i.redemption_id FROM inventory_items i JOIN redemptions r ON r.twitch_redemption_id = i.redemption_id
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             WHERE i.id = $1 AND i.viewer_id = $2 AND rew.streamer_id = $3"
        ).bind(inventory_id).bind(viewer_id).bind(channel_id).fetch_optional(&self.pool).await?)
    }
    pub async fn get_viewer_settings(&self, viewer_id: &str) -> DbResult<ViewerSettings> {
        if let Some(settings) = sqlx::query_as::<_, ViewerSettings>(
            "SELECT viewer_id, auto_buy_enabled, trade_link, updated_at FROM viewer_settings WHERE viewer_id = $1"
        ).bind(viewer_id).fetch_optional(&self.pool).await? { return Ok(settings); }
        Ok(ViewerSettings { viewer_id: viewer_id.to_string(), auto_buy_enabled: true, trade_link: None, updated_at: Utc::now() })
    }

    pub async fn save_viewer_settings(&self, viewer_id: &str, auto_buy_enabled: bool, trade_link: Option<&str>) -> DbResult<ViewerSettings> {
        Ok(sqlx::query_as::<_, ViewerSettings>(
            "INSERT INTO viewer_settings (viewer_id, auto_buy_enabled, trade_link) VALUES ($1, $2, $3)
             ON CONFLICT (viewer_id) DO UPDATE SET auto_buy_enabled = EXCLUDED.auto_buy_enabled,
                 trade_link = EXCLUDED.trade_link, updated_at = NOW()
             RETURNING viewer_id, auto_buy_enabled, trade_link, updated_at"
        ).bind(viewer_id).bind(auto_buy_enabled).bind(trade_link).fetch_one(&self.pool).await?)
    }

    pub async fn latest_inventory_attempt(&self, redemption_id: Uuid) -> DbResult<Option<InventoryAttempt>> {
        Ok(sqlx::query_as::<_, InventoryAttempt>(
            "SELECT a.custom_id, a.inventory_id, a.item_name, a.max_price, a.market_order_id,
                    a.status, a.outcome_kind, a.trade_link, a.trade_id, a.created_at
             FROM inventory_order_attempts a JOIN inventory_items i ON i.id = a.inventory_id
             WHERE i.redemption_id = $1 ORDER BY a.attempt_id DESC LIMIT 1"
        ).bind(redemption_id).fetch_optional(&self.pool).await?)
    }
    pub async fn get_inventory_core(&self, redemption_id: Uuid) -> DbResult<Option<(Uuid, String, i64, String, String, bool)>> {
        Ok(sqlx::query_as(
            "SELECT id, item_name, fixed_price, currency, fulfillment_mode, buyer_retry_allowed FROM inventory_items WHERE redemption_id = $1"
        ).bind(redemption_id).fetch_optional(&self.pool).await?)
    }

    pub async fn require_inventory_trade_link(&self, redemption_id: Uuid) -> DbResult<()> {
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'TRADE_LINK_REQUIRED' WHERE redemption_id = $1 AND lifecycle_status IN ('WAITING_VIEWER','ORDER_PENDING','RETRY_AVAILABLE','INSUFFICIENT_FUNDS','TRADE_LINK_REQUIRED')")
            .bind(redemption_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn require_inventory_reconciliation(&self, redemption_id: Uuid, custom_id: &str) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let id: Uuid = sqlx::query_scalar("SELECT id FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE inventory_order_attempts SET status = 'RECONCILIATION_REQUIRED' WHERE inventory_id = $1 AND custom_id = $2 AND status IN ('CALLING','ORDER_CREATED','TRADE_WAITING')")
            .bind(id).bind(custom_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'RECONCILIATION_REQUIRED' WHERE id = $1 AND lifecycle_status NOT IN ('DELIVERED','REFUNDING','REFUNDED')")
            .bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The inventory row serializes purchase, refund and delivery decisions.
    /// A claimed external call remains unsafe until a definitive result is saved.
    pub async fn begin_inventory_attempt(&self, redemption_id: Uuid, trade_link: &str, viewer_action: bool) -> DbResult<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, lifecycle_status, fulfillment_mode, buyer_retry_allowed, last_action_at
             FROM inventory_items WHERE redemption_id = $1 FOR UPDATE"
        ).bind(redemption_id).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None); };
        let inventory_id: Uuid = row.try_get("id")?;
        let status: String = row.try_get("lifecycle_status")?;
        let mode: String = row.try_get("fulfillment_mode")?;
        let buyer_allowed: bool = row.try_get("buyer_retry_allowed")?;
        let last_action: Option<DateTime<Utc>> = row.try_get("last_action_at")?;
        if mode == "LEGACY_REVIEW" || matches!(status.as_str(), "ORDER_PENDING" | "TRADE_WAITING" | "RECONCILIATION_REQUIRED" | "DELIVERED" | "REFUNDING" | "REFUNDED") {
            return Ok(None);
        }
        let redemption_status: String = sqlx::query_scalar("SELECT status FROM redemptions WHERE twitch_redemption_id = $1")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        if redemption_status != "PENDING" { return Ok(None); }
        if last_action.is_some_and(|at| (Utc::now() - at).num_seconds() < 30) { return Ok(None); }
        let latest = sqlx::query("SELECT status, outcome_kind FROM inventory_order_attempts WHERE inventory_id = $1 ORDER BY attempt_id DESC LIMIT 1")
            .bind(inventory_id).fetch_optional(&mut *tx).await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inventory_order_attempts WHERE inventory_id = $1")
            .bind(inventory_id).fetch_one(&mut *tx).await?;
        if let Some(ref previous) = latest {
            let previous_status: String = previous.try_get("status")?;
            if !matches!(previous_status.as_str(), "REJECTED" | "SELLER_FAILED" | "BUYER_FAILED") { return Ok(None); }
            if previous_status == "BUYER_FAILED" && viewer_action && !buyer_allowed { return Ok(None); }
        }
        let any_unresolved: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM inventory_order_attempts WHERE inventory_id = $1
             AND status NOT IN ('REJECTED','SELLER_FAILED','BUYER_FAILED','DELIVERED'))"
        ).bind(inventory_id).fetch_one(&mut *tx).await?;
        if any_unresolved { return Ok(None); }
        let custom_id = if count == 0 { redemption_id.to_string() } else { format!("{}-{}", redemption_id, count) };
        let item: (String, i64) = sqlx::query_as("SELECT item_name, fixed_price FROM inventory_items WHERE id = $1")
            .bind(inventory_id).fetch_one(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO inventory_order_attempts (custom_id, inventory_id, item_name, max_price, trade_link, status)
             VALUES ($1, $2, $3, $4, $5, 'CALLING')"
        ).bind(&custom_id).bind(inventory_id).bind(&item.0).bind(item.1).bind(trade_link).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'ORDER_PENDING', last_action_at = NOW(), market_custom_id = $2, market_order_id = NULL WHERE id = $1")
            .bind(inventory_id).bind(&custom_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(custom_id))
    }

    pub async fn mark_attempt_rejected(&self, redemption_id: Uuid, custom_id: &str, kind: &str, detail: &str) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT id, lifecycle_status FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        let inventory_id: Uuid = row.try_get("id")?;
        let lifecycle: String = row.try_get("lifecycle_status")?;
        sqlx::query("UPDATE inventory_order_attempts SET status = 'REJECTED', outcome_kind = $3, outcome_detail = $4, resolved_at = NOW() WHERE inventory_id = $1 AND custom_id = $2 AND status = 'CALLING'")
            .bind(inventory_id).bind(custom_id).bind(kind).bind(detail).execute(&mut *tx).await?;
        if lifecycle == "ORDER_PENDING" {
            let next = match kind { "no_money" => "INSUFFICIENT_FUNDS", "trade_link" => "TRADE_LINK_REQUIRED", _ => "RETRY_AVAILABLE" };
            sqlx::query("UPDATE inventory_items SET lifecycle_status = $2 WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id = $1 AND a.status IN ('CALLING','ORDER_CREATED','TRADE_WAITING','RECONCILIATION_REQUIRED'))")
                .bind(inventory_id).bind(next).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE redemptions SET status = 'PENDING', fail_cause = NULL, fail_description = NULL, updated_at = NOW() WHERE twitch_redemption_id = $1 AND status = 'ORDER_CREATED' AND EXISTS (SELECT 1 FROM inventory_items i WHERE i.redemption_id = $1 AND i.lifecycle_status IN ('RETRY_AVAILABLE','INSUFFICIENT_FUNDS','TRADE_LINK_REQUIRED'))")
            .bind(redemption_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_attempt_ambiguous(&self, redemption_id: Uuid, custom_id: &str, detail: &str) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT id FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        let id: Uuid = row.try_get("id")?;
        sqlx::query("UPDATE inventory_order_attempts SET status = 'RECONCILIATION_REQUIRED', outcome_detail = $3 WHERE inventory_id = $1 AND custom_id = $2 AND status = 'CALLING'")
            .bind(id).bind(custom_id).bind(detail).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'RECONCILIATION_REQUIRED' WHERE id = $1 AND lifecycle_status = 'ORDER_PENDING'")
            .bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn get_pending_inventory_without_attempt(&self) -> DbResult<Vec<PendingInventoryOrder>> {
        Ok(sqlx::query_as::<_, PendingInventoryOrder>(
            "SELECT i.redemption_id, r.twitch_reward_id AS reward_id, rew.streamer_id AS channel_id,
                    r.user_login, r.user_trade_link, i.item_name, i.fixed_price, i.currency
             FROM inventory_items i
             JOIN redemptions r ON r.twitch_redemption_id = i.redemption_id
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             WHERE r.status = 'PENDING' AND i.fulfillment_mode = 'AUTO' AND i.lifecycle_status = 'WAITING_VIEWER'
               AND NOT EXISTS (SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id = i.id)"
        ).fetch_all(&self.pool).await?)
    }
    pub async fn inventory_exists(&self, redemption_id: Uuid) -> DbResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM inventory_items WHERE redemption_id = $1)")
            .bind(redemption_id).fetch_one(&self.pool).await?)
    }
    /// INSERT commits before the caller is allowed to make any external purchase.
    /// The unique redemption constraint and DO NOTHING preserve the first snapshot.
    pub async fn create_inventory_item(&self, redemption_id: Uuid, item_name: &str, fixed_price: i64, fulfillment_mode: &str, buyer_retry_allowed: bool) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO inventory_items (id, redemption_id, viewer_id, item_name, fixed_price, currency, fulfillment_mode, buyer_retry_allowed, lifecycle_status)
             SELECT $2, r.twitch_redemption_id, r.user_id, $3, $4, r.currency, $5, $6,
                    CASE WHEN $5 = 'OPERATOR' THEN 'WAITING_OPERATOR' ELSE 'WAITING_VIEWER' END
             FROM redemptions r WHERE r.twitch_redemption_id = $1
             ON CONFLICT (redemption_id) DO NOTHING"
        )
        .bind(redemption_id).bind(Uuid::new_v4()).bind(item_name).bind(fixed_price)
        .bind(fulfillment_mode).bind(buyer_retry_allowed)
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn attach_inventory_order(&self, redemption_id: Uuid, custom_id: &str, order_id: Option<&str>, item_name: &str) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE inventory_order_attempts SET market_order_id = COALESCE(market_order_id, $2),
                    status = CASE WHEN status IN ('CALLING','RECONCILIATION_REQUIRED') THEN 'ORDER_CREATED' ELSE status END,
                    last_checked_at = NOW() WHERE custom_id = $1")
            .bind(custom_id).bind(order_id).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE inventory_items i SET market_custom_id = $2, market_order_id = COALESCE($3, i.market_order_id),
                 lifecycle_status = CASE WHEN lifecycle_status = 'RECONCILIATION_REQUIRED' OR lifecycle_status = 'ORDER_PENDING' THEN 'ORDER_PENDING' ELSE lifecycle_status END
             FROM inventory_order_attempts a
             WHERE i.redemption_id = $1 AND a.inventory_id = i.id AND a.custom_id = $2
             AND i.lifecycle_status NOT IN ('DELIVERED','REFUNDING','REFUNDED')
             AND (i.market_custom_id IS NULL OR
                  (SELECT old.attempt_id FROM inventory_order_attempts old WHERE old.custom_id = i.market_custom_id) <= a.attempt_id)"
        )
            .bind(redemption_id).bind(custom_id).bind(order_id).execute(&mut *tx).await?;
        let _ = item_name;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_trade_waiting(&self, redemption_id: Uuid, custom_id: &str, trade_id: Option<&str>, send_until: Option<DateTime<Utc>>, receive_until: Option<DateTime<Utc>>) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let id: Uuid = sqlx::query_scalar("SELECT id FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE inventory_order_attempts SET status = 'TRADE_WAITING', trade_id = COALESCE($3, trade_id), send_until = $4, receive_until = $5, last_checked_at = NOW() WHERE inventory_id = $1 AND custom_id = $2 AND status IN ('ORDER_CREATED','TRADE_WAITING')")
            .bind(id).bind(custom_id).bind(trade_id).bind(send_until).bind(receive_until).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'TRADE_WAITING' WHERE id = $1 AND market_custom_id = $2 AND lifecycle_status IN ('ORDER_PENDING','TRADE_WAITING')")
            .bind(id).bind(custom_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_terminal_trade_failure(&self, redemption_id: Uuid, custom_id: &str, buyer_fault: bool, causer: Option<&str>, reason: Option<&str>) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let id: Uuid = sqlx::query_scalar("SELECT id FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        let status = if buyer_fault { "BUYER_FAILED" } else { "SELLER_FAILED" };
        sqlx::query("UPDATE inventory_order_attempts SET status = $3, causer = $4, cancellation_reason = $5, resolved_at = NOW(), last_checked_at = NOW() WHERE inventory_id = $1 AND custom_id = $2 AND status IN ('ORDER_CREATED','TRADE_WAITING','RECONCILIATION_REQUIRED')")
            .bind(id).bind(custom_id).bind(status).bind(causer).bind(reason).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'RETRY_AVAILABLE' WHERE id = $1 AND market_custom_id = $2 AND lifecycle_status IN ('ORDER_PENDING','TRADE_WAITING','RECONCILIATION_REQUIRED')")
            .bind(id).bind(custom_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE redemptions SET status = 'PENDING', fail_cause = NULL, fail_description = NULL, updated_at = NOW() WHERE twitch_redemption_id = $1 AND status = 'ORDER_CREATED' AND EXISTS (SELECT 1 FROM inventory_items i WHERE i.redemption_id = $1 AND i.market_custom_id = $2 AND i.lifecycle_status = 'RETRY_AVAILABLE')")
            .bind(redemption_id).bind(custom_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_inventory_delivered(&self, redemption_id: Uuid, custom_id: &str) -> DbResult<bool> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT id, lifecycle_status, market_custom_id FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        let id: Uuid = row.try_get("id")?;
        let status: String = row.try_get("lifecycle_status")?;
        let active_custom_id: Option<String> = row.try_get("market_custom_id")?;
        if status == "DELIVERED" { return Ok(false); }
        if matches!(status.as_str(), "REFUNDING" | "REFUNDED") || active_custom_id.as_deref() != Some(custom_id) { return Ok(false); }
        sqlx::query("UPDATE inventory_order_attempts SET status = 'DELIVERED', resolved_at = NOW(), last_checked_at = NOW() WHERE inventory_id = $1 AND custom_id = $2")
            .bind(id).bind(custom_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'DELIVERED', acquired_at = COALESCE(acquired_at, NOW()) WHERE id = $1")
            .bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE redemptions SET status = 'COMPLETED', fail_cause = NULL, fail_description = NULL, updated_at = NOW() WHERE twitch_redemption_id = $1 AND status NOT IN ('FAILED_REFUND','COMPLETED')")
            .bind(redemption_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn reserve_inventory_refund(&self, redemption_id: Uuid) -> DbResult<bool> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT id, lifecycle_status FROM inventory_items WHERE redemption_id = $1 FOR UPDATE")
            .bind(redemption_id).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(false); };
        let id: Uuid = row.try_get("id")?;
        let status: String = row.try_get("lifecycle_status")?;
        if !matches!(status.as_str(), "WAITING_VIEWER" | "WAITING_OPERATOR" | "TRADE_LINK_REQUIRED" | "RETRY_AVAILABLE" | "INSUFFICIENT_FUNDS") { return Ok(false); }
        let redemption_status: String = sqlx::query_scalar("SELECT status FROM redemptions WHERE twitch_redemption_id = $1")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        if redemption_status != "PENDING" { return Ok(false); }
        let unsafe_attempt: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_order_attempts WHERE inventory_id = $1 AND status NOT IN ('REJECTED','SELLER_FAILED','BUYER_FAILED'))")
            .bind(id).fetch_one(&mut *tx).await?;
        if unsafe_attempt { return Ok(false); }
        sqlx::query("UPDATE inventory_items SET lifecycle_status = 'REFUNDING' WHERE id = $1")
            .bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn finish_inventory_refund(&self, redemption_id: Uuid, succeeded: bool) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        let id: Uuid = sqlx::query_scalar("SELECT id FROM inventory_items WHERE redemption_id = $1 AND lifecycle_status = 'REFUNDING' FOR UPDATE")
            .bind(redemption_id).fetch_one(&mut *tx).await?;
        if succeeded {
            sqlx::query("UPDATE inventory_items SET lifecycle_status = 'REFUNDED' WHERE id = $1").bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE redemptions SET status = 'FAILED_REFUND', fail_cause = 'explicit_refund', updated_at = NOW() WHERE twitch_redemption_id = $1")
                .bind(redemption_id).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE inventory_items SET lifecycle_status = 'RECONCILIATION_REQUIRED' WHERE id = $1")
                .bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_viewer_inventory(&self, viewer_id: &str, channel_id: Option<&str>, status: Option<&str>, search: Option<&str>, limit: i64, offset: i64) -> DbResult<Vec<InventoryItem>> {
        Ok(sqlx::query_as::<_, InventoryItem>(
            "SELECT i.id, i.redemption_id, i.viewer_id, i.item_name, i.fixed_price, i.currency,
                    i.lifecycle_status, i.fulfillment_mode, i.buyer_retry_allowed,
                    i.market_order_id, i.market_custom_id, latest.custom_id AS latest_attempt_custom_id,
                    latest.max_price AS latest_attempt_max_price, latest.status AS latest_attempt_status,
                    latest.outcome_kind AS latest_attempt_outcome_kind,
                    (SELECT COUNT(*)::BIGINT FROM inventory_order_attempts a WHERE a.inventory_id = i.id) AS attempt_count,
                    i.created_at, i.acquired_at,
                    rew.streamer_id AS channel_id, b.channel_login, rew.twitch_title AS reward_title,
                    r.status AS redemption_status, r.fail_cause, r.fail_description
             FROM inventory_items i JOIN redemptions r ON r.twitch_redemption_id = i.redemption_id
             JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id
             JOIN broadcasters b ON b.channel_id = rew.streamer_id
             LEFT JOIN LATERAL (
                 SELECT custom_id, max_price, status, outcome_kind FROM inventory_order_attempts
                 WHERE inventory_id = i.id ORDER BY attempt_id DESC LIMIT 1
             ) latest ON TRUE
             WHERE i.viewer_id = $1 AND ($2::VARCHAR IS NULL OR rew.streamer_id = $2)
               AND ($3::VARCHAR IS NULL OR i.lifecycle_status = $3)
               AND ($4::TEXT IS NULL OR i.item_name ILIKE '%' || $4 || '%')
             ORDER BY i.created_at DESC, i.id DESC LIMIT $5 OFFSET $6"
        ).bind(viewer_id).bind(channel_id).bind(status).bind(search).bind(limit).bind(offset).fetch_all(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL database in TEST_DATABASE_URL"]
    async fn inventory_database_invariants() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let db = Db { pool };
        let channel = format!("inventory-test-{}", Uuid::new_v4());
        let reward = Uuid::new_v4();
        let redemption = Uuid::new_v4();
        let old_redemption = Uuid::new_v4();
        let viewer = format!("viewer-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO users (twitch_id, login) VALUES ($1, 'streamer')")
            .bind(&channel).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO broadcasters (channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at) VALUES ($1, $1, 'test', 'test', NOW(), NOW())")
            .bind(&channel).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO rewards (twitch_id, is_paused, streamer_id, market_item_name, twitch_title, twitch_description, current_market_price, permissible_market_price_deviation, twitch_price_markup_percentage, global_cooldown_seconds, max_redemptions_per_stream, max_redemptions_per_user_per_stream, created_at, updated_at) VALUES ($1, false, $2, 'AK-47 | Redline', 'Redline', '', 2500, 10, 0, 0, 1, 1, NOW(), NOW())")
            .bind(reward).bind(&channel).execute(db.pool()).await.unwrap();
        for id in [redemption, old_redemption] {
            sqlx::query("INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, status, market_item_name, created_at, updated_at) VALUES ($1, $2, $3, 'viewer', 'test-link', 100, 'PENDING', 'AK-47 | Redline', NOW(), NOW())")
                .bind(id).bind(reward).bind(&viewer).execute(db.pool()).await.unwrap();
        }
        // The viewer has no OAuth account or session; EventSub's stable ID suffices.
        assert!(!sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE twitch_id=$1)")
            .bind(&viewer).fetch_one(db.pool()).await.unwrap());
        db.create_inventory_item(redemption, "AK-47 | Redline", 2750, "AUTO", false).await.unwrap();
        db.create_inventory_item(redemption, "Different skin", 9900, "OPERATOR", true).await.unwrap();
        let unclaimed = db.get_pending_inventory_without_attempt().await.unwrap();
        assert_eq!(unclaimed.len(), 1);
        assert_eq!(unclaimed[0].redemption_id, redemption);
        assert_eq!(unclaimed[0].fixed_price, 2750);
        let (first, duplicate) = tokio::join!(
            db.begin_inventory_attempt(redemption, "test-link", false),
            db.begin_inventory_attempt(redemption, "test-link", false),
        );
        let first = first.unwrap();
        let duplicate = duplicate.unwrap();
        assert_ne!(first.is_some(), duplicate.is_some());
        let custom_id = first.or(duplicate).unwrap();
        assert_eq!(custom_id, redemption.to_string());
        assert!(db.get_pending_inventory_without_attempt().await.unwrap().is_empty());
        let items = db.get_viewer_inventory(&viewer, Some(&channel), None, None, 20, 0).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].fixed_price, 2750);
        assert_eq!(items[0].attempt_count, 1);
        assert_eq!(items[0].latest_attempt_max_price, Some(2750));
        assert!(db.begin_inventory_attempt(redemption, "test-link", false).await.unwrap().is_none());
        db.mark_attempt_rejected(redemption, &custom_id, "no_money", "insufficient balance").await.unwrap();
        assert_eq!(db.get_viewer_inventory(&viewer, None, Some("INSUFFICIENT_FUNDS"), Some("Redline"), 20, 0).await.unwrap().len(), 1);
        sqlx::query("UPDATE inventory_items SET last_action_at = NOW() - INTERVAL '31 seconds' WHERE redemption_id = $1")
            .bind(redemption).execute(db.pool()).await.unwrap();
        let (retry_a, retry_b) = tokio::join!(
            db.begin_inventory_attempt(redemption, "test-link", false),
            db.begin_inventory_attempt(redemption, "test-link", false),
        );
        let retry_a = retry_a.unwrap();
        let retry_b = retry_b.unwrap();
        assert_ne!(retry_a.is_some(), retry_b.is_some());
        let retry_id = retry_a.or(retry_b).unwrap();
        assert_eq!(retry_id, format!("{}-1", redemption));
        let in_flight = db.get_viewer_inventory(&viewer, None, None, None, 20, 0).await.unwrap();
        assert_eq!(in_flight[0].market_custom_id.as_deref(), Some(retry_id.as_str()));
        assert!(in_flight[0].market_order_id.is_none());
        assert!(db.get_viewer_inventory("different-viewer", Some(&channel), None, None, 20, 0).await.unwrap().is_empty());
        assert!(db.get_viewer_inventory(&viewer, Some("different-channel"), None, None, 20, 0).await.unwrap().is_empty());
        assert!(sqlx::query("UPDATE inventory_items SET fixed_price = 9900 WHERE redemption_id = $1")
            .bind(redemption).execute(db.pool()).await.is_err());
        assert!(sqlx::query("UPDATE inventory_items SET viewer_id = 'other' WHERE redemption_id = $1")
            .bind(redemption).execute(db.pool()).await.is_err());
        assert!(sqlx::query("UPDATE inventory_items SET item_name = 'Different skin' WHERE redemption_id = $1")
            .bind(redemption).execute(db.pool()).await.is_err());
        assert!(sqlx::query("UPDATE inventory_items SET currency = 'USD' WHERE redemption_id = $1")
            .bind(redemption).execute(db.pool()).await.is_err());
        assert!(sqlx::query("UPDATE inventory_order_attempts SET max_price = 9999 WHERE custom_id = $1")
            .bind(&custom_id).execute(db.pool()).await.is_err());
        assert!(sqlx::query("INSERT INTO inventory_order_attempts (custom_id, inventory_id, item_name, max_price, status) SELECT $2, id, 'Different skin', 2750, 'REJECTED' FROM inventory_items WHERE redemption_id = $1")
            .bind(redemption).bind(format!("{redemption}-wrong")).execute(db.pool()).await.is_err());
        db.attach_inventory_order(redemption, &custom_id, Some("market-123"), "AK-47 | Redline").await.unwrap();
        assert!(!db.reserve_inventory_refund(redemption).await.unwrap());
        db.attach_inventory_order(redemption, &retry_id, Some("market-456"), "AK-47 | Redline").await.unwrap();
        db.attach_inventory_order(redemption, &custom_id, Some("market-123"), "AK-47 | Redline").await.unwrap();
        assert_eq!(db.get_viewer_inventory(&viewer, None, None, None, 20, 0).await.unwrap()[0].market_order_id.as_deref(), Some("market-456"));
        assert!(db.mark_inventory_delivered(redemption, &retry_id).await.unwrap());
        assert!(!db.mark_inventory_delivered(redemption, &retry_id).await.unwrap());
        db.set_redemption_order_created(redemption, 9999, Some("AK-47 | Redline"), 1).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM redemptions WHERE twitch_redemption_id = $1")
            .bind(redemption).fetch_one(db.pool()).await.unwrap();
        assert_eq!(status, "COMPLETED");
        assert!(!db.reserve_inventory_refund(redemption).await.unwrap());
        sqlx::query("UPDATE rewards SET current_market_price=8800 WHERE twitch_id=$1")
            .bind(reward).execute(db.pool()).await.unwrap();
        let items = db.get_viewer_inventory(&viewer, None, None, None, 20, 0).await.unwrap();
        assert_eq!(items[0].fixed_price, 2750);
        assert_eq!(items[0].market_order_id.as_deref(), Some("market-456"));
        assert_eq!(items[0].lifecycle_status, "DELIVERED");
        assert!(items[0].acquired_at.is_some());
        assert!(db.inventory_twitch_fulfillment_pending(redemption).await.unwrap());
        assert!(db.get_delivered_inventory_awaiting_twitch().await.unwrap().contains(&redemption));
        db.mark_inventory_twitch_fulfilled(redemption).await.unwrap();
        assert!(!db.inventory_twitch_fulfillment_pending(redemption).await.unwrap());
        let settings = db.get_viewer_settings(&viewer).await.unwrap();
        assert!(settings.auto_buy_enabled);
        assert!(settings.trade_link.is_none());
        let settings = db.save_viewer_settings(&viewer, false, Some("https://steamcommunity.com/tradeoffer/new/?partner=1&token=abc")).await.unwrap();
        assert!(!settings.auto_buy_enabled);
        // Historical paid prices do not create inventory or external orders.
        assert!(!db.inventory_exists(old_redemption).await.unwrap());
        db.create_inventory_item(old_redemption, "Historical item", 1000, "LEGACY_REVIEW", false).await.unwrap();
        assert!(db.begin_inventory_attempt(old_redemption, "test-link", false).await.unwrap().is_none());
        assert!(db.latest_inventory_attempt(old_redemption).await.unwrap().is_none());

        // Viewer auto-buy OFF creates an owned item with no attempt. Refund and
        // purchase are serialized against the same inventory row.
        let waiting_id = Uuid::new_v4();
        sqlx::query("INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, status, created_at, updated_at) VALUES ($1,$2,$3,'viewer','',100,'PENDING',NOW(),NOW())")
            .bind(waiting_id).bind(reward).bind(&viewer).execute(db.pool()).await.unwrap();
        db.create_inventory_item(waiting_id, "M4A4 | Evil Daimyo", 1950, "VIEWER", false).await.unwrap();
        assert!(!db.get_pending_inventory_without_attempt().await.unwrap().iter().any(|i| i.redemption_id == waiting_id));
        assert!(db.reserve_inventory_refund(waiting_id).await.unwrap());
        assert!(db.begin_inventory_attempt(waiting_id, "test-link", true).await.unwrap().is_none());
        db.finish_inventory_refund(waiting_id, true).await.unwrap();
        assert_eq!(db.get_viewer_inventory(&viewer, Some(&channel), Some("REFUNDED"), Some("Daimyo"), 20, 0).await.unwrap().len(), 1);

        let buyer_id = Uuid::new_v4();
        sqlx::query("INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, status, created_at, updated_at) VALUES ($1,$2,$3,'viewer','',100,'PENDING',NOW(),NOW())")
            .bind(buyer_id).bind(reward).bind(&viewer).execute(db.pool()).await.unwrap();
        db.create_inventory_item(buyer_id, "Desert Eagle | Printstream", 6200, "VIEWER", false).await.unwrap();
        let buyer_custom = db.begin_inventory_attempt(buyer_id, "test-link", true).await.unwrap().unwrap();
        db.attach_inventory_order(buyer_id, &buyer_custom, Some("buyer-market"), "Desert Eagle | Printstream").await.unwrap();
        db.set_trade_waiting(buyer_id, &buyer_custom, Some("trade-1"), None, Some(Utc::now())).await.unwrap();
        assert!(!db.reserve_inventory_refund(buyer_id).await.unwrap());
        db.set_terminal_trade_failure(buyer_id, &buyer_custom, true, Some("buyer"), Some("declined")).await.unwrap();
        sqlx::query("UPDATE rewards SET retry_on_buyer_failure = TRUE WHERE twitch_id = $1")
            .bind(reward).execute(db.pool()).await.unwrap();
        sqlx::query("UPDATE inventory_items SET last_action_at = NOW() - INTERVAL '31 seconds' WHERE redemption_id = $1")
            .bind(buyer_id).execute(db.pool()).await.unwrap();
        assert!(db.begin_inventory_attempt(buyer_id, "test-link", true).await.unwrap().is_none());
        assert!(db.reserve_inventory_refund(buyer_id).await.unwrap());
        db.finish_inventory_refund(buyer_id, true).await.unwrap();
        assert!(!db.mark_inventory_delivered(buyer_id, &buyer_custom).await.unwrap());

        let allowed_id = Uuid::new_v4();
        sqlx::query("INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, status, created_at, updated_at) VALUES ($1,$2,$3,'viewer','',100,'PENDING',NOW(),NOW())")
            .bind(allowed_id).bind(reward).bind(&viewer).execute(db.pool()).await.unwrap();
        db.create_inventory_item(allowed_id, "USP-S | Cortex", 3900, "VIEWER", true).await.unwrap();
        let first = db.begin_inventory_attempt(allowed_id, "test-link", true).await.unwrap().unwrap();
        db.attach_inventory_order(allowed_id, &first, Some("allowed-market"), "USP-S | Cortex").await.unwrap();
        db.set_terminal_trade_failure(allowed_id, &first, true, Some("buyer"), Some("expired")).await.unwrap();
        sqlx::query("UPDATE inventory_items SET last_action_at = NOW() - INTERVAL '31 seconds' WHERE redemption_id = $1")
            .bind(allowed_id).execute(db.pool()).await.unwrap();
        assert!(db.begin_inventory_attempt(allowed_id, "test-link", true).await.unwrap().is_some());
        assert!(!db.reserve_inventory_refund(allowed_id).await.unwrap());

        let seller_id = Uuid::new_v4();
        sqlx::query("INSERT INTO redemptions (twitch_redemption_id, twitch_reward_id, user_id, user_login, user_trade_link, twitch_points_cost, status, created_at, updated_at) VALUES ($1,$2,$3,'viewer','',100,'PENDING',NOW(),NOW())")
            .bind(seller_id).bind(reward).bind(&viewer).execute(db.pool()).await.unwrap();
        db.create_inventory_item(seller_id, "AWP | Asiimov", 4100, "VIEWER", false).await.unwrap();
        let seller_custom = db.begin_inventory_attempt(seller_id, "test-link", true).await.unwrap().unwrap();
        db.mark_attempt_ambiguous(seller_id, &seller_custom, "transport timeout").await.unwrap();
        assert!(db.begin_inventory_attempt(seller_id, "test-link", true).await.unwrap().is_none());
        assert!(!db.reserve_inventory_refund(seller_id).await.unwrap());
        db.attach_inventory_order(seller_id, &seller_custom, Some("seller-market"), "AWP | Asiimov").await.unwrap();
        db.set_trade_waiting(seller_id, &seller_custom, Some("seller-trade"), None, None).await.unwrap();
        assert!(db.begin_inventory_attempt(seller_id, "test-link", true).await.unwrap().is_none());
        db.set_terminal_trade_failure(seller_id, &seller_custom, false, Some("seller"), Some("cancelled")).await.unwrap();
        assert_eq!(db.get_viewer_inventory(&viewer, None, Some("RETRY_AVAILABLE"), Some("Asiimov"), 20, 0).await.unwrap()[0].attempt_count, 1);
        sqlx::query("UPDATE inventory_items SET last_action_at = NOW() - INTERVAL '31 seconds' WHERE redemption_id = $1")
            .bind(seller_id).execute(db.pool()).await.unwrap();
        let seller_retry = db.begin_inventory_attempt(seller_id, "test-link", true).await.unwrap().unwrap();
        assert_eq!(seller_retry, format!("{}-1", seller_id));
        let attempt = db.latest_inventory_attempt(seller_id).await.unwrap().unwrap();
        assert_eq!(attempt.item_name, "AWP | Asiimov");
        assert_eq!(attempt.max_price, Some(4100));
        assert_eq!(db.get_viewer_inventory(&viewer, None, None, Some("Asiimov"), 20, 0).await.unwrap()[0].fixed_price, 4100);

        let configured = crate::db::rewards::NewReward {
            twitch_id: Uuid::new_v4(), is_paused: false, pause_reason: None,
            streamer_id: channel.clone(), reward_type: crate::db::rewards::RewardType::Fixed,
            pricing_mode: crate::db::rewards::PricingMode::Auto, price_strategy: None,
            market_item_name: Some("P250 | Asiimov".into()), filter_config: None, pool_items: None,
            manual_twitch_points: None, twitch_title: "Configured reward".into(), twitch_description: String::new(),
            current_market_price: 4100, permissible_market_price_deviation: 10,
            twitch_price_markup_percentage: 0, global_cooldown_seconds: 0,
            max_redemptions_per_stream: 0, max_redemptions_per_user_per_stream: 0,
            market_autobuy: true, retry_on_buyer_failure: true, currency: "USD".into(),
            min_market_price: None, max_market_price: None, chat_min_messages: None,
            chat_min_characters: None, chat_time_window_hours: None, chat_logical_operator: None,
            refund_if_chat_req_failed: true, purchase_limits: None, is_public: true,
        };
        let created = db.create_reward(&configured).await.unwrap();
        assert!(created.retry_on_buyer_failure);
        db.update_reward(configured.twitch_id, &crate::db::rewards::UpdateReward {
            retry_on_buyer_failure: Some(false), ..Default::default()
        }).await.unwrap();
        assert!(!db.get_reward_by_twitch_id(configured.twitch_id).await.unwrap().unwrap().retry_on_buyer_failure);
        assert!(db.upsert_reward(&configured).await.unwrap().retry_on_buyer_failure);

        let orphan_id = Uuid::new_v4();
        let orphan = crate::db::redemptions::NewRedemption {
            twitch_redemption_id: orphan_id, twitch_reward_id: reward,
            user_id: viewer.clone(), user_login: "viewer".into(), user_trade_link: String::new(),
            twitch_points_cost: 100, currency: "USD".into(),
            status: crate::db::redemptions::RedemptionStatus::Pending,
            market_item_name: Some("P250 | Asiimov".into()),
        };
        assert!(db.insert_redemption_if_new(&orphan).await.unwrap().is_some());
        sqlx::query("UPDATE redemptions SET inventory_resolution_claimed_at = NOW() - INTERVAL '6 minutes' WHERE twitch_redemption_id = $1")
            .bind(orphan_id).execute(db.pool()).await.unwrap();
        assert_eq!(db.claim_pending_inventory_resolution().await.unwrap(), vec![orphan_id]);
        assert!(db.claim_pending_inventory_resolution().await.unwrap().is_empty());
        db.create_inventory_item(orphan_id, "P250 | Asiimov", 1234, "VIEWER", false).await.unwrap();
        sqlx::query("UPDATE redemptions SET inventory_resolution_claimed_at = NOW() - INTERVAL '6 minutes' WHERE twitch_redemption_id = $1")
            .bind(orphan_id).execute(db.pool()).await.unwrap();
        assert!(db.claim_pending_inventory_resolution().await.unwrap().is_empty());
    }
}
