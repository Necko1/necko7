CREATE TABLE IF NOT EXISTS "channel_logs" (
    "id" BIGSERIAL PRIMARY KEY,
    "broadcaster_id" VARCHAR(255) NOT NULL,
    "level" VARCHAR(16) NOT NULL,
    "category" VARCHAR(32) NOT NULL,
    "event_type" VARCHAR(64) NOT NULL,
    "message" TEXT NOT NULL,
    "details" JSONB,
    "solution_hint" TEXT,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS "idx_channel_logs_broadcaster_created" ON "channel_logs"("broadcaster_id", "created_at" DESC);
CREATE INDEX IF NOT EXISTS "idx_channel_logs_broadcaster_level" ON "channel_logs"("broadcaster_id", "level", "created_at" DESC);
CREATE INDEX IF NOT EXISTS "idx_channel_logs_broadcaster_category" ON "channel_logs"("broadcaster_id", "category", "created_at" DESC);
