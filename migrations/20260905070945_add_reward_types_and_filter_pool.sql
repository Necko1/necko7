ALTER TABLE "rewards"
    ADD COLUMN "reward_type" VARCHAR(20) NOT NULL DEFAULT 'FIXED',
    ADD COLUMN "pricing_mode" VARCHAR(20) NOT NULL DEFAULT 'AUTO',
    ADD COLUMN "price_strategy" VARCHAR(20),
    ADD COLUMN "filter_config" JSONB,
    ADD COLUMN "pool_items" JSONB,
    ALTER COLUMN "market_item_name" DROP NOT NULL,
    ALTER COLUMN "market_item_name" SET DEFAULT '';

ALTER TABLE "redemptions"
    ADD COLUMN "market_item_name" TEXT;
