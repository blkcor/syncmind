-- +goose Up
CREATE TABLE devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    public_key_fingerprint VARCHAR(64) UNIQUE NOT NULL,
    public_key BYTEA NOT NULL,
    paired_device_id UUID NULL REFERENCES devices(id),
    device_type VARCHAR(20) CHECK (device_type IN ('desktop', 'mobile')),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ,
    is_active BOOLEAN DEFAULT TRUE
);

-- +goose Down
DROP TABLE IF EXISTS devices;
