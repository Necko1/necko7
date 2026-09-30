use super::{Db, error::DbResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttemptParameters {
    pub request_id: Uuid,
    pub max_price: i64,
    pub chance_to_transfer: i16,
    pub trade_link: String,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ManualAttempt {
    pub custom_id: String,
    pub request_id: Uuid,
    pub max_price: i64,
    pub chance_to_transfer: i16,
    pub trade_link: String,
    pub paid_price: Option<i64>,
    pub market_order_id: Option<String>,
    pub status: String,
    pub outcome_kind: Option<String>,
    pub outcome_detail: Option<String>,
    pub last_market_stage: Option<String>,
    pub trade_id: Option<String>,
    pub send_until: Option<DateTime<Utc>>,
    pub receive_until: Option<DateTime<Utc>>,
    pub settlement: Option<DateTime<Utc>>,
    pub causer: Option<String>,
    pub cancellation_reason: Option<String>,
    pub market_refund: Option<serde_json::Value>,
    pub initiator_user_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ManualOrder {
    pub id: Uuid,
    pub inventory_id: Uuid,
    pub channel_id: String,
    pub origin: String,
    pub item_name: String,
    pub currency: String,
    pub trade_link: String,
    pub steam_partner: String,
    pub initial_max_price: i64,
    pub initial_chance_to_transfer: i16,
    pub description: String,
    pub tags: Vec<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub closed_by: Option<String>,
    pub close_reason: Option<String>,
    pub status: String,
    pub can_retry: bool,
    pub can_close: bool,
    pub action_block_reason: Option<String>,
    #[sqlx(skip)]
    pub attempts: Vec<ManualAttempt>,
}

#[derive(Debug, FromRow, Serialize, ToSchema)]
pub struct ManualAuditEvent {
    pub id: i64,
    pub event_key: String,
    pub manual_order_id: Uuid,
    pub event_type: String,
    pub actor_kind: String,
    pub actor_user_id: Option<String>,
    pub attempt_custom_id: Option<String>,
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

macro_rules! manual_select { ($tail:literal) => { concat!("SELECT m.id,i.id AS inventory_id,m.channel_id,m.origin,m.item_name,m.currency,
    m.trade_link,m.steam_partner,m.initial_max_price,m.initial_chance_to_transfer,m.description,m.tags,
    m.created_by,m.created_at,m.updated_at,m.closed_at,m.closed_by,m.close_reason,i.lifecycle_status AS status,
    (m.closed_at IS NULL AND i.lifecycle_status IN ('RETRY_AVAILABLE','INSUFFICIENT_FUNDS','TRADE_LINK_REQUIRED')
        AND EXISTS(SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id=i.id)
        AND NOT EXISTS(SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id=i.id
                       AND a.status NOT IN ('REJECTED','SELLER_FAILED','BUYER_FAILED'))) AS can_retry,
    (m.closed_at IS NULL AND i.lifecycle_status != 'DELIVERED'
        AND NOT EXISTS(SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id=i.id
                       AND NOT (a.status='REJECTED' OR (COALESCE(a.last_market_stage,'')='5'
                           AND a.status IN ('SELLER_FAILED','BUYER_FAILED','TERMINAL_UNCLASSIFIED'))))) AS can_close,
    CASE WHEN m.closed_at IS NOT NULL THEN 'closed'
         WHEN i.lifecycle_status='DELIVERED' THEN 'delivered'
         WHEN i.lifecycle_status='OPERATOR_REVIEW' THEN 'operator_review'
         WHEN i.lifecycle_status='RECONCILIATION_REQUIRED' THEN 'uncertain_market_result'
         WHEN i.lifecycle_status IN ('ORDER_PENDING','TRADE_WAITING','TRADE_ACCEPTED') THEN 'active_delivery'
         WHEN i.lifecycle_status='WAITING_OPERATOR' THEN 'initial_delivery_queued'
         ELSE NULL END AS action_block_reason
    FROM manual_orders m JOIN inventory_items i ON i.manual_order_id=m.id", $tail) }; }

pub enum AttemptClaim {
    New(String),
    Existing(String),
    Blocked,
    Conflict,
}

pub fn fingerprint<T: Serialize>(value: &T) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("serializable request"),
    ))
}

impl Db {
    pub async fn manual_order_by_request(
        &self,
        channel: &str,
        request: Uuid,
    ) -> DbResult<Option<(Uuid, String)>> {
        Ok(sqlx::query_as("SELECT id,request_fingerprint FROM manual_orders WHERE channel_id=$1 AND request_id=$2")
            .bind(channel).bind(request).fetch_optional(self.pool()).await?)
    }

    pub async fn create_manual_order(
        &self,
        channel: &str,
        item: &str,
        currency: &str,
        parameters: &AttemptParameters,
        partner: &str,
        description: &str,
        tags: &[String],
        actor: &str,
        request_fingerprint: &str,
    ) -> DbResult<(Uuid, bool, bool)> {
        let mut tx = self.pool().begin().await?;
        let id = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO manual_orders(id,channel_id,item_name,currency,trade_link,steam_partner,
            initial_max_price,initial_chance_to_transfer,description,tags,created_by,request_id,request_fingerprint)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) ON CONFLICT(channel_id,request_id) DO NOTHING")
            .bind(id).bind(channel).bind(item).bind(currency).bind(&parameters.trade_link).bind(partner)
            .bind(parameters.max_price).bind(parameters.chance_to_transfer).bind(description).bind(tags)
            .bind(actor).bind(parameters.request_id).bind(request_fingerprint).execute(&mut *tx).await?.rows_affected() > 0;
        if !inserted {
            let prior: (Uuid, String) = sqlx::query_as("SELECT id,request_fingerprint FROM manual_orders WHERE channel_id=$1 AND request_id=$2")
                .bind(channel).bind(parameters.request_id).fetch_one(&mut *tx).await?;
            tx.commit().await?;
            return Ok((prior.0, false, prior.1 == request_fingerprint));
        }
        sqlx::query("INSERT INTO inventory_items(id,manual_order_id,item_name,fixed_price,currency,fulfillment_mode,lifecycle_status)
            VALUES($1,$2,$3,$4,$5,'OPERATOR','WAITING_OPERATOR')")
            .bind(Uuid::new_v4()).bind(id).bind(item).bind(parameters.max_price).bind(currency).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO fulfillment_audit_events(event_key,manual_order_id,event_type,actor_kind,actor_user_id)
            VALUES($1,$2,'manual_order_created','operator',$3)")
            .bind(format!("manual:{id}:created")).bind(id).bind(actor).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok((id, true, true))
    }

    pub async fn get_manual_order(&self, channel: &str, id: Uuid) -> DbResult<Option<ManualOrder>> {
        let mut order: Option<ManualOrder> =
            sqlx::query_as(manual_select!(" WHERE m.channel_id=$1 AND m.id=$2"))
                .bind(channel)
                .bind(id)
                .fetch_optional(self.pool())
                .await?;
        if let Some(order) = order.as_mut() {
            order.attempts = self.manual_attempts(order.inventory_id).await?;
        }
        Ok(order)
    }

    pub async fn list_manual_orders(
        &self,
        channel: &str,
        search: &str,
        status: Option<&str>,
        tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<(Vec<ManualOrder>, i64)> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM manual_orders m JOIN inventory_items i ON i.manual_order_id=m.id WHERE m.channel_id=$1 AND ($2='' OR POSITION(lower($2) IN lower(m.item_name || ' ' || m.id::text || ' ' || m.steam_partner || ' ' || m.description))>0)
            AND ($3::TEXT IS NULL OR i.lifecycle_status=$3) AND ($4::TEXT IS NULL OR $4=ANY(m.tags))")
            .bind(channel).bind(search).bind(status).bind(tag).fetch_one(self.pool()).await?;
        let mut orders: Vec<ManualOrder> = sqlx::query_as(manual_select!(" WHERE m.channel_id=$1 AND ($2='' OR POSITION(lower($2) IN lower(m.item_name || ' ' || m.id::text || ' ' || m.steam_partner || ' ' || m.description))>0)
            AND ($3::TEXT IS NULL OR i.lifecycle_status=$3) AND ($4::TEXT IS NULL OR $4=ANY(m.tags)) ORDER BY m.created_at DESC,m.id DESC LIMIT $5 OFFSET $6"))
            .bind(channel).bind(search).bind(status).bind(tag).bind(limit).bind(offset).fetch_all(self.pool()).await?;
        for order in &mut orders {
            order.attempts = self.manual_attempts(order.inventory_id).await?;
        }
        Ok((orders, count))
    }

    pub async fn manual_attempts(&self, inventory: Uuid) -> DbResult<Vec<ManualAttempt>> {
        Ok(sqlx::query_as("SELECT custom_id,request_id,max_price,chance_to_transfer,trade_link,paid_price,market_order_id,
            status,outcome_kind,outcome_detail,last_market_stage,trade_id,send_until,receive_until,settlement,causer,cancellation_reason,
            market_refund,initiator_user_id,created_at,last_checked_at FROM inventory_order_attempts WHERE inventory_id=$1 ORDER BY attempt_id")
            .bind(inventory).fetch_all(self.pool()).await?)
    }

    pub async fn manual_audit(&self, channel: &str, id: Uuid) -> DbResult<Vec<ManualAuditEvent>> {
        Ok(sqlx::query_as("SELECT e.id,e.event_key,e.manual_order_id,e.event_type,e.actor_kind,e.actor_user_id,
            e.attempt_custom_id,e.details,e.created_at FROM fulfillment_audit_events e JOIN manual_orders m ON m.id=e.manual_order_id
            WHERE m.channel_id=$1 AND m.id=$2 ORDER BY e.id")
            .bind(channel).bind(id).fetch_all(self.pool()).await?)
    }

    pub async fn begin_manual_attempt(
        &self,
        inventory: Uuid,
        parameters: &AttemptParameters,
        partner: &str,
        actor: &str,
        initial: bool,
    ) -> DbResult<AttemptClaim> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query("SELECT i.manual_order_id,i.lifecycle_status,m.closed_at,m.initial_max_price,
            m.initial_chance_to_transfer,m.trade_link,m.request_id FROM inventory_items i JOIN manual_orders m ON m.id=i.manual_order_id
            WHERE i.id=$1 FOR UPDATE OF i")
            .bind(inventory).fetch_one(&mut *tx).await?;
        let order_id: Uuid = row.try_get("manual_order_id")?;
        let hash = fingerprint(parameters);
        let prior: Option<(String, String)> = sqlx::query_as("SELECT custom_id,request_fingerprint FROM inventory_order_attempts WHERE inventory_id=$1 AND request_id=$2")
            .bind(inventory).bind(parameters.request_id).fetch_optional(&mut *tx).await?;
        if let Some((custom, prior_hash)) = prior {
            return Ok(if prior_hash == hash {
                AttemptClaim::Existing(custom)
            } else {
                AttemptClaim::Conflict
            });
        }
        let status: String = row.try_get("lifecycle_status")?;
        let closed: Option<DateTime<Utc>> = row.try_get("closed_at")?;
        let attempts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM inventory_order_attempts WHERE inventory_id=$1",
        )
        .bind(inventory)
        .fetch_one(&mut *tx)
        .await?;
        let unresolved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_order_attempts WHERE inventory_id=$1 AND status NOT IN ('REJECTED','SELLER_FAILED','BUYER_FAILED'))")
            .bind(inventory).fetch_one(&mut *tx).await?;
        let allowed = if initial {
            attempts == 0
                && status == "WAITING_OPERATOR"
                && parameters.max_price == row.try_get::<i64, _>("initial_max_price")?
                && parameters.chance_to_transfer
                    == row.try_get::<i16, _>("initial_chance_to_transfer")?
                && parameters.trade_link == row.try_get::<String, _>("trade_link")?
                && parameters.request_id == row.try_get::<Uuid, _>("request_id")?
        } else {
            attempts > 0
                && [
                    "RETRY_AVAILABLE",
                    "INSUFFICIENT_FUNDS",
                    "TRADE_LINK_REQUIRED",
                ]
                .contains(&status.as_str())
        };
        if closed.is_some() || unresolved || !allowed {
            return Ok(AttemptClaim::Blocked);
        }
        let custom = format!("manual-{order_id}-{attempts}");
        sqlx::query("INSERT INTO inventory_order_attempts(custom_id,inventory_id,item_name,max_price,trade_link,
            chance_to_transfer,request_id,request_fingerprint,status,next_poll_at,initiator_kind,initiator_user_id)
            SELECT $2,id,item_name,$3,$4,$5,$6,$7,'CALLING',NOW(),'operator',$8 FROM inventory_items WHERE id=$1")
            .bind(inventory).bind(&custom).bind(parameters.max_price).bind(&parameters.trade_link)
            .bind(parameters.chance_to_transfer).bind(parameters.request_id).bind(&hash).bind(actor).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_items SET lifecycle_status='ORDER_PENDING',market_custom_id=$2,market_order_id=NULL,last_action_at=NOW() WHERE id=$1")
            .bind(inventory).bind(&custom).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE manual_orders SET trade_link=$2,steam_partner=$3,updated_at=NOW() WHERE id=$1",
        )
        .bind(order_id)
        .bind(&parameters.trade_link)
        .bind(partner)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(AttemptClaim::New(custom))
    }

    pub async fn queued_manual_orders(&self) -> DbResult<Vec<(String, Uuid)>> {
        Ok(sqlx::query_as("SELECT m.channel_id,m.id FROM manual_orders m JOIN inventory_items i ON i.manual_order_id=m.id
            WHERE m.closed_at IS NULL AND i.lifecycle_status='WAITING_OPERATOR'
            AND NOT EXISTS(SELECT 1 FROM inventory_order_attempts a WHERE a.inventory_id=i.id)
            ORDER BY m.created_at LIMIT 100").fetch_all(self.pool()).await?)
    }

    pub async fn manual_initial_parameters(&self, id: Uuid) -> DbResult<AttemptParameters> {
        #[derive(FromRow)]
        struct Parameters {
            request_id: Uuid,
            max_price: i64,
            chance_to_transfer: i16,
            trade_link: String,
        }
        let row: Parameters = sqlx::query_as("SELECT request_id,initial_max_price AS max_price,initial_chance_to_transfer AS chance_to_transfer,trade_link FROM manual_orders WHERE id=$1")
            .bind(id).fetch_one(self.pool()).await?;
        Ok(AttemptParameters {
            request_id: row.request_id,
            max_price: row.max_price,
            chance_to_transfer: row.chance_to_transfer,
            trade_link: row.trade_link,
        })
    }

    pub async fn update_manual_metadata(
        &self,
        channel: &str,
        id: Uuid,
        description: &str,
        tags: &[String],
        actor: &str,
    ) -> DbResult<bool> {
        let mut tx = self.pool().begin().await?;
        let old: Option<(String, Vec<String>)> = sqlx::query_as(
            "SELECT description,tags FROM manual_orders WHERE channel_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(channel)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(old) = old else {
            return Ok(false);
        };
        if old.0 != description || old.1 != tags {
            sqlx::query(
                "UPDATE manual_orders SET description=$2,tags=$3,updated_at=NOW() WHERE id=$1",
            )
            .bind(id)
            .bind(description)
            .bind(tags)
            .execute(&mut *tx)
            .await?;
            sqlx::query("INSERT INTO fulfillment_audit_events(event_key,manual_order_id,event_type,actor_kind,actor_user_id,details)
                VALUES($1,$2,'manual_metadata_updated','operator',$3,$4)")
                .bind(format!("manual:{id}:metadata:{}", Uuid::new_v4())).bind(id).bind(actor)
                .bind(serde_json::json!({"old_description":old.0,"description":description,"old_tags":old.1,"tags":tags})).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn close_manual_order(
        &self,
        channel: &str,
        id: Uuid,
        reason: &str,
        actor: &str,
    ) -> DbResult<bool> {
        let mut tx = self.pool().begin().await?;
        let inventory: Option<(Uuid, String)> = sqlx::query_as("SELECT i.id,i.lifecycle_status FROM inventory_items i JOIN manual_orders m ON m.id=i.manual_order_id
            WHERE m.channel_id=$1 AND m.id=$2 AND m.closed_at IS NULL FOR UPDATE OF i")
            .bind(channel).bind(id).fetch_optional(&mut *tx).await?;
        let Some((inventory, status)) = inventory else {
            return Ok(false);
        };
        let unsafe_attempt: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_order_attempts WHERE inventory_id=$1
            AND NOT (status='REJECTED' OR (COALESCE(last_market_stage,'')='5' AND status IN ('SELLER_FAILED','BUYER_FAILED','TERMINAL_UNCLASSIFIED'))))")
            .bind(inventory).fetch_one(&mut *tx).await?;
        if status == "DELIVERED" || unsafe_attempt || reason.trim().is_empty() {
            return Ok(false);
        }
        let changed = sqlx::query("UPDATE manual_orders SET closed_at=NOW(),closed_by=$2,close_reason=$3,updated_at=NOW() WHERE id=$1 AND closed_at IS NULL")
            .bind(id).bind(actor).bind(reason).execute(&mut *tx).await?.rows_affected();
        if changed == 0 {
            return Ok(false);
        }
        sqlx::query("UPDATE inventory_items SET lifecycle_status='CANCELLED' WHERE id=$1")
            .bind(inventory)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO fulfillment_audit_events(event_key,manual_order_id,inventory_id,event_type,actor_kind,actor_user_id,details)
            VALUES($1,$2,$3,'manual_order_closed','operator',$4,jsonb_build_object('reason',$5::TEXT))")
            .bind(format!("manual:{id}:closed")).bind(id).bind(inventory).bind(actor).bind(reason).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn delivery_source(
        &self,
        inventory: Uuid,
    ) -> DbResult<(String, Option<Uuid>, Option<Uuid>)> {
        Ok(sqlx::query_as("SELECT COALESCE(m.channel_id,rw.streamer_id),i.redemption_id,i.manual_order_id FROM inventory_items i
            LEFT JOIN manual_orders m ON m.id=i.manual_order_id LEFT JOIN redemptions r ON r.fulfillment_id=i.redemption_id
            LEFT JOIN rewards rw ON rw.twitch_id=r.twitch_reward_id WHERE i.id=$1")
            .bind(inventory).fetch_one(self.pool()).await?)
    }
}
