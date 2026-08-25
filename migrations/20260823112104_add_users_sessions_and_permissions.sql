CREATE TABLE "users" (
    "twitch_id" VARCHAR(255) NOT NULL,
    "login" TEXT NOT NULL,
    "avatar_url" TEXT NULL,
    "created_at" TIMESTAMP(0) WITH TIME ZONE NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMP(0) WITH TIME ZONE NOT NULL DEFAULT NOW()
);
ALTER TABLE "users" ADD PRIMARY KEY ("twitch_id");

CREATE TABLE "sessions" (
    "session_id" UUID NOT NULL,
    "user_id" VARCHAR(255) NOT NULL,
    "expires_at" TIMESTAMP(0) WITH TIME ZONE NOT NULL
);
ALTER TABLE "sessions" ADD PRIMARY KEY ("session_id");

CREATE TABLE "channel_permissions" (
    "channel_id" VARCHAR(255) NOT NULL,
    "user_id" VARCHAR(255) NOT NULL,
    "role" VARCHAR(255) CHECK ("role" IN ('OWNER', 'EDITOR')) NOT NULL,
    "granted_by" VARCHAR(255) NOT NULL,
    "created_at" TIMESTAMP(0) WITH TIME ZONE NOT NULL DEFAULT NOW()
);
ALTER TABLE "channel_permissions" ADD PRIMARY KEY ("channel_id", "user_id");

ALTER TABLE "rewards"
    ADD COLUMN "market_autobuy" BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE "broadcaster_settings"
    ADD COLUMN "refund_if_no_money" BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN "pause_reward_if_no_money" BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN "market_chance_to_transfer" SMALLINT NOT NULL DEFAULT 0;

INSERT INTO "users" ("twitch_id", "login", "created_at", "updated_at")
SELECT "channel_id", "channel_login", "created_at", "updated_at"
FROM "broadcasters"
ON CONFLICT ("twitch_id") DO NOTHING;

ALTER TABLE "broadcasters"
    ADD CONSTRAINT "broadcasters_channel_id_foreign"
    FOREIGN KEY ("channel_id") REFERENCES "users"("twitch_id") ON DELETE CASCADE;

ALTER TABLE "sessions"
    ADD CONSTRAINT "sessions_user_id_foreign"
    FOREIGN KEY ("user_id") REFERENCES "users"("twitch_id") ON DELETE CASCADE;

ALTER TABLE "channel_permissions"
    ADD CONSTRAINT "channel_permissions_channel_id_foreign"
    FOREIGN KEY ("channel_id") REFERENCES "broadcasters"("channel_id") ON DELETE CASCADE;

ALTER TABLE "channel_permissions"
    ADD CONSTRAINT "channel_permissions_user_id_foreign"
    FOREIGN KEY ("user_id") REFERENCES "users"("twitch_id") ON DELETE CASCADE;

ALTER TABLE "channel_permissions"
    ADD CONSTRAINT "channel_permissions_granted_by_foreign"
    FOREIGN KEY ("granted_by") REFERENCES "users"("twitch_id") ON DELETE CASCADE;