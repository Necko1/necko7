-- Manual fulfillment has its own identity; no fake redemption or viewer is needed.
CREATE TABLE manual_orders (
    id UUID PRIMARY KEY,
    channel_id VARCHAR(255) NOT NULL REFERENCES broadcasters(channel_id),
    origin TEXT NOT NULL DEFAULT 'MANUAL' CHECK (origin = 'MANUAL'),
    item_name TEXT NOT NULL,
    currency TEXT NOT NULL,
    trade_link TEXT NOT NULL,
    steam_partner TEXT NOT NULL,
    initial_max_price BIGINT NOT NULL CHECK (initial_max_price BETWEEN 1 AND 2147483647),
    initial_chance_to_transfer SMALLINT NOT NULL CHECK (initial_chance_to_transfer BETWEEN 0 AND 100),
    description TEXT NOT NULL DEFAULT '',
    tags TEXT[] NOT NULL DEFAULT '{}',
    created_by TEXT NOT NULL,
    request_id UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ,
    closed_by TEXT,
    close_reason TEXT,
    UNIQUE (channel_id, request_id),
    CHECK ((closed_at IS NULL AND closed_by IS NULL AND close_reason IS NULL)
        OR (closed_at IS NOT NULL AND closed_by IS NOT NULL AND close_reason IS NOT NULL AND length(trim(close_reason)) > 0))
);
CREATE INDEX manual_orders_channel_created ON manual_orders(channel_id, created_at DESC);
CREATE INDEX manual_orders_tags ON manual_orders USING GIN(tags);

ALTER TABLE inventory_items ALTER COLUMN redemption_id DROP NOT NULL;
ALTER TABLE inventory_items ALTER COLUMN viewer_id DROP NOT NULL;
ALTER TABLE inventory_items ADD COLUMN manual_order_id UUID UNIQUE REFERENCES manual_orders(id);
ALTER TABLE inventory_items ADD CONSTRAINT inventory_source_consistent CHECK (
    (redemption_id IS NOT NULL AND viewer_id IS NOT NULL AND manual_order_id IS NULL)
    OR (redemption_id IS NULL AND viewer_id IS NULL AND manual_order_id IS NOT NULL
        AND fulfillment_mode = 'OPERATOR' AND twitch_fulfilled_at IS NULL
        AND twitch_fulfillment_claimed_at IS NULL)
);
ALTER TABLE inventory_items DROP CONSTRAINT inventory_items_lifecycle_status_check;
ALTER TABLE inventory_items ADD CONSTRAINT inventory_items_lifecycle_status_check CHECK (
    lifecycle_status IN ('WAITING_VIEWER','WAITING_OPERATOR','TRADE_LINK_REQUIRED','ORDER_PENDING',
        'TRADE_WAITING','TRADE_ACCEPTED','RETRY_AVAILABLE','INSUFFICIENT_FUNDS',
        'RECONCILIATION_REQUIRED','OPERATOR_REVIEW','DELIVERED','REFUNDING','REFUNDED','DISCARDED','CANCELLED')
);
ALTER TABLE inventory_items ADD CHECK (lifecycle_status != 'CANCELLED' OR manual_order_id IS NOT NULL);

CREATE OR REPLACE FUNCTION validate_inventory_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.manual_order_id IS NOT NULL THEN
        IF NEW.redemption_id IS NOT NULL OR NEW.viewer_id IS NOT NULL
           OR NOT EXISTS (SELECT 1 FROM manual_orders m WHERE m.id=NEW.manual_order_id
                          AND m.item_name=NEW.item_name AND m.currency=NEW.currency) THEN
            RAISE EXCEPTION 'Manual inventory must match its order and have no viewer or redemption';
        END IF;
    ELSIF NOT EXISTS (SELECT 1 FROM redemptions r WHERE r.fulfillment_id=NEW.redemption_id AND r.user_id=NEW.viewer_id) THEN
        RAISE EXCEPTION 'Inventory owner must match redemption viewer';
    END IF;
    RETURN NEW;
