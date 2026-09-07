ALTER TABLE "broadcaster_settings"
    ADD COLUMN IF NOT EXISTS "public_rewards_config" JSONB NOT NULL DEFAULT '{"enabled": false, "show_description": true, "show_cost_points": true, "show_cooldown_and_limits": true, "show_paused_rewards": true, "show_pause_reason": true, "show_market_price": true, "show_price_deviation": true, "show_pool_items": true, "show_pool_chances": true, "show_pool_item_prices": true, "show_filter_details": true, "show_chat_requirements": true, "show_purchase_limits": true}'::jsonb;

ALTER TABLE "rewards"
    ADD COLUMN IF NOT EXISTS "is_public" BOOLEAN NOT NULL DEFAULT TRUE;

CREATE TABLE IF NOT EXISTS "viewer_channels" (
    "user_id" VARCHAR(255) NOT NULL REFERENCES "users"("twitch_id") ON DELETE CASCADE,
    "channel_id" VARCHAR(255) NOT NULL REFERENCES "broadcasters"("channel_id") ON DELETE CASCADE,
    "is_pinned" BOOLEAN NOT NULL DEFAULT TRUE,
    "is_hidden" BOOLEAN NOT NULL DEFAULT FALSE,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY ("user_id", "channel_id")
);

CREATE INDEX IF NOT EXISTS "idx_chat_messages_user_broadcaster" ON "chat_messages"("chatter_user_id", "broadcaster_id");
CREATE INDEX IF NOT EXISTS "idx_redemptions_user_created_at" ON "redemptions"("user_id", "created_at" DESC);
