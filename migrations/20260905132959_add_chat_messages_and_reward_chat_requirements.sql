CREATE TABLE IF NOT EXISTS "chat_messages" (
    "id" BIGSERIAL PRIMARY KEY,
    "message_id" VARCHAR(64) NOT NULL,
    "broadcaster_id" VARCHAR(64) NOT NULL,
    "chatter_user_id" VARCHAR(64) NOT NULL,
    "chatter_user_login" VARCHAR(128) NOT NULL,
    "message_text" TEXT NOT NULL,
    "char_count" INT NOT NULL,
    "sent_at" TIMESTAMPTZ NOT NULL,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS "idx_chat_messages_lookup" ON "chat_messages"("broadcaster_id", "chatter_user_id", "sent_at");
CREATE INDEX IF NOT EXISTS "idx_chat_messages_leaderboard" ON "chat_messages"("broadcaster_id", "sent_at");
CREATE INDEX IF NOT EXISTS "idx_chat_messages_sent_at" ON "chat_messages"("sent_at");

ALTER TABLE "rewards"
    ADD COLUMN "chat_min_messages" INT,
    ADD COLUMN "chat_min_characters" INT,
    ADD COLUMN "chat_time_window_hours" INT,
    ADD COLUMN "chat_logical_operator" VARCHAR(10),
    ADD COLUMN "refund_if_chat_req_failed" BOOLEAN NOT NULL DEFAULT TRUE;

DELETE FROM "app_settings" WHERE "key" = 'bot_auth';
