-- Existing redemptions retain their historical counting behavior. A denied
-- EventSub redemption remains recorded without consuming a purchase-limit slot.
ALTER TABLE redemptions
    ADD COLUMN purchase_limit_decision JSONB NOT NULL DEFAULT '{"kind":"admitted"}'::jsonb;

ALTER TABLE redemptions
    ADD CONSTRAINT redemptions_purchase_limit_decision_kind_check
    CHECK (purchase_limit_decision->>'kind' IN ('admitted', 'global_rejected', 'user_rejected'));
