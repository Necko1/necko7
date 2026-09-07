ALTER TABLE "broadcaster_settings"
    ADD COLUMN "add_bot_badge" BOOLEAN NOT NULL DEFAULT FALSE;

COMMENT ON COLUMN "broadcaster_settings"."add_bot_badge" IS 'whether to add Twitch chat bot badge (send via app access token) or not (send via bot user access token)';