END; $$;

CREATE OR REPLACE FUNCTION protect_inventory_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.redemption_id IS DISTINCT FROM OLD.redemption_id
       OR NEW.manual_order_id IS DISTINCT FROM OLD.manual_order_id
       OR NEW.viewer_id IS DISTINCT FROM OLD.viewer_id
       OR NEW.item_name IS DISTINCT FROM OLD.item_name
       OR NEW.fixed_price IS DISTINCT FROM OLD.fixed_price
       OR NEW.currency IS DISTINCT FROM OLD.currency
       OR NEW.fulfillment_mode IS DISTINCT FROM OLD.fulfillment_mode
       OR NEW.buyer_retry_allowed IS DISTINCT FROM OLD.buyer_retry_allowed THEN
        RAISE EXCEPTION 'Inventory identity, value and fulfillment policy are immutable';
    END IF;
    RETURN NEW;
END; $$;

-- Historical attempts have no reliable chance snapshot; do not invent one.
ALTER TABLE inventory_order_attempts ADD COLUMN chance_to_transfer SMALLINT CHECK (chance_to_transfer BETWEEN 0 AND 100);
ALTER TABLE inventory_order_attempts ADD COLUMN paid_price BIGINT;
ALTER TABLE inventory_order_attempts ADD COLUMN request_id UUID;
ALTER TABLE inventory_order_attempts ADD COLUMN request_fingerprint TEXT;
CREATE UNIQUE INDEX inventory_attempt_request ON inventory_order_attempts(inventory_id, request_id) WHERE request_id IS NOT NULL;
-- New manual attempts also treat an uncertain result as a live purchase. Do
-- not tighten historical Twitch/Script rows whose old evidence may be unknown.
CREATE UNIQUE INDEX manual_inventory_one_unresolved_attempt ON inventory_order_attempts(inventory_id)
    WHERE request_id IS NOT NULL AND status IN ('CALLING','ORDER_CREATED','TRADE_WAITING','TRADE_ACCEPTED','RECONCILIATION_REQUIRED');

CREATE OR REPLACE FUNCTION protect_inventory_attempt_identity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE item_name_snapshot TEXT;
DECLARE mode_snapshot VARCHAR(24);
BEGIN
    SELECT item_name, fulfillment_mode INTO item_name_snapshot, mode_snapshot FROM inventory_items WHERE id=NEW.inventory_id;
    IF TG_OP='UPDATE' AND (NEW.custom_id IS DISTINCT FROM OLD.custom_id
       OR NEW.inventory_id IS DISTINCT FROM OLD.inventory_id
       OR NEW.item_name IS DISTINCT FROM OLD.item_name
       OR NEW.max_price IS DISTINCT FROM OLD.max_price
       OR NEW.trade_link IS DISTINCT FROM OLD.trade_link
       OR NEW.chance_to_transfer IS DISTINCT FROM OLD.chance_to_transfer
       OR NEW.request_id IS DISTINCT FROM OLD.request_id
       OR NEW.request_fingerprint IS DISTINCT FROM OLD.request_fingerprint) THEN
        RAISE EXCEPTION 'Market attempt identity, parameters and destination are immutable';
    END IF;
    IF mode_snapshot != 'LEGACY_REVIEW' AND NEW.item_name IS DISTINCT FROM item_name_snapshot THEN
        RAISE EXCEPTION 'Market attempt item must match inventory item';
    END IF;
    IF EXISTS (SELECT 1 FROM inventory_items WHERE id=NEW.inventory_id AND manual_order_id IS NOT NULL)
       AND (NEW.chance_to_transfer IS NULL OR NEW.request_id IS NULL OR NEW.request_fingerprint IS NULL
            OR NEW.max_price IS NULL OR NEW.max_price NOT BETWEEN 1 AND 2147483647) THEN
        RAISE EXCEPTION 'Manual attempts require parameter and request snapshots';
    END IF;
    RETURN NEW;
END; $$;

