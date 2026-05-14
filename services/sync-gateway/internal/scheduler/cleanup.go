package scheduler

import (
	"context"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/jackc/pgx/v5/pgxpool"
	"go.uber.org/zap"
)

// CleanupScheduler runs periodic maintenance tasks.
type CleanupScheduler struct {
	db *pgxpool.Pool
}

// NewCleanupScheduler creates a new CleanupScheduler.
func NewCleanupScheduler(db *pgxpool.Pool) *CleanupScheduler {
	return &CleanupScheduler{db: db}
}

// Start begins the background cleanup loops.
func (s *CleanupScheduler) Start(ctx context.Context) {
	go s.runPairingSessionCleanup(ctx)
	go s.runBundleCleanup(ctx)
}

func (s *CleanupScheduler) runPairingSessionCleanup(ctx context.Context) {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			pairingStore := model.NewPairingStore(s.db)
			if err := pairingStore.MarkExpired(ctx); err != nil {
				logger.L().Warn("failed to mark expired pairing sessions", zap.Error(err))
			}
			if err := pairingStore.DeleteOldExpired(ctx); err != nil {
				logger.L().Warn("failed to delete old expired pairing sessions", zap.Error(err))
			}
		}
	}
}

func (s *CleanupScheduler) runBundleCleanup(ctx context.Context) {
	ticker := time.NewTicker(24 * time.Hour)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			bundleStore := model.NewBundleStore(s.db)
			if err := bundleStore.CleanupAckedAndExpired(ctx); err != nil {
				logger.L().Warn("failed to cleanup acked and expired bundles", zap.Error(err))
			}
		}
	}
}
