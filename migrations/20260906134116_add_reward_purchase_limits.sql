ALTER TABLE "rewards"
    ADD COLUMN IF NOT EXISTS "purchase_limits" JSONB;

CREATE INDEX IF NOT EXISTS "idx_redemptions_reward_status_created"
    ON "redemptions"("twitch_reward_id", "status", "created_at");

CREATE INDEX IF NOT EXISTS "idx_redemptions_reward_user_status_created"
    ON "redemptions"("twitch_reward_id", "user_id", "status", "created_at");
