package handler

import (
	"bytes"
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
	"github.com/cloudwego/hertz/pkg/route/param"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
)

func setupSyncIntegrationTestDB(t *testing.T) (*pgxpool.Pool, *redis.Client) {
	dbURL := "postgres://postgres:postgres@localhost:5432/syncmind_test?sslmode=disable"
	pool, err := pgxpool.New(context.Background(), dbURL)
	if err != nil {
		t.Skipf("Skipping test: failed to connect to database: %v", err)
	}
	if err := pool.Ping(context.Background()); err != nil {
		t.Skipf("Skipping test: database unreachable: %v", err)
	}
	_, _ = pool.Exec(context.Background(), "DELETE FROM sync_bundles")
	_, _ = pool.Exec(context.Background(), "DELETE FROM pairing_sessions")
	_, _ = pool.Exec(context.Background(), "DELETE FROM devices")

	rdb := redis.NewClient(&redis.Options{Addr: "localhost:6379"})
	if err := rdb.Ping(context.Background()).Err(); err != nil {
		rdb = nil
	} else {
		_ = rdb.FlushDB(context.Background()).Err()
	}

	return pool, rdb
}

func TestFullSyncFlow(t *testing.T) {
	db, rdb := setupSyncIntegrationTestDB(t)
	defer db.Close()
	if rdb != nil {
		defer rdb.Close()
	}

	pairingHandler := NewPairingHandler(db)
	syncHandler := NewSyncHandler(db, rdb)

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

	// Get device IDs
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
	authMW := middleware.AuthMiddleware(cfg, db, rdb)

	// Device 1 (initiator) uploads a bundle via syncHandler.Upload
	payload := []byte("hello sync bundle payload")
	initToken, err := crypto.SignDeviceJWT(initPriv, initDeviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign initiator token: %v", err)
	}

	ctxUpload := app.NewContext(0)
	ctxUpload.Request.SetBody(payload)
	ctxUpload.Request.Header.Set("Authorization", "Bearer "+initToken)
	ctxUpload.Request.Header.Set("X-Syncmind-Content-Type", "application/octet-stream")
	ctxUpload.Request.Header.Set("Content-Type", "application/octet-stream")

	// Run auth middleware then upload handler
	uploadCalled := false
	uploadHandler := func(c context.Context, h *app.RequestContext) {
		uploadCalled = true
		syncHandler.Upload(c, h)
	}
	ctxUpload.SetHandlers([]app.HandlerFunc{authMW, uploadHandler})
	ctxUpload.Next(context.Background())
	if !uploadCalled {
		t.Fatalf("upload handler not called, auth failed with status %d: %s", ctxUpload.Response.StatusCode(), ctxUpload.Response.Body())
	}
	if ctxUpload.Response.StatusCode() != http.StatusCreated {
		t.Fatalf("expected 201 for upload, got %d: %s", ctxUpload.Response.StatusCode(), ctxUpload.Response.Body())
	}
	var uploadResp map[string]string
	if err := json.Unmarshal(ctxUpload.Response.Body(), &uploadResp); err != nil {
		t.Fatalf("failed to parse upload response: %v", err)
	}
	bundleID := uploadResp["bundle_id"]
	if bundleID == "" {
		t.Fatal("expected bundle_id in upload response")
	}

	// Verify bundle is stored in DB
	bundleStore := model.NewBundleStore(db)
	bundle, err := bundleStore.GetByID(context.Background(), uuid.MustParse(bundleID))
	if err != nil {
		t.Fatalf("failed to get bundle from DB: %v", err)
	}
	if !bytes.Equal(bundle.EncryptedPayload, payload) {
		t.Fatal("bundle payload mismatch")
	}
	if bundle.FromDeviceID != initDeviceID {
		t.Fatal("bundle from_device_id mismatch")
	}
	if bundle.ToDeviceID != respDeviceID {
		t.Fatal("bundle to_device_id mismatch")
	}

	// Device 2 (responder) lists bundles via syncHandler.List
	respToken, err := crypto.SignDeviceJWT(respPriv, respDeviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign responder token: %v", err)
	}

	ctxList := app.NewContext(0)
	ctxList.Request.Header.Set("Authorization", "Bearer "+respToken)

	listCalled := false
	listHandler := func(c context.Context, h *app.RequestContext) {
		listCalled = true
		syncHandler.List(c, h)
	}
	ctxList.SetHandlers([]app.HandlerFunc{authMW, listHandler})
	ctxList.Next(context.Background())
	if !listCalled {
		t.Fatalf("list handler not called, auth failed with status %d: %s", ctxList.Response.StatusCode(), ctxList.Response.Body())
	}
	if ctxList.Response.StatusCode() != http.StatusOK {
		t.Fatalf("expected 200 for list, got %d: %s", ctxList.Response.StatusCode(), ctxList.Response.Body())
	}
	var listResp []map[string]any
	if err := json.Unmarshal(ctxList.Response.Body(), &listResp); err != nil {
		t.Fatalf("failed to parse list response: %v", err)
	}
	if len(listResp) != 1 {
		t.Fatalf("expected 1 bundle in list, got %d", len(listResp))
	}
	if listResp[0]["bundle_id"] != bundleID {
		t.Fatalf("expected bundle_id %s, got %v", bundleID, listResp[0]["bundle_id"])
	}

	// Device 2 downloads the bundle via syncHandler.Download
	ctxDownload := app.NewContext(0)
	ctxDownload.Request.Header.Set("Authorization", "Bearer "+respToken)
	ctxDownload.Params = param.Params{{Key: "id", Value: bundleID}}

	downloadCalled := false
	downloadHandler := func(c context.Context, h *app.RequestContext) {
		downloadCalled = true
		syncHandler.Download(c, h)
	}
	ctxDownload.SetHandlers([]app.HandlerFunc{authMW, downloadHandler})
	ctxDownload.Next(context.Background())
	if !downloadCalled {
		t.Fatalf("download handler not called, auth failed with status %d: %s", ctxDownload.Response.StatusCode(), ctxDownload.Response.Body())
	}
	if ctxDownload.Response.StatusCode() != http.StatusOK {
		t.Fatalf("expected 200 for download, got %d: %s", ctxDownload.Response.StatusCode(), ctxDownload.Response.Body())
	}
	if !bytes.Equal(ctxDownload.Response.Body(), payload) {
		t.Fatal("downloaded payload mismatch")
	}

	// Device 2 acks the bundle via syncHandler.Ack
	ctxAck := app.NewContext(0)
	ctxAck.Request.Header.Set("Authorization", "Bearer "+respToken)
	ctxAck.Params = param.Params{{Key: "id", Value: bundleID}}

	ackCalled := false
	ackHandler := func(c context.Context, h *app.RequestContext) {
		ackCalled = true
		syncHandler.Ack(c, h)
	}
	ctxAck.SetHandlers([]app.HandlerFunc{authMW, ackHandler})
	ctxAck.Next(context.Background())
	if !ackCalled {
		t.Fatalf("ack handler not called, auth failed with status %d: %s", ctxAck.Response.StatusCode(), ctxAck.Response.Body())
	}
	if ctxAck.Response.StatusCode() != http.StatusNoContent {
		t.Fatalf("expected 204 for ack, got %d: %s", ctxAck.Response.StatusCode(), ctxAck.Response.Body())
	}

	// Verify ack updates acked_at in DB
	bundleAfterAck, err := bundleStore.GetByID(context.Background(), uuid.MustParse(bundleID))
	if err != nil {
		t.Fatalf("failed to get bundle after ack: %v", err)
	}
	if bundleAfterAck.AckedAt == nil {
		t.Fatal("expected acked_at to be set after ack")
	}
	if bundleAfterAck.AckedAt.After(time.Now().UTC().Add(time.Minute)) || bundleAfterAck.AckedAt.Before(time.Now().UTC().Add(-time.Minute)) {
		t.Fatalf("acked_at seems unreasonable: %v", *bundleAfterAck.AckedAt)
	}
}
