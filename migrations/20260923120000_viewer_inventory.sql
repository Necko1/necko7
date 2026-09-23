-- A redemption has one persistent item. Twitch IDs on redemptions exist before local login.
CREATE TABLE inventory_items (
    id UUID PRIMARY KEY,
    redemption_id UUID NOT NULL UNIQUE REFERENCES redemptions(twitch_redemption_id),
    viewer_id VARCHAR(255) NOT NULL,
    item_name TEXT NOT NULL,
    fixed_price BIGINT NOT NULL CHECK (fixed_price >= 0),
    currency VARCHAR(255) NOT NULL,
    market_order_id TEXT,
    market_custom_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    acquired_at TIMESTAMPTZ
);
CREATE INDEX inventory_items_viewer_created_idx ON inventory_items(viewer_id, created_at DESC);

CREATE FUNCTION validate_inventory_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM redemptions r WHERE r.twitch_redemption_id = NEW.redemption_id AND r.user_id = NEW.viewer_id) THEN
        RAISE EXCEPTION 'Inventory owner must match redemption viewer';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER inventory_owner_matches_redemption BEFORE INSERT ON inventory_items
FOR EACH ROW EXECUTE FUNCTION validate_inventory_owner();

CREATE FUNCTION protect_inventory_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.redemption_id IS DISTINCT FROM OLD.redemption_id
       OR NEW.viewer_id IS DISTINCT FROM OLD.viewer_id
       OR NEW.fixed_price IS DISTINCT FROM OLD.fixed_price
       OR NEW.currency IS DISTINCT FROM OLD.currency THEN
        RAISE EXCEPTION 'Inventory ownership and fixed price are immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER inventory_snapshot_immutable BEFORE UPDATE ON inventory_items
FOR EACH ROW EXECUTE FUNCTION protect_inventory_snapshot();

-- A unique custom_id is the idempotency key for each external buy-for call.
CREATE TABLE inventory_order_attempts (
    attempt_id BIGSERIAL UNIQUE,
    custom_id TEXT PRIMARY KEY,
    inventory_id UUID NOT NULL REFERENCES inventory_items(id),
    item_name TEXT NOT NULL,
    max_price BIGINT CHECK (max_price >= 0),
    market_order_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX inventory_order_attempts_inventory_idx ON inventory_order_attempts(inventory_id, created_at DESC);

-- Historical paid price is not the initial buy-for ceiling. No reliable historical
-- snapshot exists, so old rows are intentionally not backfilled.
