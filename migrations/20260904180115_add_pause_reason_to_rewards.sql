ALTER TABLE "rewards" ADD COLUMN "pause_reason" VARCHAR(64) NULL;
UPDATE "rewards" SET "pause_reason" = 'MANUAL' WHERE "is_paused" = TRUE;
