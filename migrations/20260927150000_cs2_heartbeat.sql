-- Keep desktop reachability separate from game activity. Existing last_seen_at
-- remains the last accepted GSI timestamp for compatibility.
ALTER TABLE cs2_devices ADD COLUMN last_heartbeat_at TIMESTAMPTZ;
