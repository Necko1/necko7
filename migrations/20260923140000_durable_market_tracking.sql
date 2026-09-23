-- Market observations are historical evidence on the attempt. A final stage-5
-- response may clear timestamps, so later nulls must never erase them.
ALTER TABLE inventory_order_attempts ADD COLUMN settlement TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN trade_created_at TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN last_market_stage VARCHAR(16);
ALTER TABLE inventory_order_attempts ADD COLUMN market_refund JSONB;
ALTER TABLE inventory_order_attempts ADD COLUMN next_poll_at TIMESTAMPTZ;
ALTER TABLE inventory_order_attempts ADD COLUMN evidence_complete BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE inventory_items ADD COLUMN twitch_fulfillment_claimed_at TIMESTAMPTZ;

ALTER TABLE inventory_order_attempts DROP CONSTRAINT inventory_order_attempts_status_check;
ALTER TABLE inventory_order_attempts ADD CONSTRAINT inventory_order_attempts_status_check
    CHECK (status IN ('CALLING', 'REJECTED', 'ORDER_CREATED', 'TRADE_WAITING',
        'TRADE_ACCEPTED', 'SELLER_FAILED', 'BUYER_FAILED', 'TERMINAL_UNCLASSIFIED',
        'DELIVERED', 'RECONCILIATION_REQUIRED'));
ALTER TABLE inventory_items DROP CONSTRAINT inventory_items_lifecycle_status_check;
ALTER TABLE inventory_items ADD CONSTRAINT inventory_items_lifecycle_status_check
    CHECK (lifecycle_status IN ('WAITING_VIEWER', 'WAITING_OPERATOR', 'TRADE_LINK_REQUIRED',
        'ORDER_PENDING', 'TRADE_WAITING', 'TRADE_ACCEPTED', 'RETRY_AVAILABLE',
        'INSUFFICIENT_FUNDS', 'RECONCILIATION_REQUIRED', 'OPERATOR_REVIEW',
        'DELIVERED', 'REFUNDING', 'REFUNDED'));

DROP INDEX inventory_one_live_attempt;
CREATE UNIQUE INDEX inventory_one_live_attempt ON inventory_order_attempts(inventory_id)
    WHERE status IN ('CALLING', 'ORDER_CREATED', 'TRADE_WAITING', 'TRADE_ACCEPTED');

-- Existing null trade evidence is unknown, never evidence of absence.
UPDATE inventory_order_attempts SET evidence_complete = FALSE;
UPDATE inventory_order_attempts SET trade_created_at = COALESCE(last_checked_at, created_at)
WHERE receive_until > TIMESTAMPTZ '1970-01-01 00:00:00+00' AND trade_id IS NOT NULL;
UPDATE inventory_order_attempts SET next_poll_at = NOW()
WHERE status IN ('CALLING', 'ORDER_CREATED', 'TRADE_WAITING', 'RECONCILIATION_REQUIRED');

-- The previous watcher could mark DELIVERED at stage 1 when settlement appeared.
-- Recheck the same custom ID before trusting those historical completions.
UPDATE inventory_order_attempts a SET status = 'RECONCILIATION_REQUIRED',
    outcome_kind = 'legacy_delivery_unverified', next_poll_at = NOW()
FROM inventory_items i
WHERE a.inventory_id = i.id AND i.fulfillment_mode != 'LEGACY_REVIEW'
  AND i.market_custom_id = a.custom_id AND a.status = 'DELIVERED';
UPDATE inventory_items i SET lifecycle_status = 'RECONCILIATION_REQUIRED'
FROM inventory_order_attempts a
WHERE a.inventory_id = i.id AND i.fulfillment_mode != 'LEGACY_REVIEW'
  AND i.market_custom_id = a.custom_id AND a.outcome_kind = 'legacy_delivery_unverified';

-- Old failure writes were only reached from stage 5. The old seller branch also
-- included missing/unknown causer, so downgrade those cases to honest review.
UPDATE inventory_order_attempts SET last_market_stage = '5'
WHERE status IN ('SELLER_FAILED','BUYER_FAILED');
UPDATE inventory_order_attempts SET status = 'TERMINAL_UNCLASSIFIED', outcome_kind = 'terminal_unclassified'
WHERE status = 'SELLER_FAILED' AND LOWER(COALESCE(causer, '')) != 'seller';
UPDATE inventory_items i SET lifecycle_status = 'OPERATOR_REVIEW'
FROM inventory_order_attempts a
WHERE a.inventory_id = i.id AND i.market_custom_id = a.custom_id
  AND a.status = 'TERMINAL_UNCLASSIFIED' AND i.lifecycle_status = 'RETRY_AVAILABLE';

CREATE INDEX inventory_attempt_poll_due_idx ON inventory_order_attempts(next_poll_at)
WHERE status IN ('CALLING', 'ORDER_CREATED', 'TRADE_WAITING', 'TRADE_ACCEPTED', 'RECONCILIATION_REQUIRED');

-- Best-effort chat is claimed before sending so concurrent pollers and restarts
-- cannot announce the same attempt milestone twice.
CREATE TABLE inventory_attempt_chat_events (
    custom_id TEXT NOT NULL REFERENCES inventory_order_attempts(custom_id),
    event_key TEXT NOT NULL,
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (custom_id, event_key)
);
