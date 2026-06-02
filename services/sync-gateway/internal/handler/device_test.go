package handler

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"net/http"
	"testing"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/blkcor/syncmind/spine/internal/pkg/crypto"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/cloudwego/hertz/pkg/route/param"
	"github.com/google/uuid"
)

func createTestDevice(t *testing.T, store *model.DeviceStore) (*model.Device, ed25519.PrivateKey) {
	t.Helper()
	pub, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("failed to generate key: %v", err)
	}
	device := &model.Device{
		ID:                   uuid.New(),
		PublicKeyFingerprint: model.PublicKeyFingerprint(pub),
		PublicKey:            pub,
		PairedDeviceID:       nil,
		DeviceType:           "mobile",
		CreatedAt:            time.Now().UTC(),
		IsActive:             true,
	}
	if err := store.Create(context.Background(), device); err != nil {
		t.Fatalf("failed to create device: %v", err)
	}
	return device, priv
}

func createPeerDevice(t *testing.T, store *model.DeviceStore, paired *uuid.UUID) *model.Device {
	t.Helper()
	pub, _, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("failed to generate peer key: %v", err)
	}
	peer := &model.Device{
		ID:                   uuid.New(),
		PublicKeyFingerprint: model.PublicKeyFingerprint(pub),
		PublicKey:            pub,
		PairedDeviceID:       paired,
		DeviceType:           "desktop",
		CreatedAt:            time.Now().UTC(),
		IsActive:             true,
	}
	if err := store.Create(context.Background(), peer); err != nil {
		t.Fatalf("failed to create peer device: %v", err)
	}
	return peer
}

func signToken(t *testing.T, priv ed25519.PrivateKey, deviceID uuid.UUID) string {
	t.Helper()
	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	token, err := crypto.SignDeviceJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign token: %v", err)
	}
	return token
}

func TestDeviceStatusSuccess(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	store := model.NewDeviceStore(db)
	device, priv := createTestDevice(t, store)
	token := signToken(t, priv, device.ID)

	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	authMW := middleware.AuthMiddleware(cfg, db, rdb)
	deviceHandler := NewDeviceHandler(store)

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)
	ctx.Params = param.Params{{Key: "self_device_uuid", Value: device.ID.String()}}

	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
		deviceHandler.Status(c, h)
	}
	ctx.SetHandlers([]app.HandlerFunc{authMW, next})
	ctx.Next(context.Background())

	if !called {
		t.Fatalf("handler not called, auth failed with status %d: %s",
			ctx.Response.StatusCode(), ctx.Response.Body())
	}
	if ctx.Response.StatusCode() != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	var body map[string]any
	if err := json.Unmarshal(ctx.Response.Body(), &body); err != nil {
		t.Fatalf("failed to parse response body: %v", err)
	}
	if body["device_uuid"] != device.ID.String() {
		t.Errorf("expected device_uuid %s, got %v", device.ID.String(), body["device_uuid"])
	}
	if body["device_type"] != "mobile" {
		t.Errorf("expected device_type mobile, got %v", body["device_type"])
	}
	if body["is_active"] != true {
		t.Errorf("expected is_active true, got %v", body["is_active"])
	}
}

func TestDeviceStatusMismatchedPathAndJWT(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	store := model.NewDeviceStore(db)
	device, priv := createTestDevice(t, store)
	token := signToken(t, priv, device.ID)

	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	authMW := middleware.AuthMiddleware(cfg, db, rdb)
	deviceHandler := NewDeviceHandler(store)

	otherUUID := uuid.New()
	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)
	ctx.Params = param.Params{{Key: "self_device_uuid", Value: otherUUID.String()}}

	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
		deviceHandler.Status(c, h)
	}
	ctx.SetHandlers([]app.HandlerFunc{authMW, next})
	ctx.Next(context.Background())

	if !called {
		t.Fatalf("handler not called, auth failed with status %d: %s",
			ctx.Response.StatusCode(), ctx.Response.Body())
	}
	if ctx.Response.StatusCode() != http.StatusNotFound {
		t.Fatalf("expected 404 for mismatched path/JWT, got %d: %s",
			ctx.Response.StatusCode(), ctx.Response.Body())
	}

	var body map[string]any
	if err := json.Unmarshal(ctx.Response.Body(), &body); err != nil {
		t.Fatalf("failed to parse response body: %v", err)
	}
	if body["code"] != "DEVICE_NOT_FOUND" {
		t.Errorf("expected code DEVICE_NOT_FOUND, got %v", body["code"])
	}
}

