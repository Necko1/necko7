-- Inventory is one immutable economic position. Old rows cannot be presumed safe
-- to retry: prior attempt outcomes were not persisted.
ALTER TABLE redemptions ADD COLUMN inventory_resolution_claimed_at TIMESTAMPTZ;
ALTER TABLE inventory_items ADD COLUMN fulfillment_mode VARCHAR(24) NOT NULL DEFAULT 'LEGACY_REVIEW'
    CHECK (fulfillment_mode IN ('AUTO', 'VIEWER', 'OPERATOR', 'LEGACY_REVIEW'));
ALTER TABLE inventory_items ADD COLUMN lifecycle_status VARCHAR(32) NOT NULL DEFAULT 'RECONCILIATION_REQUIRED'
    CHECK (lifecycle_status IN ('WAITING_VIEWER', 'WAITING_OPERATOR', 'TRADE_LINK_REQUIRED',
        'ORDER_PENDING', 'TRADE_WAITING', 'RETRY_AVAILABLE', 'INSUFFICIENT_FUNDS',
        'RECONCILIATION_REQUIRED', 'DELIVERED', 'REFUNDING', 'REFUNDED'));
ALTER TABLE inventory_items ADD COLUMN buyer_retry_allowed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE inventory_items ADD COLUMN last_action_at TIMESTAMPTZ;
-- Local delivery and the subsequent Twitch fulfillment call cross a process boundary.
-- A null marker on a delivered live item is a durable recovery task.
ALTER TABLE inventory_items ADD COLUMN twitch_fulfilled_at TIMESTAMPTZ;

ALTER TABLE inventory_order_attempts ADD COLUMN status VARCHAR(32) NOT NULL DEFAULT 'RECONCILIATION_REQUIRED'
    CHECK (status IN ('CALLING', 'REJECTED', 'ORDER_CREATED', 'TRADE_WAITING',
        'SELLER_FAILED', 'BUYER_FAILED', 'DELIVERED', 'RECONCILIATION_REQUIRED'));
ALTER TABLE inventory_order_attempts ADD COLUMN outcome_kind TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN outcome_detail TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN trade_link TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN trade_id TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN causer TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN cancellation_reason TEXT;
ALTER TABLE inventory_order_attempts ADD COLUMN send_until TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN receive_until TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN last_checked_at TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN resolved_at TIMESTAMPTZ;

-- A unique unresolved attempt also protects against parallel explicit actions.
CREATE UNIQUE INDEX inventory_one_live_attempt ON inventory_order_attempts(inventory_id)
    WHERE status IN ('CALLING', 'ORDER_CREATED', 'TRADE_WAITING');

CREATE FUNCTION protect_inventory_attempt_identity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE item_name_snapshot TEXT;
DECLARE mode_snapshot VARCHAR(24);
BEGIN
    SELECT item_name, fulfillment_mode INTO item_name_snapshot, mode_snapshot
    FROM inventory_items WHERE id = NEW.inventory_id;
    IF TG_OP = 'UPDATE' THEN
        IF NEW.custom_id IS DISTINCT FROM OLD.custom_id OR NEW.inventory_id IS DISTINCT FROM OLD.inventory_id
           OR NEW.item_name IS DISTINCT FROM OLD.item_name OR NEW.max_price IS DISTINCT FROM OLD.max_price
           OR NEW.trade_link IS DISTINCT FROM OLD.trade_link THEN
            RAISE EXCEPTION 'Market attempt identity, ceiling and destination are immutable';
        END IF;
    END IF;
    IF mode_snapshot != 'LEGACY_REVIEW' AND NEW.item_name IS DISTINCT FROM item_name_snapshot THEN
        RAISE EXCEPTION 'Market attempt item must match inventory item';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER inventory_attempt_identity BEFORE INSERT OR UPDATE ON inventory_order_attempts
FOR EACH ROW EXECUTE FUNCTION protect_inventory_attempt_identity();

-- The earlier filter fallback could save candidate B's name with candidate A's
-- fixed price. That cannot be repaired reliably after delivery; keep the raw
-- historical row in LEGACY_REVIEW instead of inventing a corrected identity.

UPDATE inventory_items i SET lifecycle_status = CASE
    WHEN r.status = 'COMPLETED' THEN 'DELIVERED'
    WHEN r.status = 'FAILED_REFUND' THEN 'REFUNDED'
    ELSE 'RECONCILIATION_REQUIRED' END
FROM redemptions r WHERE r.twitch_redemption_id = i.redemption_id;
UPDATE inventory_items SET twitch_fulfilled_at = NOW()
WHERE fulfillment_mode = 'LEGACY_REVIEW' AND lifecycle_status = 'DELIVERED';
UPDATE inventory_order_attempts a SET status = 'DELIVERED'
FROM inventory_items i WHERE a.inventory_id = i.id AND i.lifecycle_status = 'DELIVERED'
    AND a.custom_id = i.market_custom_id;

CREATE OR REPLACE FUNCTION protect_inventory_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.redemption_id IS DISTINCT FROM OLD.redemption_id
       OR NEW.viewer_id IS DISTINCT FROM OLD.viewer_id
       OR NEW.item_name IS DISTINCT FROM OLD.item_name
       OR NEW.fixed_price IS DISTINCT FROM OLD.fixed_price
       OR NEW.currency IS DISTINCT FROM OLD.currency
       OR NEW.fulfillment_mode IS DISTINCT FROM OLD.fulfillment_mode
       OR NEW.buyer_retry_allowed IS DISTINCT FROM OLD.buyer_retry_allowed THEN
        RAISE EXCEPTION 'Inventory identity, value and fulfillment policy are immutable';
    END IF;
    RETURN NEW;
END;
$$;

-- A profile setting does not require a local users row: Twitch EventSub provides
-- the stable user ID before the viewer has ever authenticated here.
CREATE TABLE viewer_settings (
    viewer_id VARCHAR(255) PRIMARY KEY,
    auto_buy_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    trade_link TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE rewards ADD COLUMN retry_on_buyer_failure BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE broadcaster_settings DROP COLUMN refund_on_buyer_fail;
ALTER TABLE broadcaster_settings DROP COLUMN refund_if_no_money;
