package model

import (
	"context"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// SyncBundle represents an encrypted sync bundle.
type SyncBundle struct {
	ID               uuid.UUID  `db:"id"`
	FromDeviceID     uuid.UUID  `db:"from_device_id"`
	ToDeviceID       uuid.UUID  `db:"to_device_id"`
	EncryptedPayload []byte     `db:"encrypted_payload"`
	PayloadHash      string     `db:"payload_hash"`
	PayloadSizeBytes int        `db:"payload_size_bytes"`
	ContentType      string     `db:"content_type"`
	CreatedAt        time.Time  `db:"created_at"`
	ExpiresAt        time.Time  `db:"expires_at"`
	AckedAt          *time.Time `db:"acked_at"`
	DeletedAt        *time.Time `db:"deleted_at"`
}

// BundleStore provides database operations for sync bundles.
type BundleStore struct {
	pool *pgxpool.Pool
}

// NewBundleStore creates a new BundleStore.
func NewBundleStore(pool *pgxpool.Pool) *BundleStore {
	return &BundleStore{pool: pool}
}

// Create inserts a new sync bundle.
func (s *BundleStore) Create(ctx context.Context, b *SyncBundle) error {
	query := `
		INSERT INTO sync_bundles (id, from_device_id, to_device_id, encrypted_payload, payload_hash, payload_size_bytes, content_type, created_at, expires_at, acked_at, deleted_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
	`
	_, err := s.pool.Exec(ctx, query, b.ID, b.FromDeviceID, b.ToDeviceID, b.EncryptedPayload, b.PayloadHash, b.PayloadSizeBytes, b.ContentType, b.CreatedAt, b.ExpiresAt, b.AckedAt, b.DeletedAt)
	return err
}

// GetByID retrieves a bundle by ID.
func (s *BundleStore) GetByID(ctx context.Context, id uuid.UUID) (*SyncBundle, error) {
	query := `
		SELECT id, from_device_id, to_device_id, encrypted_payload, payload_hash, payload_size_bytes, content_type, created_at, expires_at, acked_at, deleted_at
		FROM sync_bundles WHERE id = $1
	`
	row := s.pool.QueryRow(ctx, query, id)
	var b SyncBundle
	err := row.Scan(&b.ID, &b.FromDeviceID, &b.ToDeviceID, &b.EncryptedPayload, &b.PayloadHash, &b.PayloadSizeBytes, &b.ContentType, &b.CreatedAt, &b.ExpiresAt, &b.AckedAt, &b.DeletedAt)
	if err != nil {
		return nil, err
	}
	return &b, nil
}

// ListPending retrieves pending bundles for a device (metadata only, no payload).
func (s *BundleStore) ListPending(ctx context.Context, toDeviceID uuid.UUID, limit int) ([]*SyncBundle, error) {
	query := `
		SELECT id, from_device_id, to_device_id, payload_hash, payload_size_bytes, content_type, created_at, expires_at
		FROM sync_bundles
		WHERE to_device_id = $1 AND acked_at IS NULL
		ORDER BY created_at DESC
		LIMIT $2
	`
	rows, err := s.pool.Query(ctx, query, toDeviceID, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var bundles []*SyncBundle
	for rows.Next() {
		var b SyncBundle
		if err := rows.Scan(&b.ID, &b.FromDeviceID, &b.ToDeviceID, &b.PayloadHash, &b.PayloadSizeBytes, &b.ContentType, &b.CreatedAt, &b.ExpiresAt); err != nil {
			return nil, err
		}
		bundles = append(bundles, &b)
	}
	return bundles, rows.Err()
}

// Ack marks a bundle as acknowledged.
func (s *BundleStore) Ack(ctx context.Context, id uuid.UUID) error {
	query := `UPDATE sync_bundles SET acked_at = NOW() WHERE id = $1`
	_, err := s.pool.Exec(ctx, query, id)
	return err
}

// CleanupAckedAndExpired hard-deletes bundles that are acked > 7 days or expired.
func (s *BundleStore) CleanupAckedAndExpired(ctx context.Context) error {
	query := `
		DELETE FROM sync_bundles
		WHERE (acked_at IS NOT NULL AND acked_at < NOW() - INTERVAL '7 days')
		OR (expires_at < NOW())
	`
	_, err := s.pool.Exec(ctx, query)
	return err
}