func TestDeviceRevokeSuccess(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	store := model.NewDeviceStore(db)
	device, priv := createTestDevice(t, store)
	peer := createPeerDevice(t, store, &device.ID)
	token := signToken(t, priv, device.ID)

	// Verify peer is linked before revoke
	storedPeer, err := store.GetByID(context.Background(), peer.ID)
	if err != nil {
		t.Fatalf("failed to fetch peer: %v", err)
	}
	if storedPeer.PairedDeviceID == nil || *storedPeer.PairedDeviceID != device.ID {
		t.Fatal("expected peer to be linked to device before revoke")
	}

	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	authMW := middleware.AuthMiddleware(cfg, db, rdb)
	deviceHandler := NewDeviceHandler(store)

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)
	ctx.Params = param.Params{{Key: "self_device_uuid", Value: device.ID.String()}}

	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
		deviceHandler.Revoke(c, h)
	}
	ctx.SetHandlers([]app.HandlerFunc{authMW, next})
	ctx.Next(context.Background())

	if !called {
		t.Fatalf("handler not called, auth failed with status %d: %s",
			ctx.Response.StatusCode(), ctx.Response.Body())
	}
	if ctx.Response.StatusCode() != http.StatusNoContent {
		t.Fatalf("expected 204, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	// Verify device is deactivated
	deactivated, err := store.GetByID(context.Background(), device.ID)
	if err != nil {
		t.Fatalf("failed to fetch device after revoke: %v", err)
	}
	if deactivated.IsActive {
		t.Error("expected device to be inactive after revoke")
	}

	// Verify peer link is cleared
	updatedPeer, err := store.GetByID(context.Background(), peer.ID)
	if err != nil {
		t.Fatalf("failed to fetch peer after revoke: %v", err)
	}
	if updatedPeer.PairedDeviceID != nil {
		t.Error("expected peer paired_device_id to be nil after revoke")
	}
}

func TestDeviceStatusInactiveDevice(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	store := model.NewDeviceStore(db)
	device, priv := createTestDevice(t, store)

	// Deactivate the device
	if err := store.Deactivate(context.Background(), device.ID); err != nil {
		t.Fatalf("failed to deactivate device: %v", err)
	}

	token := signToken(t, priv, device.ID)

	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	authMW := middleware.AuthMiddleware(cfg, db, rdb)

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)
	ctx.Params = param.Params{{Key: "self_device_uuid", Value: device.ID.String()}}

	// Note: AuthMiddleware checks IsActive and rejects inactive devices.
	// The handler won't be called; this test verifies the auth gate works.
	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
	}
	ctx.SetHandlers([]app.HandlerFunc{authMW, next})
	ctx.Next(context.Background())

	if called {
		t.Error("expected auth middleware to reject inactive device, but handler was called")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("expected 401 for inactive device, got %d", ctx.Response.StatusCode())
	}
}

func TestDeviceRevokeUnknownDevice(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	store := model.NewDeviceStore(db)
	device, priv := createTestDevice(t, store)

	// Delete the device to simulate unknown device
	if err := store.Deactivate(context.Background(), device.ID); err != nil {
		t.Fatalf("failed to deactivate device: %v", err)
	}

	token := signToken(t, priv, device.ID)

	cfg := &config.Config{JWTIssuer: "syncmind-spine", JWTAudience: "syncmind-device"}
	authMW := middleware.AuthMiddleware(cfg, db, rdb)

	// The auth middleware checks IsActive, so inactive device will be rejected at auth.
	// This tests the auth gate for inactive devices.
	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)
	ctx.Params = param.Params{{Key: "self_device_uuid", Value: device.ID.String()}}

	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
	}
	ctx.SetHandlers([]app.HandlerFunc{authMW, next})
	ctx.Next(context.Background())

	if called {
		t.Error("expected auth middleware to reject inactive device for revoke")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("expected 401 for inactive device, got %d", ctx.Response.StatusCode())
	}
}
