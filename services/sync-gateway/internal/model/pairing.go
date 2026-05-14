package model

import (
	"context"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PairingSession represents a device pairing session.
type PairingSession struct {
	ID                uuid.UUID `db:"id"`
	InitiatorDeviceID uuid.UUID `db:"initiator_device_id"`
	InitiatorPubkey   []byte    `db:"initiator_pubkey"`
	ResponderPubkey   []byte    `db:"responder_pubkey"`
	Status            string    `db:"status"`
	ExpiresAt         time.Time `db:"expires_at"`
	CreatedAt         time.Time `db:"created_at"`
}

// PairingStore provides database operations for pairing sessions.
type PairingStore struct {
	pool *pgxpool.Pool
}

// NewPairingStore creates a new PairingStore.
func NewPairingStore(pool *pgxpool.Pool) *PairingStore {
	return &PairingStore{pool: pool}
}

// Create inserts a new pairing session.
func (s *PairingStore) Create(ctx context.Context, ps *PairingSession) error {
	query := `
		INSERT INTO pairing_sessions (id, initiator_device_id, initiator_pubkey, responder_pubkey, status, expires_at, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
	`
	_, err := s.pool.Exec(ctx, query, ps.ID, ps.InitiatorDeviceID, ps.InitiatorPubkey, ps.ResponderPubkey, ps.Status, ps.ExpiresAt, ps.CreatedAt)
	return err
}

// GetByID retrieves a pairing session by ID.
func (s *PairingStore) GetByID(ctx context.Context, id uuid.UUID) (*PairingSession, error) {
	query := `
		SELECT id, initiator_device_id, initiator_pubkey, responder_pubkey, status, expires_at, created_at
		FROM pairing_sessions WHERE id = $1
	`
	row := s.pool.QueryRow(ctx, query, id)
	var ps PairingSession
	err := row.Scan(&ps.ID, &ps.InitiatorDeviceID, &ps.InitiatorPubkey, &ps.ResponderPubkey, &ps.Status, &ps.ExpiresAt, &ps.CreatedAt)
	if err != nil {
		return nil, err
	}
	return &ps, nil
}

// Complete updates the responder pubkey and status to completed.
func (s *PairingStore) Complete(ctx context.Context, id uuid.UUID, responderPubkey []byte) error {
	query := `
		UPDATE pairing_sessions
		SET responder_pubkey = $1, status = 'completed'
		WHERE id = $2 AND status = 'pending' AND expires_at > NOW()
	`
	_, err := s.pool.Exec(ctx, query, responderPubkey, id)
	return err
}

// MarkExpired updates status to expired for sessions past their expiry.
func (s *PairingStore) MarkExpired(ctx context.Context) error {
	query := `
		UPDATE pairing_sessions
		SET status = 'expired'
		WHERE status = 'pending' AND expires_at < NOW()
	`
	_, err := s.pool.Exec(ctx, query)
	return err
}

// DeleteOldExpired removes expired sessions older than 24 hours.
func (s *PairingStore) DeleteOldExpired(ctx context.Context) error {
	query := `
		DELETE FROM pairing_sessions
		WHERE status = 'expired' AND created_at < NOW() - INTERVAL '24 hours'
	`
	_, err := s.pool.Exec(ctx, query)
	return err
}
