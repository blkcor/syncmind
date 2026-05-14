package handler

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"strconv"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/metrics"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

// SyncHandler handles sync bundle endpoints.
type SyncHandler struct {
	db  *pgxpool.Pool
	rdb *redis.Client
}

// NewSyncHandler creates a new SyncHandler.
func NewSyncHandler(db *pgxpool.Pool, rdb *redis.Client) *SyncHandler {
	return &SyncHandler{db: db, rdb: rdb}
}

// Upload handles POST /v1/sync/bundle.
func (h *SyncHandler) Upload(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)
	deviceIDVal, _ := c.Get(middleware.DeviceIDKey)
	deviceID := deviceIDVal.(uuid.UUID)

	log.Info("upload started", zap.String("device_id", deviceID.String()))

	deviceStore := model.NewDeviceStore(h.db)
	device, err := deviceStore.GetByID(ctx, deviceID)
	if err != nil {
		log.Error("failed to get device", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}
	if device.PairedDeviceID == nil {
		log.Warn("device not paired", zap.String("device_id", deviceID.String()))
		c.JSON(http.StatusUnprocessableEntity, map[string]any{"code": "DEVICE_NOT_PAIRED", "message": "device has no paired partner"})
		return
	}

	// Idempotency check
	idempotencyKey := string(c.GetHeader("Idempotency-Key"))
	if idempotencyKey != "" {
		redisKey := "idempotency:" + deviceID.String() + ":" + idempotencyKey
		cached, err := h.rdb.Get(ctx, redisKey).Result()
		if err == nil && cached != "" {
			log.Info("idempotency cache hit", zap.String("bundle_id", cached))
			c.JSON(http.StatusOK, map[string]any{"bundle_id": cached})
			return
		}
	}

	payload := c.Request.Body()
	if len(payload) == 0 {
		log.Warn("empty payload")
		c.JSON(http.StatusBadRequest, map[string]any{"code": "EMPTY_PAYLOAD", "message": "bundle payload is empty"})
		return
	}

	hash := sha256.Sum256(payload)
	payloadHash := hex.EncodeToString(hash[:])
	contentType := string(c.GetHeader("X-Syncmind-Content-Type"))
	if contentType == "" {
		contentType = "application/octet-stream"
	}

	bundleID := uuid.New()
	bundle := &model.SyncBundle{
		ID:               bundleID,
		FromDeviceID:     deviceID,
		ToDeviceID:       *device.PairedDeviceID,
		EncryptedPayload: payload,
		PayloadHash:      payloadHash,
		PayloadSizeBytes: len(payload),
		ContentType:      contentType,
		CreatedAt:        time.Now().UTC(),
		ExpiresAt:        time.Now().UTC().Add(30 * 24 * time.Hour),
	}

	bundleStore := model.NewBundleStore(h.db)
	if err := bundleStore.Create(ctx, bundle); err != nil {
		log.Error("failed to create bundle", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	// Publish notification to Redis
	notification := fmt.Sprintf(`{"type":"new_bundle","bundle_id":"%s","from_device":"%s","payload_size":%d,"content_type":"%s"}`,
		bundleID, deviceID, len(payload), contentType)
	_ = h.rdb.Publish(ctx, "sync:notify:"+device.PairedDeviceID.String(), notification).Err()

	metrics.SyncBundlesTotal.WithLabelValues(contentType).Inc()
	metrics.SyncBundleSizeBytes.WithLabelValues(contentType).Observe(float64(len(payload)))

	// Cache idempotency key for 24h
	if idempotencyKey != "" {
		redisKey := "idempotency:" + deviceID.String() + ":" + idempotencyKey
		_ = h.rdb.Set(ctx, redisKey, bundleID.String(), 24*time.Hour).Err()
	}

	log.Info("upload completed", zap.String("bundle_id", bundleID.String()), zap.Int("payload_size", len(payload)))
	c.JSON(http.StatusCreated, map[string]any{"bundle_id": bundleID.String()})
}

// List handles GET /v1/sync/bundles.
func (h *SyncHandler) List(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)
	deviceIDVal, _ := c.Get(middleware.DeviceIDKey)
	deviceID := deviceIDVal.(uuid.UUID)

	limitStr := c.Query("limit")
	limit := 20
	if limitStr != "" {
		if l, err := strconv.Atoi(limitStr); err == nil && l > 0 {
			limit = l
		}
	}

	bundleStore := model.NewBundleStore(h.db)
	bundles, err := bundleStore.ListPending(ctx, deviceID, limit)
	if err != nil {
		log.Error("failed to list bundles", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	results := make([]map[string]any, len(bundles))
	for i, b := range bundles {
		results[i] = map[string]any{
			"bundle_id":    b.ID.String(),
			"from_device":  b.FromDeviceID.String(),
			"payload_size": b.PayloadSizeBytes,
			"content_type": b.ContentType,
			"created_at":   b.CreatedAt.Format(time.RFC3339),
			"payload_hash": b.PayloadHash,
		}
	}

	log.Info("list bundles", zap.Int("count", len(results)))
	c.JSON(http.StatusOK, results)
}

// Download handles GET /v1/sync/bundles/:id.
func (h *SyncHandler) Download(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)
	deviceIDVal, _ := c.Get(middleware.DeviceIDKey)
	deviceID := deviceIDVal.(uuid.UUID)

	bundleIDStr := c.Param("id")
	bundleID, err := uuid.Parse(bundleIDStr)
	if err != nil {
		log.Warn("invalid bundle id", zap.String("bundle_id", bundleIDStr))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_BUNDLE_ID", "message": "invalid bundle id"})
		return
	}

	bundleStore := model.NewBundleStore(h.db)
	bundle, err := bundleStore.GetByID(ctx, bundleID)
	if err != nil {
		log.Warn("bundle not found", zap.String("bundle_id", bundleIDStr))
		c.Status(http.StatusNotFound)
		return
	}
	if bundle.ToDeviceID != deviceID {
		log.Warn("unauthorized bundle access", zap.String("bundle_id", bundleIDStr), zap.String("device_id", deviceID.String()))
		c.Status(http.StatusNotFound)
		return
	}

	log.Info("download bundle", zap.String("bundle_id", bundleID.String()), zap.Int("payload_size", bundle.PayloadSizeBytes))
	c.Header("X-Syncmind-Content-Type", bundle.ContentType)
	c.Header("X-Syncmind-Payload-Hash", bundle.PayloadHash)
	c.Data(http.StatusOK, "application/octet-stream", bundle.EncryptedPayload)
}

// Ack handles DELETE /v1/sync/bundles/:id.
func (h *SyncHandler) Ack(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)
	deviceIDVal, _ := c.Get(middleware.DeviceIDKey)
	deviceID := deviceIDVal.(uuid.UUID)

	bundleIDStr := c.Param("id")
	bundleID, err := uuid.Parse(bundleIDStr)
	if err != nil {
		log.Warn("invalid bundle id", zap.String("bundle_id", bundleIDStr))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_BUNDLE_ID", "message": "invalid bundle id"})
		return
	}

	bundleStore := model.NewBundleStore(h.db)
	bundle, err := bundleStore.GetByID(ctx, bundleID)
	if err != nil {
		log.Warn("bundle not found", zap.String("bundle_id", bundleIDStr))
		c.Status(http.StatusNotFound)
		return
	}
	if bundle.ToDeviceID != deviceID {
		log.Warn("unauthorized bundle ack", zap.String("bundle_id", bundleIDStr), zap.String("device_id", deviceID.String()))
		c.Status(http.StatusNotFound)
		return
	}

	if err := bundleStore.Ack(ctx, bundleID); err != nil {
		log.Error("failed to ack bundle", zap.String("bundle_id", bundleIDStr), zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	log.Info("bundle acked", zap.String("bundle_id", bundleID.String()))
	c.Status(http.StatusNoContent)
}
