-- +goose Up
CREATE INDEX idx_sync_bundles_to_device_acked ON sync_bundles(to_device_id, acked_at);
CREATE INDEX idx_pairing_sessions_expires ON pairing_sessions(expires_at);

-- +goose Down
DROP INDEX IF EXISTS idx_sync_bundles_to_device_acked;
DROP INDEX IF EXISTS idx_pairing_sessions_expires;
