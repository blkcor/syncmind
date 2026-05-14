package model

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Device represents a registered device in the system.
type Device struct {
	ID                   uuid.UUID  `db:"id"`
	PublicKeyFingerprint string     `db:"public_key_fingerprint"`
	PublicKey            []byte     `db:"public_key"`
	PairedDeviceID       *uuid.UUID `db:"paired_device_id"`
	DeviceType           string     `db:"device_type"`
	CreatedAt            time.Time  `db:"created_at"`
	LastSeenAt           *time.Time `db:"last_seen_at"`
	IsActive             bool       `db:"is_active"`
}

// DeviceStore provides database operations for devices.
type DeviceStore struct {
	pool *pgxpool.Pool
}

// NewDeviceStore creates a new DeviceStore.
func NewDeviceStore(pool *pgxpool.Pool) *DeviceStore {
	return &DeviceStore{pool: pool}
}

// Create inserts a new device record.
func (s *DeviceStore) Create(ctx context.Context, d *Device) error {
	query := `
		INSERT INTO devices (id, public_key_fingerprint, public_key, paired_device_id, device_type, created_at, last_seen_at, is_active)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
	`
	_, err := s.pool.Exec(ctx, query, d.ID, d.PublicKeyFingerprint, d.PublicKey, d.PairedDeviceID, d.DeviceType, d.CreatedAt, d.LastSeenAt, d.IsActive)
	return err
}

// GetByID retrieves a device by its UUID.
func (s *DeviceStore) GetByID(ctx context.Context, id uuid.UUID) (*Device, error) {
	query := `
		SELECT id, public_key_fingerprint, public_key, paired_device_id, device_type, created_at, last_seen_at, is_active
		FROM devices WHERE id = $1
	`
	row := s.pool.QueryRow(ctx, query, id)
	var d Device
	err := row.Scan(&d.ID, &d.PublicKeyFingerprint, &d.PublicKey, &d.PairedDeviceID, &d.DeviceType, &d.CreatedAt, &d.LastSeenAt, &d.IsActive)
	if err != nil {
		return nil, err
	}
	return &d, nil
}

// GetByFingerprint retrieves a device by public key fingerprint.
func (s *DeviceStore) GetByFingerprint(ctx context.Context, fingerprint string) (*Device, error) {
	query := `
		SELECT id, public_key_fingerprint, public_key, paired_device_id, device_type, created_at, last_seen_at, is_active
		FROM devices WHERE public_key_fingerprint = $1
	`
	row := s.pool.QueryRow(ctx, query, fingerprint)
	var d Device
	err := row.Scan(&d.ID, &d.PublicKeyFingerprint, &d.PublicKey, &d.PairedDeviceID, &d.DeviceType, &d.CreatedAt, &d.LastSeenAt, &d.IsActive)
	if err != nil {
		return nil, err
	}
	return &d, nil
}

// UpdatePairedDevice sets the paired_device_id for a device.
func (s *DeviceStore) UpdatePairedDevice(ctx context.Context, id, pairedID uuid.UUID) error {
	query := `UPDATE devices SET paired_device_id = $1 WHERE id = $2`
	_, err := s.pool.Exec(ctx, query, pairedID, id)
	return err
}

// Deactivate sets is_active = false for a device.
func (s *DeviceStore) Deactivate(ctx context.Context, id uuid.UUID) error {
	query := `UPDATE devices SET is_active = false WHERE id = $1`
	_, err := s.pool.Exec(ctx, query, id)
	return err
}

// UpdateLastSeen sets last_seen_at to the provided timestamp.
func (s *DeviceStore) UpdateLastSeen(ctx context.Context, id uuid.UUID, t time.Time) error {
	query := `UPDATE devices SET last_seen_at = $1 WHERE id = $2`
	_, err := s.pool.Exec(ctx, query, t, id)
	return err
}

// PublicKeyFingerprint computes the SHA-256 fingerprint of a public key.
func PublicKeyFingerprint(pubKey []byte) string {
	hash := sha256.Sum256(pubKey)
	return hex.EncodeToString(hash[:])
}
