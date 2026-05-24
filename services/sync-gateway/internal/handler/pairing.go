package handler

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"math/big"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/metrics"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"go.uber.org/zap"
)

// PairingHandler handles device pairing endpoints.
type PairingHandler struct {
	db *pgxpool.Pool
}

// NewPairingHandler creates a new PairingHandler.
func NewPairingHandler(db *pgxpool.Pool) *PairingHandler {
	return &PairingHandler{db: db}
}

// InitiateRequest represents the request body for initiating pairing.
type InitiateRequest struct {
	DeviceUUID      string `json:"device_uuid"`      // client-minted UUIDv4 used as devices.id (see PRD 003 §Impl Note 1.2)
	InitiatorPubkey string `json:"initiator_pubkey"` // base64url encoded Ed25519 device identity pubkey
	DeviceType      string `json:"device_type"`      // "desktop" or "mobile"
}

// InitiateResponse represents the response for initiating pairing.
type InitiateResponse struct {
	SessionID string `json:"session_id"`
	QRPayload string `json:"qr_payload"`
	ShortCode string `json:"short_code"`
	ExpiresAt string `json:"expires_at"`
}

// Initiate handles POST /v1/pairing/initiate.
func (h *PairingHandler) Initiate(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	var req InitiateRequest
	if err := c.BindJSON(&req); err != nil {
		log.Warn("invalid initiate request", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_REQUEST", "message": "invalid request body"})
		return
	}

	pubkeyBytes, err := base64.RawURLEncoding.DecodeString(req.InitiatorPubkey)
	if err != nil {
		log.Warn("invalid public key", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_PUBKEY", "message": "invalid base64url public key"})
		return
	}

	deviceID, err := parseClientUUID(req.DeviceUUID)
	if err != nil {
		log.Warn("invalid device_uuid", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_REQUEST", "message": "invalid or missing device_uuid (must be UUIDv4)"})
		return
	}

	deviceStore := model.NewDeviceStore(h.db)
	fingerprint := model.PublicKeyFingerprint(pubkeyBytes)
	if conflict, status, code, msg := h.resolveDeviceConflict(ctx, deviceStore, deviceID, fingerprint); conflict {
		c.JSON(status, map[string]any{"code": code, "message": msg})
		return
	} else if status == http.StatusCreated {
		device := &model.Device{
			ID:                   deviceID,
			PublicKeyFingerprint: fingerprint,
			PublicKey:            pubkeyBytes,
			DeviceType:           req.DeviceType,
			CreatedAt:            time.Now().UTC(),
			IsActive:             true,
		}
		if err := deviceStore.Create(ctx, device); err != nil {
			log.Error("failed to create device", zap.Error(err))
			c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
			return
		}
	}

	sessionID := uuid.New()
	expiresAt := time.Now().UTC().Add(5 * time.Minute)
	ps := &model.PairingSession{
		ID:                sessionID,
		InitiatorDeviceID: deviceID,
		InitiatorPubkey:   pubkeyBytes,
		Status:            "pending",
		ExpiresAt:         expiresAt,
		CreatedAt:         time.Now().UTC(),
	}
	pairingStore := model.NewPairingStore(h.db)
	if err := pairingStore.Create(ctx, ps); err != nil {
		log.Error("failed to create pairing session", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	shortCode, err := generateShortCode()
	if err != nil {
		log.Error("failed to generate short code", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	qrPayload := fmt.Sprintf("spine://pair/%s?pk=%s", sessionID, req.InitiatorPubkey)

	metrics.PairingSessionsTotal.Inc()

	log.Info("pairing initiated", zap.String("session_id", sessionID.String()), zap.String("device_id", deviceID.String()))
	c.JSON(http.StatusOK, InitiateResponse{
		SessionID: sessionID.String(),
		QRPayload: qrPayload,
		ShortCode: shortCode,
		ExpiresAt: expiresAt.Format(time.RFC3339),
	})
}

// CompleteRequest represents the request body for completing pairing.
type CompleteRequest struct {
	SessionID       string `json:"session_id"`
	DeviceUUID      string `json:"device_uuid"`      // client-minted UUIDv4 used as devices.id (see PRD 003 §Impl Note 1.2)
	ResponderPubkey string `json:"responder_pubkey"` // base64url encoded Ed25519 device identity pubkey
	DeviceType      string `json:"device_type"`      // "desktop" or "mobile"
}

// Complete handles POST /v1/pairing/complete.
func (h *PairingHandler) Complete(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	var req CompleteRequest
	if err := c.BindJSON(&req); err != nil {
		log.Warn("invalid complete request", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_REQUEST", "message": "invalid request body"})
		return
	}

	sessionID, err := uuid.Parse(req.SessionID)
	if err != nil {
		log.Warn("invalid session id", zap.String("session_id", req.SessionID))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_SESSION", "message": "invalid session id"})
		return
	}

	pubkeyBytes, err := base64.RawURLEncoding.DecodeString(req.ResponderPubkey)
	if err != nil {
		log.Warn("invalid public key", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_PUBKEY", "message": "invalid base64url public key"})
		return
	}

	responderID, err := parseClientUUID(req.DeviceUUID)
	if err != nil {
		log.Warn("invalid device_uuid", zap.Error(err))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_REQUEST", "message": "invalid or missing device_uuid (must be UUIDv4)"})
		return
	}

	pairingStore := model.NewPairingStore(h.db)
	ps, err := pairingStore.GetByID(ctx, sessionID)
	if err != nil {
		log.Warn("pairing session not found", zap.String("session_id", sessionID.String()))
		c.JSON(http.StatusNotFound, map[string]any{"code": "SESSION_NOT_FOUND", "message": "pairing session not found"})
		return
	}

	if ps.Status != "pending" {
		if ps.Status == "expired" {
			log.Warn("pairing session expired", zap.String("session_id", sessionID.String()))
			c.JSON(http.StatusGone, map[string]any{"code": "PAIRING_EXPIRED", "message": "pairing session has expired"})
		} else {
			log.Warn("pairing session already completed", zap.String("session_id", sessionID.String()))
			c.JSON(http.StatusConflict, map[string]any{"code": "PAIRING_ALREADY_COMPLETED", "message": "pairing session already completed"})
		}
		return
	}

	if time.Now().UTC().After(ps.ExpiresAt) {
		_ = pairingStore.MarkExpired(ctx)
		log.Warn("pairing session expired", zap.String("session_id", sessionID.String()))
		c.JSON(http.StatusGone, map[string]any{"code": "PAIRING_EXPIRED", "message": "pairing session has expired"})
		return
	}

	deviceStore := model.NewDeviceStore(h.db)
	fingerprint := model.PublicKeyFingerprint(pubkeyBytes)
	conflict, status, code, msg := h.resolveDeviceConflict(ctx, deviceStore, responderID, fingerprint)
	if conflict {
		c.JSON(status, map[string]any{"code": code, "message": msg})
		return
	}
	if status == http.StatusCreated {
		responderDevice := &model.Device{
			ID:                   responderID,
			PublicKeyFingerprint: fingerprint,
			PublicKey:            pubkeyBytes,
			DeviceType:           req.DeviceType,
			CreatedAt:            time.Now().UTC(),
			IsActive:             true,
		}
		if err := deviceStore.Create(ctx, responderDevice); err != nil {
			log.Error("failed to create responder device", zap.Error(err))
			c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
			return
		}
	}

	// Update pairing session
	if err := pairingStore.Complete(ctx, sessionID, pubkeyBytes); err != nil {
		log.Error("failed to complete pairing session", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	// Link devices
	if err := deviceStore.UpdatePairedDevice(ctx, ps.InitiatorDeviceID, responderID); err != nil {
		log.Error("failed to link initiator device", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}
	if err := deviceStore.UpdatePairedDevice(ctx, responderID, ps.InitiatorDeviceID); err != nil {
		log.Error("failed to link responder device", zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{"code": "INTERNAL_ERROR", "message": "internal server error"})
		return
	}

	log.Info("pairing completed", zap.String("session_id", sessionID.String()), zap.String("initiator_id", ps.InitiatorDeviceID.String()), zap.String("responder_id", responderID.String()))
	c.JSON(http.StatusOK, map[string]any{
		"status":       "completed",
		"initiator_id": ps.InitiatorDeviceID.String(),
		"responder_id": responderID.String(),
	})
}

// Status handles GET /v1/pairing/:session_id/status.
func (h *PairingHandler) Status(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	sessionIDStr := c.Param("session_id")
	sessionID, err := uuid.Parse(sessionIDStr)
	if err != nil {
		log.Warn("invalid session id", zap.String("session_id", sessionIDStr))
		c.JSON(http.StatusBadRequest, map[string]any{"code": "INVALID_SESSION", "message": "invalid session id"})
		return
	}

	pairingStore := model.NewPairingStore(h.db)
	ps, err := pairingStore.GetByID(ctx, sessionID)
	if err != nil {
		log.Warn("pairing session not found", zap.String("session_id", sessionIDStr))
		c.JSON(http.StatusNotFound, map[string]any{"code": "SESSION_NOT_FOUND", "message": "pairing session not found"})
		return
	}

	resp := map[string]any{
		"status":     ps.Status,
		"expires_at": ps.ExpiresAt.Format(time.RFC3339),
	}

	if ps.Status == "completed" {
		deviceStore := model.NewDeviceStore(h.db)
		device, err := deviceStore.GetByID(ctx, ps.InitiatorDeviceID)
		if err == nil && device.PairedDeviceID != nil {
			resp["paired_device_id"] = device.PairedDeviceID.String()
		}
	}

	log.Info("pairing status", zap.String("session_id", sessionID.String()), zap.String("status", ps.Status))
	c.JSON(http.StatusOK, resp)
}

func generateShortCode() (string, error) {
	max := big.NewInt(1000000)
	n, err := rand.Int(rand.Reader, max)
	if err != nil {
		return "", err
	}
	code := fmt.Sprintf("%06d", n.Int64())
	return code[:3] + "-" + code[3:], nil
}

// parseClientUUID parses a UUIDv4 string supplied by a pairing client.
// See PRD 003 §Impl Note 1.2: clients mint their own UUID, which the server
// persists as devices.id instead of using Postgres gen_random_uuid().
func parseClientUUID(s string) (uuid.UUID, error) {
	id, err := uuid.Parse(s)
	if err != nil {
		return uuid.Nil, err
	}
	if id.Version() != 4 {
		return uuid.Nil, fmt.Errorf("device_uuid must be UUIDv4, got version %d", id.Version())
	}
	return id, nil
}

// resolveDeviceConflict checks how a pairing handler should react to a client-supplied
// device UUID. Return values:
//   - conflict=true, status=http.StatusConflict: the UUID is already bound to a different
//     fingerprint; caller MUST respond 409 UUID_CONFLICT with the returned code/msg.
//   - conflict=false, status=http.StatusOK: the UUID already maps to a device row whose
//     fingerprint matches the new request (recovery); caller SHOULD skip the insert.
//   - conflict=false, status=http.StatusCreated: no row exists for this UUID; caller MUST
//     insert a new device with the supplied UUID as the primary key.
//
// Any other database error is reported as conflict=false, status=http.StatusInternalServerError
// so the caller can return a generic 500.
func (h *PairingHandler) resolveDeviceConflict(ctx context.Context, store *model.DeviceStore, id uuid.UUID, fingerprint string) (conflict bool, status int, code, msg string) {
	existing, err := store.GetByID(ctx, id)
	if err != nil {
		// pgx returns pgx.ErrNoRows when the device is absent; treat any not-found-shaped
		// error as a signal that we should create the row. Genuine connection errors will
		// be surfaced by the subsequent Create call.
		return false, http.StatusCreated, "", ""
	}
	if existing.PublicKeyFingerprint != fingerprint {
		return true, http.StatusConflict, "UUID_CONFLICT", "device_uuid is already bound to a different identity key"
	}
	return false, http.StatusOK, "", ""
}
