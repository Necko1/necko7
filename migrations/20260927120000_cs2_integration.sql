-- One pending credential per existing broadcaster. Consumed credentials are removed.
CREATE TABLE cs2_pairing_codes (
    channel_id TEXT PRIMARY KEY REFERENCES broadcasters(channel_id) ON DELETE CASCADE,
    code_hash BYTEA NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX cs2_pairing_expiry ON cs2_pairing_codes(expires_at);
CREATE TABLE cs2_devices (
    id UUID PRIMARY KEY,
    channel_id TEXT NOT NULL REFERENCES broadcasters(channel_id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL CHECK (octet_length(public_key) = 32),
    app_version TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX cs2_one_active_device ON cs2_devices(channel_id) WHERE revoked_at IS NULL;
CREATE INDEX cs2_devices_channel ON cs2_devices(channel_id);
-- Retain sessions longer than the signed timestamp window. Device row locking
-- serializes ingestion/revocation. A maximum of 32 live sessions is enforced.
CREATE TABLE cs2_sessions (
    device_id UUID NOT NULL REFERENCES cs2_devices(id) ON DELETE CASCADE,
    session_id UUID NOT NULL,
    highest_seq BIGINT NOT NULL CHECK (highest_seq > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (device_id, session_id)
);
CREATE INDEX cs2_sessions_expiry ON cs2_sessions(expires_at);
