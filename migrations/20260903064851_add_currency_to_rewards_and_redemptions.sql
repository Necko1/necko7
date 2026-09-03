ALTER TABLE "rewards"
    ADD COLUMN "currency" VARCHAR(10) NOT NULL DEFAULT 'RUB';

ALTER TABLE "redemptions"
    ADD COLUMN "currency" VARCHAR(10) NOT NULL DEFAULT 'RUB';

UPDATE "rewards" r
SET "currency" = bs."market_currency"
FROM "broadcaster_settings" bs
WHERE r."streamer_id" = bs."channel_id";

UPDATE "redemptions" rd
SET "currency" = rw."currency"
FROM "rewards" rw
WHERE rd."twitch_reward_id" = rw."twitch_id";

ALTER TABLE "broadcaster_settings"
    DROP COLUMN "market_currency";
