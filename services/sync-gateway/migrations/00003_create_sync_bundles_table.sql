-- +goose Up
CREATE TABLE sync_bundles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_device_id UUID NOT NULL,
    to_device_id UUID NOT NULL,
    encrypted_payload BYTEA NOT NULL,
    payload_hash VARCHAR(64) NOT NULL,
    payload_size_bytes INT NOT NULL,
    content_type VARCHAR(50) DEFAULT 'application/octet-stream',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    acked_at TIMESTAMPTZ NULL,
    deleted_at TIMESTAMPTZ NULL
);

-- +goose Down
DROP TABLE IF EXISTS sync_bundles;
