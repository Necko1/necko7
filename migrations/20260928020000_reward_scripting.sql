ALTER TABLE rewards ADD COLUMN is_visible BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE rewards ADD COLUMN script_alias TEXT CHECK(script_alias IS NULL OR script_alias ~ '^[a-z][a-z0-9_]{0,63}$');
CREATE UNIQUE INDEX rewards_script_alias ON rewards(streamer_id,script_alias) WHERE script_alias IS NOT NULL AND NOT is_deleted;
