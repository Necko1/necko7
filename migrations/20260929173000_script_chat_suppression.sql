-- A script may replace selected fulfillment notices for this one reward trigger.
-- Keep the choice with the fulfillment so recovery and later attempts respect it.
ALTER TABLE redemptions
    ADD COLUMN script_suppressed_chat_keys TEXT[] NOT NULL DEFAULT '{}'::TEXT[];
