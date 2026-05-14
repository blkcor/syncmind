-- +goose Up
CREATE TABLE pairing_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    initiator_device_id UUID NOT NULL,
    initiator_pubkey BYTEA NOT NULL,
    responder_pubkey BYTEA NULL,
    status VARCHAR(20) CHECK (status IN ('pending', 'completed', 'expired', 'cancelled')),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- +goose Down
DROP TABLE IF EXISTS pairing_sessions;
