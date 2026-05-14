package handler

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"testing"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/blkcor/syncmind/spine/internal/pkg/crypto"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
)

func TestDeviceDeactivationFlow(t *testing.T) {
	db := setupTestDB(t)
	defer db.Close()

	pairingHandler := NewPairingHandler(db)

	// Generate keypairs for two devices
	_, initPriv, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("failed to generate initiator key: %v", err)
	}
	initPubkey := initPriv.Public().(ed25519.PublicKey)

	_, respPriv, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("failed to generate responder key: %v", err)
	}
	respPubkey := respPriv.Public().(ed25519.PublicKey)

	// Pair devices via pairing handlers
	reqBody, _ := json.Marshal(map[string]string{
		"initiator_pubkey": base64.RawURLEncoding.EncodeToString(initPubkey),
		"device_type":      "desktop",
	})
	ctx := app.NewContext(0)
	ctx.Request.SetBody(reqBody)
	ctx.Request.Header.Set("Content-Type", "application/json")
	pairingHandler.Initiate(context.Background(), ctx)
	if ctx.Response.StatusCode() != http.StatusOK {
		t.Fatalf("initiate failed: %d %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}
	var initResp InitiateResponse
	if err := json.Unmarshal(ctx.Response.Body(), &initResp); err != nil {
		t.Fatalf("failed to parse initiate response: %v", err)
	}

	completeBody, _ := json.Marshal(map[string]string{
		"session_id":       initResp.SessionID,
		"responder_pubkey": base64.RawURLEncoding.EncodeToString(respPubkey),
		"device_type":      "mobile",
	})
	ctx2 := app.NewContext(0)
	ctx2.Request.SetBody(completeBody)
	ctx2.Request.Header.Set("Content-Type", "application/json")
	pairingHandler.Complete(context.Background(), ctx2)
	if ctx2.Response.StatusCode() != http.StatusOK {
		t.Fatalf("complete failed: %d %s", ctx2.Response.StatusCode(), ctx2.Response.Body())
	}

	// Get the actual device IDs from DB
	pairingStore := model.NewPairingStore(db)
	session, err := pairingStore.GetByID(context.Background(), uuid.MustParse(initResp.SessionID))
	if err != nil {
		t.Fatalf("failed to get session: %v", err)
	}
	initDeviceID := session.InitiatorDeviceID

	deviceStore := model.NewDeviceStore(db)
	initDevice, err := deviceStore.GetByID(context.Background(), initDeviceID)
	if err != nil {
		t.Fatalf("failed to get initiator device: %v", err)
	}
	if initDevice.PairedDeviceID == nil {
		t.Fatal("expected initiator device to have a paired device")
	}
	respDeviceID := *initDevice.PairedDeviceID

	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}

	// Verify both devices can access protected endpoints initially
	authMW := middleware.AuthMiddleware(cfg, db, nil)

	initToken, err := crypto.SignDeviceJWT(initPriv, initDeviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign initiator token: %v", err)
	}
	respToken, err := crypto.SignDeviceJWT(respPriv, respDeviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign responder token: %v", err)
	}

	// Initiator request should succeed
	ctxInit := app.NewContext(0)
	ctxInit.Request.Header.Set("Authorization", "Bearer "+initToken)
	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	ctxInit.SetHandlers([]app.HandlerFunc{authMW, next})
	ctxInit.Next(context.Background())
	if !called {
		t.Fatalf("expected initiator to be authorized, got status %d", ctxInit.Response.StatusCode())
	}

	// Responder request should succeed
	ctxResp := app.NewContext(0)
	ctxResp.Request.Header.Set("Authorization", "Bearer "+respToken)
	called = false
	ctxResp.SetHandlers([]app.HandlerFunc{authMW, next})
	ctxResp.Next(context.Background())
	if !called {
		t.Fatalf("expected responder to be authorized, got status %d", ctxResp.Response.StatusCode())
	}

	// Deactivate initiator device
	_, err = db.Exec(context.Background(), "UPDATE devices SET is_active = false WHERE id = $1", initDeviceID)
	if err != nil {
		t.Fatalf("failed to deactivate device: %v", err)
	}

	// Deactivated device should get 401
	ctxInit2 := app.NewContext(0)
	ctxInit2.Request.Header.Set("Authorization", "Bearer "+initToken)
	called = false
	ctxInit2.SetHandlers([]app.HandlerFunc{authMW, next})
	ctxInit2.Next(context.Background())
	if called {
		t.Fatal("expected next handler NOT to be called for deactivated device")
	}
	if ctxInit2.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("expected 401 for deactivated device, got %d", ctxInit2.Response.StatusCode())
	}

	// Active device (responder) should still work
	ctxResp2 := app.NewContext(0)
	ctxResp2.Request.Header.Set("Authorization", "Bearer "+respToken)
	called = false
	ctxResp2.SetHandlers([]app.HandlerFunc{authMW, next})
	ctxResp2.Next(context.Background())
	if !called {
		t.Fatalf("expected responder to still be authorized after initiator deactivation, got status %d", ctxResp2.Response.StatusCode())
	}
}
