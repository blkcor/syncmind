-- +goose Up
ALTER TABLE pairing_sessions
ADD COLUMN short_code VARCHAR(7) NULL;

CREATE UNIQUE INDEX idx_pairing_sessions_pending_short_code
ON pairing_sessions (short_code)
WHERE status = 'pending' AND short_code IS NOT NULL;

-- +goose Down
DROP INDEX IF EXISTS idx_pairing_sessions_pending_short_code;

ALTER TABLE pairing_sessions
DROP COLUMN IF EXISTS short_code;
