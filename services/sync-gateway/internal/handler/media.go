package handler

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

var allowedMediaTypes = map[string]bool{
	"image/jpeg": true,
	"image/png":  true,
	"image/heic": true,
	"audio/m4a":  true,
	"audio/wav":  true,
}

// MediaHandler handles mobile media upload endpoints.
type MediaHandler struct {
	db  *pgxpool.Pool
	rdb *redis.Client
}

// NewMediaHandler creates a new MediaHandler.
func NewMediaHandler(db *pgxpool.Pool, rdb *redis.Client) *MediaHandler {
	return &MediaHandler{db: db, rdb: rdb}
}

// Upload handles POST /v1/media/upload.
func (h *MediaHandler) Upload(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)
	deviceIDVal, _ := c.Get(middleware.DeviceIDKey)
	deviceID := deviceIDVal.(uuid.UUID)

	log.Info("media upload started", zap.String("device_id", deviceID.String()))

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

	fileHeader, err := c.FormFile("file")
	if err != nil {
		log.Warn("missing file field")
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_REQUEST", "message": "missing file field"})
		return
	}

	contentType := fileHeader.Header.Get("Content-Type")
	if !allowedMediaTypes[contentType] {
		log.Warn("unsupported media type", zap.String("content_type", contentType))
		c.JSON(http.StatusUnsupportedMediaType, map[string]any{"code": "MEDIA_TYPE_UNSUPPORTED", "message": fmt.Sprintf("unsupported media type: %s", contentType)})
		return
	}

	file, err := fileHeader.Open()
	if err != nil {
		log.Error("failed to open uploaded file", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}
	defer file.Close()

	payload := make([]byte, fileHeader.Size)
	_, err = file.Read(payload)
	if err != nil {
		log.Error("failed to read uploaded file", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	hash := sha256.Sum256(payload)
	payloadHash := hex.EncodeToString(hash[:])

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
		log.Error("failed to create media bundle", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	// Publish notification to Redis
	notification := fmt.Sprintf(`{"type":"new_bundle","bundle_id":"%s","from_device":"%s","payload_size":%d,"content_type":"%s"}`,
		bundleID, deviceID, len(payload), contentType)
	_ = h.rdb.Publish(ctx, "sync:notify:"+device.PairedDeviceID.String(), notification).Err()

	log.Info("media upload completed", zap.String("bundle_id", bundleID.String()), zap.String("content_type", contentType), zap.Int("payload_size", len(payload)))
	c.JSON(http.StatusCreated, map[string]any{
		"media_id":   bundleID.String(),
		"bundle_id":  bundleID.String(),
		"expires_at": bundle.ExpiresAt.Format(time.RFC3339),
	})
}
