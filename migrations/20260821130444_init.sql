CREATE TABLE "broadcasters"(
    "channel_id" VARCHAR(255) NOT NULL,
    "channel_login" TEXT NOT NULL,
    "user_access_token" TEXT NOT NULL,
    "refresh_token" TEXT NOT NULL,
    "created_at" TIMESTAMP(0) WITH
        TIME zone NOT NULL,
        "updated_at" TIMESTAMP(0)
    WITH
        TIME zone NOT NULL
);
ALTER TABLE
    "broadcasters" ADD PRIMARY KEY("channel_id");
ALTER TABLE
    "broadcasters" ADD CONSTRAINT "broadcasters_channel_login_unique" UNIQUE("channel_login");
COMMENT
ON COLUMN
    "broadcasters"."user_access_token" IS 'auto updated with refresh token if twitch returned 403';
CREATE TABLE "app_settings"(
    "key" VARCHAR(255) NOT NULL,
    "value" TEXT NOT NULL
);
ALTER TABLE
    "app_settings" ADD PRIMARY KEY("key");
CREATE TABLE "rewards"(
    "twitch_id" UUID NOT NULL,
    "is_paused" BOOLEAN NOT NULL,
    "is_deleted" BOOLEAN NOT NULL DEFAULT FALSE,
    "streamer_id" VARCHAR(255) NOT NULL,
    "market_item_name" TEXT NOT NULL,
    "twitch_title" TEXT NOT NULL,
    "twitch_description" TEXT NOT NULL,
    "current_market_price" INTEGER NOT NULL,
    "permissible_market_price_deviation" INTEGER NOT NULL,
    "twitch_price_markup_percentage" SMALLINT NOT NULL,
    "global_cooldown_seconds" INTEGER NOT NULL,
    "max_redemptions_per_stream" SMALLINT NOT NULL,
    "max_redemptions_per_user_per_stream" SMALLINT NOT NULL,
    "created_at" TIMESTAMP(0) WITH
        TIME zone NOT NULL,
        "updated_at" TIMESTAMP(0)
    WITH
        TIME zone NOT NULL
);
ALTER TABLE
    "rewards" ADD PRIMARY KEY("twitch_id");
CREATE INDEX "rewards_streamer_id_index" ON
    "rewards"("streamer_id");
COMMENT
ON COLUMN
    "rewards"."current_market_price" IS '0.52 RUB = 52
0.52 USD = 520';
COMMENT
ON COLUMN
    "rewards"."permissible_market_price_deviation" IS 'max permissible deviation from the market price';
COMMENT
ON COLUMN
    "rewards"."twitch_price_markup_percentage" IS 'market price * base price multiplier * (twitch price markup percentage/100) = reward cost in twitch points';
CREATE TABLE "redemptions"(
    "twitch_redemption_id" UUID NOT NULL,
    "twitch_reward_id" UUID NOT NULL,
    "user_id" VARCHAR(255) NOT NULL,
    "user_login" TEXT NOT NULL,
    "user_trade_link" TEXT NOT NULL,
    "twitch_points_cost" BIGINT NOT NULL,
    "market_paid_price" BIGINT NULL,
    "status" VARCHAR(255) CHECK
        (
            "status" IN(
                'PENDING',
                'ORDER_CREATED',
                'FAILED_REFUND',
                'FAILED_PENALTY',
                'COMPLETED'
            )
        ) NOT NULL,
        "fail_cause" TEXT NULL,
        "fail_description" TEXT NULL,
        "created_at" TIMESTAMP(0)
    WITH
        TIME zone NOT NULL,
        "updated_at" TIMESTAMP(0)
    WITH
        TIME zone NOT NULL
);
CREATE INDEX "redemptions_user_id_user_login_index" ON
    "redemptions"("user_id", "user_login");
ALTER TABLE
    "redemptions" ADD PRIMARY KEY("twitch_redemption_id");
COMMENT
ON COLUMN
    "redemptions"."twitch_redemption_id" IS 'also used as custom_id for market /buy-for';
COMMENT
ON COLUMN
    "redemptions"."user_id" IS 'who redeemed';
COMMENT
ON COLUMN
    "redemptions"."market_paid_price" IS '0.52 RUB = 52
0.52 USD = 520';
COMMENT
ON COLUMN
    "redemptions"."fail_cause" IS 'buyer, seller, other';
CREATE TABLE "broadcaster_settings"(
    "channel_id" VARCHAR(255) NOT NULL,
    "is_active" BOOLEAN NOT NULL,
    "market_api_key" TEXT NOT NULL,
    "market_currency" VARCHAR(255) CHECK
        (
            "market_currency" IN('RUB', 'USD', 'EUR')
        ) NOT NULL,
        "base_price_multiplier" SMALLINT NOT NULL DEFAULT 200,
        "update_prices_period" INTEGER NOT NULL,
        "refund_on_buyer_fail" BOOLEAN NOT NULL DEFAULT FALSE,
        "updated_at" TIMESTAMP(0)
    WITH
        TIME zone NOT NULL
);
ALTER TABLE
    "broadcaster_settings" ADD PRIMARY KEY("channel_id");
COMMENT
ON COLUMN
    "broadcaster_settings"."base_price_multiplier" IS 'market price * base price multiplier * (twitch price markup percentage/100) = reward cost in twitch points';
COMMENT
ON COLUMN
    "broadcaster_settings"."update_prices_period" IS 'in seconds';
COMMENT
ON COLUMN
    "broadcaster_settings"."refund_on_buyer_fail" IS 'should points be returned if the buyer didnt accept the trade on time?';
ALTER TABLE
    "redemptions" ADD CONSTRAINT "redemptions_twitch_reward_id_foreign" FOREIGN KEY("twitch_reward_id") REFERENCES "rewards"("twitch_id");
ALTER TABLE
    "rewards" ADD CONSTRAINT "rewards_streamer_id_foreign" FOREIGN KEY("streamer_id") REFERENCES "broadcasters"("channel_id");
ALTER TABLE
    "broadcaster_settings" ADD CONSTRAINT "broadcaster_settings_channel_id_foreign" FOREIGN KEY("channel_id") REFERENCES "broadcasters"("channel_id");