CREATE OR REPLACE FUNCTION audit_inventory_change() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE event_name TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO fulfillment_audit_events(event_key, redemption_id, inventory_id, event_type, actor_kind)
        VALUES ('inventory:' || NEW.id || ':created', NEW.redemption_id, NEW.id, 'inventory_created', 'system')
        ON CONFLICT DO NOTHING;
        RETURN NEW;
    END IF;
    IF NEW.lifecycle_status = 'DELIVERED' AND OLD.lifecycle_status IS DISTINCT FROM 'DELIVERED'
       AND NEW.twitch_fulfilled_at IS NULL
       AND EXISTS(SELECT 1 FROM redemptions r WHERE r.fulfillment_id=NEW.redemption_id AND r.origin='TWITCH') THEN
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

-- Every subsequent market/inventory audit event keeps its initiating script correlation.
-- Explicit human actors remain human; automatic work inherits script attribution.
CREATE FUNCTION attribute_script_fulfillment_audit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE attribution JSONB;
BEGIN
 SELECT jsonb_build_object('origin',origin,'script_project_id',script_project_id,
  'script_revision',script_revision,'script_execution_id',script_execution_id)
 INTO attribution FROM redemptions WHERE fulfillment_id=NEW.redemption_id AND origin='SCRIPT';
 IF attribution IS NOT NULL THEN
  NEW.details=NEW.details || attribution;
  IF NEW.actor_kind='system' THEN NEW.actor_kind='script'; END IF;
 END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER script_fulfillment_audit_attribution BEFORE INSERT ON fulfillment_audit_events
 FOR EACH ROW EXECUTE FUNCTION attribute_script_fulfillment_audit();

CREATE INDEX redemptions_script_rate ON redemptions(script_project_id,created_at DESC) WHERE script_project_id IS NOT NULL;
