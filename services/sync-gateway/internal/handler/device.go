package handler

import (
	"context"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"go.uber.org/zap"
)

// DeviceHandler handles self-device status and revocation endpoints.
type DeviceHandler struct {
	devices *model.DeviceStore
}

// NewDeviceHandler creates a new DeviceHandler.
func NewDeviceHandler(devices *model.DeviceStore) *DeviceHandler {
	return &DeviceHandler{devices: devices}
}

// Status handles GET /v1/devices/:self_device_uuid.
// Returns device info only when the path UUID matches the authenticated device's JWT sub.
func (h *DeviceHandler) Status(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	pathUUID, ok := parseAndAuthorizeDevice(ctx, c, log)
	if !ok {
		return
	}

	device, err := h.devices.GetByID(ctx, pathUUID)
	if err != nil {
		if err == pgx.ErrNoRows {
			c.JSON(http.StatusNotFound, map[string]any{
				"code":    "DEVICE_NOT_FOUND",
				"message": "device not found",
			})
			return
		}
		log.Error("failed to fetch device", zap.String("device_id", pathUUID.String()), zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{
			"code":    "INTERNAL_ERROR",
			"message": "internal server error",
		})
		return
	}

	if !device.IsActive {
		c.JSON(http.StatusNotFound, map[string]any{
			"code":    "DEVICE_REVOKED",
			"message": "device has been revoked",
		})
		return
	}

	c.JSON(http.StatusOK, map[string]any{
		"device_uuid":      device.ID.String(),
		"device_type":      device.DeviceType,
		"paired_device_id": pairedDeviceIDOrNull(device.PairedDeviceID),
		"is_active":        device.IsActive,
		"last_seen_at":     nullableTime(device.LastSeenAt),
	})
}

// Revoke handles POST /v1/devices/:self_device_uuid/revoke.
// Deactivates the device and clears the peer's paired_device_id link.
func (h *DeviceHandler) Revoke(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	pathUUID, ok := parseAndAuthorizeDevice(ctx, c, log)
	if !ok {
		return
	}

	// Check that the device exists (or return appropriate stale-pairing response).
	device, err := h.devices.GetByID(ctx, pathUUID)
	if err != nil {
		if err == pgx.ErrNoRows {
			c.JSON(http.StatusNotFound, map[string]any{
				"code":    "DEVICE_NOT_FOUND",
				"message": "device not found",
			})
			return
		}
		log.Error("failed to fetch device for revoke", zap.String("device_id", pathUUID.String()), zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{
			"code":    "INTERNAL_ERROR",
			"message": "internal server error",
		})
		return
	}

	if !device.IsActive {
		c.JSON(http.StatusNotFound, map[string]any{
			"code":    "DEVICE_REVOKED",
			"message": "device has already been revoked",
		})
		return
	}

	if err := h.devices.Deactivate(ctx, pathUUID); err != nil {
		log.Error("failed to deactivate device", zap.String("device_id", pathUUID.String()), zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{
			"code":    "INTERNAL_ERROR",
			"message": "internal server error",
		})
		return
	}

	if err := h.devices.UnlinkPeer(ctx, pathUUID); err != nil {
		log.Error("failed to unlink peer", zap.String("device_id", pathUUID.String()), zap.Error(err))
	}

	c.SetStatusCode(http.StatusNoContent)
}

// parseAndAuthorizeDevice extracts the path UUID, validates it against the JWT sub,
// and returns the parsed UUID. On failure it writes an error response and returns false.
func parseAndAuthorizeDevice(ctx context.Context, c *app.RequestContext, log *zap.Logger) (uuid.UUID, bool) {
	authDeviceID, ok := c.Get(middleware.DeviceIDKey)
	if !ok {
		log.Warn("device_id missing from context")
		c.JSON(http.StatusUnauthorized, map[string]any{
			"code":    "AUTH_MISSING",
			"message": "device identity not in context",
		})
		return uuid.Nil, false
	}

	authUUID, ok := authDeviceID.(uuid.UUID)
	if !ok {
		log.Warn("device_id in context is not a UUID")
		c.JSON(http.StatusInternalServerError, map[string]any{
			"code":    "INTERNAL_ERROR",
			"message": "invalid device identity type",
		})
		return uuid.Nil, false
	}

	pathUUIDStr := c.Param("self_device_uuid")
	pathUUID, err := uuid.Parse(pathUUIDStr)
	if err != nil {
		c.JSON(http.StatusNotFound, map[string]any{
			"code":    "DEVICE_NOT_FOUND",
			"message": "device not found",
		})
		return uuid.Nil, false
	}

	if authUUID != pathUUID {
		c.JSON(http.StatusNotFound, map[string]any{
			"code":    "DEVICE_NOT_FOUND",
			"message": "device not found",
		})
		return uuid.Nil, false
	}

	return pathUUID, true
}

func pairedDeviceIDOrNull(id *uuid.UUID) *string {
	if id == nil {
		return nil
	}
	s := id.String()
	return &s
}

func nullableTime(t *time.Time) *string {
	if t == nil {
		return nil
	}
	s := t.Format(time.RFC3339)
	return &s
}