ALTER TABLE fulfillment_audit_events ALTER COLUMN redemption_id DROP NOT NULL;
ALTER TABLE fulfillment_audit_events ADD COLUMN manual_order_id UUID REFERENCES manual_orders(id);
ALTER TABLE fulfillment_audit_events ADD CHECK ((redemption_id IS NOT NULL) <> (manual_order_id IS NOT NULL));
CREATE INDEX fulfillment_audit_manual ON fulfillment_audit_events(manual_order_id, id);

-- Existing transition triggers remain shared. Attribute manual events before the
-- source constraint is checked, and expose only safe attempt parameters in audit.
CREATE FUNCTION attribute_manual_fulfillment_audit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE snapshot RECORD;
BEGIN
    IF NEW.inventory_id IS NOT NULL THEN
        SELECT manual_order_id INTO NEW.manual_order_id FROM inventory_items WHERE id=NEW.inventory_id;
    END IF;
    IF NEW.manual_order_id IS NOT NULL THEN
        NEW.details=NEW.details || jsonb_build_object('origin','MANUAL');
        IF NEW.attempt_custom_id IS NOT NULL THEN
            SELECT max_price,chance_to_transfer,paid_price INTO snapshot FROM inventory_order_attempts WHERE custom_id=NEW.attempt_custom_id;
            NEW.details=NEW.details || jsonb_build_object('max_price',snapshot.max_price,
                'chance_to_transfer',snapshot.chance_to_transfer,'paid_price',snapshot.paid_price);
        END IF;
    END IF;
    RETURN NEW;
END; $$;
CREATE TRIGGER manual_fulfillment_audit_attribution BEFORE INSERT ON fulfillment_audit_events
FOR EACH ROW EXECUTE FUNCTION attribute_manual_fulfillment_audit();

-- The per-order audit is append-only and permanent; the normal log remains
-- subject to existing retention. Mirror events transactionally, without tokens.
CREATE FUNCTION log_manual_fulfillment_audit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.manual_order_id IS NOT NULL THEN
        INSERT INTO channel_logs(broadcaster_id,level,category,event_type,message,details,created_at)
        SELECT channel_id,
            CASE WHEN NEW.event_type IN ('market_order_rejected','market_reconciliation_required','buyer_reverted',
                'seller_reverted','buyer_not_accepted','seller_not_sent','seller_cancelled','terminal_unclassified') THEN 'WARN' ELSE 'INFO' END,
            'MANUAL', NEW.event_type, 'Manual order ' || m.id || ': ' || NEW.event_type,
            jsonb_build_object('manual_order_id',m.id,'origin','MANUAL','item_name',m.item_name,
                'attempt_custom_id',NEW.attempt_custom_id,'actor_user_id',NEW.actor_user_id), NEW.created_at
        FROM manual_orders m WHERE m.id=NEW.manual_order_id;
    END IF;
    RETURN NEW;
END; $$;
CREATE TRIGGER manual_fulfillment_channel_log AFTER INSERT ON fulfillment_audit_events
FOR EACH ROW EXECUTE FUNCTION log_manual_fulfillment_audit();

CREATE FUNCTION protect_manual_order_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id OR NEW.channel_id IS DISTINCT FROM OLD.channel_id
       OR NEW.origin IS DISTINCT FROM OLD.origin OR NEW.item_name IS DISTINCT FROM OLD.item_name
       OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.request_id IS DISTINCT FROM OLD.request_id
       OR NEW.request_fingerprint IS DISTINCT FROM OLD.request_fingerprint
       OR NEW.initial_max_price IS DISTINCT FROM OLD.initial_max_price
       OR NEW.initial_chance_to_transfer IS DISTINCT FROM OLD.initial_chance_to_transfer THEN
        RAISE EXCEPTION 'Manual order identity and initial parameters are immutable';
    END IF;
    RETURN NEW;
END; $$;
CREATE TRIGGER manual_order_identity BEFORE UPDATE ON manual_orders
FOR EACH ROW EXECUTE FUNCTION protect_manual_order_identity();
