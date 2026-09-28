-- Preserve existing economic identities and all referencing foreign keys.
ALTER TABLE redemptions RENAME COLUMN twitch_redemption_id TO fulfillment_id;
ALTER TABLE redemptions ADD COLUMN twitch_redemption_id UUID UNIQUE;
ALTER TABLE redemptions ADD COLUMN origin TEXT NOT NULL DEFAULT 'TWITCH' CHECK(origin IN ('TWITCH','SCRIPT'));
UPDATE redemptions SET twitch_redemption_id=fulfillment_id;
ALTER TABLE redemptions ADD COLUMN script_project_id UUID REFERENCES script_projects(id);
ALTER TABLE redemptions ADD COLUMN script_revision BIGINT;
ALTER TABLE redemptions ADD COLUMN script_execution_id UUID;
ALTER TABLE redemptions ADD CONSTRAINT redemption_origin_consistent CHECK(
 (origin='TWITCH' AND twitch_redemption_id IS NOT NULL) OR
 (origin='SCRIPT' AND twitch_redemption_id IS NULL AND twitch_points_cost=0 AND script_project_id IS NOT NULL AND script_revision IS NOT NULL AND script_execution_id IS NOT NULL));
CREATE FUNCTION set_twitch_redemption_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.origin='TWITCH' AND NEW.twitch_redemption_id IS NULL THEN NEW.twitch_redemption_id=NEW.fulfillment_id; END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER set_twitch_redemption_identity BEFORE INSERT ON redemptions FOR EACH ROW EXECUTE FUNCTION set_twitch_redemption_identity();
CREATE OR REPLACE FUNCTION validate_inventory_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM redemptions r WHERE r.fulfillment_id=NEW.redemption_id AND r.user_id=NEW.viewer_id) THEN RAISE EXCEPTION 'Inventory owner must match redemption viewer'; END IF;
 RETURN NEW;
END; $$;
ALTER TABLE fulfillment_audit_events DROP CONSTRAINT fulfillment_audit_events_actor_kind_check;
ALTER TABLE fulfillment_audit_events ADD CHECK(actor_kind IN ('system','viewer','operator','script','scheduler','user'));
CREATE OR REPLACE FUNCTION audit_redemption_insert() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 INSERT INTO fulfillment_audit_events(event_key,redemption_id,event_type,actor_kind,actor_user_id,details)
 VALUES('redemption:'||NEW.fulfillment_id||':redeemed',NEW.fulfillment_id,'reward_redeemed',CASE WHEN NEW.origin='SCRIPT' THEN 'script' ELSE 'viewer' END,NEW.user_id,
 jsonb_build_object('origin',NEW.origin,'script_project_id',NEW.script_project_id,'script_revision',NEW.script_revision,'script_execution_id',NEW.script_execution_id)) ON CONFLICT DO NOTHING;
 RETURN NEW;
END; $$;
ALTER TABLE inventory_items DROP CONSTRAINT inventory_items_lifecycle_status_check;
ALTER TABLE inventory_items ADD CHECK(lifecycle_status IN ('WAITING_VIEWER','WAITING_OPERATOR','TRADE_LINK_REQUIRED','ORDER_PENDING','TRADE_WAITING','TRADE_ACCEPTED','RETRY_AVAILABLE','INSUFFICIENT_FUNDS','RECONCILIATION_REQUIRED','OPERATOR_REVIEW','DELIVERED','REFUNDING','REFUNDED','DISCARDED'));
ALTER TABLE inventory_items ADD COLUMN discarded_at TIMESTAMPTZ;
ALTER TABLE inventory_items ADD COLUMN discarded_by TEXT;
