ALTER TABLE "broadcaster_settings"
    ADD COLUMN "chat_messages" JSONB NOT NULL DEFAULT '{}'::jsonb;
