ALTER TABLE redemptions DROP CONSTRAINT IF EXISTS redemptions_status_check;

ALTER TABLE redemptions
    ADD CONSTRAINT redemptions_status_check
    CHECK (status IN ('PENDING', 'ORDER_CREATED', 'FAILED_REFUND', 'FAILED_PENALTY', 'COMPLETED', 'MANUAL_HOLD'));
