-- New events are recorded only when a real transition happens after this migration.
-- Historical rows are intentionally not reconstructed from their current state.
CREATE TABLE fulfillment_audit_events (
    id BIGSERIAL UNIQUE,
    event_key TEXT PRIMARY KEY,
    redemption_id UUID NOT NULL REFERENCES redemptions(twitch_redemption_id),
    inventory_id UUID REFERENCES inventory_items(id),
    attempt_custom_id TEXT REFERENCES inventory_order_attempts(custom_id),
    event_type TEXT NOT NULL,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('system', 'viewer', 'operator')),
    actor_user_id TEXT,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX fulfillment_audit_redemption_idx ON fulfillment_audit_events(redemption_id, created_at, id);

CREATE FUNCTION protect_fulfillment_audit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Fulfillment audit events are append-only';
END;
$$;
CREATE TRIGGER fulfillment_audit_append_only BEFORE UPDATE OR DELETE ON fulfillment_audit_events
FOR EACH ROW EXECUTE FUNCTION protect_fulfillment_audit();

ALTER TABLE inventory_order_attempts ADD COLUMN initiator_kind TEXT NOT NULL DEFAULT 'system'
    CHECK (initiator_kind IN ('system', 'viewer', 'operator'));
ALTER TABLE inventory_order_attempts ADD COLUMN initiator_user_id TEXT;

CREATE FUNCTION audit_redemption_insert() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO fulfillment_audit_events(event_key, redemption_id, event_type, actor_kind, actor_user_id)
    VALUES ('redemption:' || NEW.twitch_redemption_id || ':redeemed', NEW.twitch_redemption_id,
            'reward_redeemed', 'viewer', NEW.user_id) ON CONFLICT DO NOTHING;
    RETURN NEW;
END;
$$;
CREATE TRIGGER redemption_fulfillment_audit AFTER INSERT ON redemptions
FOR EACH ROW EXECUTE FUNCTION audit_redemption_insert();

CREATE FUNCTION audit_inventory_change() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE event_name TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':created', NEW.redemption_id, NEW.id, 'inventory_created', 'system')
        ON CONFLICT DO NOTHING;
        RETURN NEW;
    END IF;
    IF NEW.lifecycle_status = 'DELIVERED' AND OLD.lifecycle_status IS DISTINCT FROM 'DELIVERED'
       AND NEW.twitch_fulfilled_at IS NULL THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':twitch-pending', NEW.redemption_id, NEW.id, 'twitch_fulfillment_pending', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.twitch_fulfilled_at IS NOT NULL AND OLD.twitch_fulfilled_at IS NULL THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':twitch-fulfilled', NEW.redemption_id, NEW.id, 'twitch_fulfillment_succeeded', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.lifecycle_status = 'REFUNDED' AND OLD.lifecycle_status = 'REFUNDING' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':twitch-refunded', NEW.redemption_id, NEW.id, 'twitch_refund_succeeded', 'system')
        ON CONFLICT DO NOTHING;
    ELSIF NEW.lifecycle_status = 'RECONCILIATION_REQUIRED' AND OLD.lifecycle_status = 'REFUNDING' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':twitch-refund-uncertain', NEW.redemption_id, NEW.id, 'twitch_refund_uncertain', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER inventory_fulfillment_audit AFTER INSERT OR UPDATE ON inventory_items
FOR EACH ROW EXECUTE FUNCTION audit_inventory_change();

CREATE FUNCTION audit_market_attempt_change() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE redemption UUID;
DECLARE event_base TEXT;
DECLARE event_name TEXT;
DECLARE prior_count BIGINT;
BEGIN
    SELECT redemption_id INTO redemption FROM inventory_items WHERE id = NEW.inventory_id;
    event_base := 'attempt:' || NEW.custom_id || ':';
    IF TG_OP = 'INSERT' THEN
        event_name := CASE NEW.initiator_kind
            WHEN 'viewer' THEN 'viewer_order_requested'
            WHEN 'operator' THEN 'operator_order_requested'
            ELSE 'automatic_order_initiated' END;
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id,
                                             event_type, actor_kind, actor_user_id)
        VALUES (event_base || 'requested', redemption, NEW.inventory_id, NEW.custom_id,
                event_name, NEW.initiator_kind, NEW.initiator_user_id) ON CONFLICT DO NOTHING;
        SELECT COUNT(*) INTO prior_count FROM inventory_order_attempts
        WHERE inventory_id = NEW.inventory_id AND custom_id != NEW.custom_id;
        IF prior_count > 0 THEN
            INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id,
                                                 event_type, actor_kind, actor_user_id)
            VALUES (event_base || 'retry', redemption, NEW.inventory_id, NEW.custom_id,
                    'retry_attempt_created', NEW.initiator_kind, NEW.initiator_user_id) ON CONFLICT DO NOTHING;
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.status IN ('ORDER_CREATED','TRADE_WAITING','TRADE_ACCEPTED','DELIVERED')
        OR NEW.last_market_stage IN ('1','2','5'))
       AND OLD.status IN ('CALLING','RECONCILIATION_REQUIRED') THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id, event_type, actor_kind)
        VALUES (event_base || 'order-created', redemption, NEW.inventory_id, NEW.custom_id, 'market_order_created', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.trade_created_at IS NOT NULL AND OLD.trade_created_at IS NULL THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id, event_type, actor_kind)
        VALUES (event_base || 'trade-created', redemption, NEW.inventory_id, NEW.custom_id, 'steam_trade_created', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.settlement IS NOT NULL AND OLD.settlement IS NULL THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id, event_type, actor_kind)
        VALUES (event_base || 'trade-accepted', redemption, NEW.inventory_id, NEW.custom_id, 'buyer_accepted_trade', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.status = 'DELIVERED' AND OLD.status != 'DELIVERED' AND NEW.last_market_stage = '2' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id, event_type, actor_kind)
        VALUES (event_base || 'delivered', redemption, NEW.inventory_id, NEW.custom_id, 'market_stage_2_delivered', 'system')
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.status = 'REJECTED' AND OLD.status != 'REJECTED' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id,
                                             event_type, actor_kind, details)
        VALUES (event_base || 'rejected', redemption, NEW.inventory_id, NEW.custom_id,
                'market_order_rejected', 'system', jsonb_build_object('outcome', NEW.outcome_kind))
        ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.last_market_stage = '5' AND OLD.last_market_stage IS DISTINCT FROM '5' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id,
                                             event_type, actor_kind)
        VALUES (event_base || 'terminal', redemption, NEW.inventory_id, NEW.custom_id,
                COALESCE(NEW.outcome_kind, 'terminal_unclassified'), 'system') ON CONFLICT DO NOTHING;
    END IF;
    IF NEW.status = 'RECONCILIATION_REQUIRED' AND OLD.status != 'RECONCILIATION_REQUIRED' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, attempt_custom_id, event_type, actor_kind)
        VALUES (event_base || 'reconciliation', redemption, NEW.inventory_id, NEW.custom_id,
                'market_reconciliation_required', 'system') ON CONFLICT DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER market_attempt_fulfillment_audit AFTER INSERT OR UPDATE ON inventory_order_attempts
FOR EACH ROW EXECUTE FUNCTION audit_market_attempt_change();
