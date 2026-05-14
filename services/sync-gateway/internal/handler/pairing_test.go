package handler

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"os"
	"testing"

	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/cloudwego/hertz/pkg/protocol/consts"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

func setupTestDB(t *testing.T) *pgxpool.Pool {
	dbURL := os.Getenv("TEST_DATABASE_URL")
	if dbURL == "" {
		dbURL = "postgres://postgres:postgres@localhost:5432/syncmind_test?sslmode=disable"
	}

	pool, err := pgxpool.New(context.Background(), dbURL)
	if err != nil {
		t.Skipf("Skipping test: failed to connect to database: %v", err)
	}

	if err := pool.Ping(context.Background()); err != nil {
		t.Skipf("Skipping test: database unreachable: %v", err)
	}

	// Clean tables for test isolation
	_, _ = pool.Exec(context.Background(), "DELETE FROM sync_bundles")
	_, _ = pool.Exec(context.Background(), "DELETE FROM pairing_sessions")
	_, _ = pool.Exec(context.Background(), "DELETE FROM devices")

	return pool
}

func TestPairingInitiate(t *testing.T) {
	pool := setupTestDB(t)
	defer pool.Close()

	handler := NewPairingHandler(pool)

	pubkey := make([]byte, 32)
	for i := range pubkey {
		pubkey[i] = byte(i)
	}

	reqBody, _ := json.Marshal(map[string]string{
		"initiator_pubkey": base64.RawURLEncoding.EncodeToString(pubkey),
		"device_type":      "desktop",
	})

	ctx := app.NewContext(0)
	ctx.Request.SetBody(reqBody)
	ctx.Request.Header.Set("Content-Type", "application/json")

	handler.Initiate(context.Background(), ctx)

	if ctx.Response.StatusCode() != consts.StatusOK {
		t.Fatalf("Expected 200, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	var resp InitiateResponse
	if err := json.Unmarshal(ctx.Response.Body(), &resp); err != nil {
		t.Fatalf("Failed to parse response: %v", err)
	}

	if resp.SessionID == "" {
		t.Fatal("Expected session_id to be set")
	}
	if resp.QRPayload == "" {
		t.Fatal("Expected qr_payload to be set")
	}
	if resp.ShortCode == "" {
		t.Fatal("Expected short_code to be set")
	}
	if resp.ExpiresAt == "" {
		t.Fatal("Expected expires_at to be set")
	}
}

func TestPairingComplete(t *testing.T) {
	pool := setupTestDB(t)
	defer pool.Close()

	handler := NewPairingHandler(pool)

	initPubkey := make([]byte, 32)
	respPubkey := make([]byte, 32)
	respPubkey[0] = 1

	// Initiate
	reqBody, _ := json.Marshal(map[string]string{
		"initiator_pubkey": base64.RawURLEncoding.EncodeToString(initPubkey),
		"device_type":      "desktop",
	})
	ctx := app.NewContext(0)
	ctx.Request.SetBody(reqBody)
	ctx.Request.Header.Set("Content-Type", "application/json")
	handler.Initiate(context.Background(), ctx)

	var initResp InitiateResponse
	json.Unmarshal(ctx.Response.Body(), &initResp)

	// Complete
	completeBody, _ := json.Marshal(map[string]string{
		"session_id":       initResp.SessionID,
		"responder_pubkey": base64.RawURLEncoding.EncodeToString(respPubkey),
		"device_type":      "mobile",
	})
	ctx2 := app.NewContext(0)
	ctx2.Request.SetBody(completeBody)
	ctx2.Request.Header.Set("Content-Type", "application/json")
	handler.Complete(context.Background(), ctx2)

	if ctx2.Response.StatusCode() != consts.StatusOK {
		t.Fatalf("Expected 200, got %d: %s", ctx2.Response.StatusCode(), ctx2.Response.Body())
	}

	// Verify devices are linked
	pairingStore := model.NewPairingStore(pool)
	session, err := pairingStore.GetByID(context.Background(), uuid.MustParse(initResp.SessionID))
	if err != nil {
		t.Fatalf("Failed to get session: %v", err)
	}
	if session.Status != "completed" {
		t.Fatalf("Expected status completed, got %s", session.Status)
	}
}

func TestPairingExpired(t *testing.T) {
	pool := setupTestDB(t)
	defer pool.Close()

	handler := NewPairingHandler(pool)

	// Try to complete non-existent session
	completeBody, _ := json.Marshal(map[string]string{
		"session_id":       "00000000-0000-0000-0000-000000000000",
		"responder_pubkey": base64.RawURLEncoding.EncodeToString(make([]byte, 32)),
		"device_type":      "mobile",
	})
	ctx := app.NewContext(0)
	ctx.Request.SetBody(completeBody)
	ctx.Request.Header.Set("Content-Type", "application/json")
	handler.Complete(context.Background(), ctx)

	if ctx.Response.StatusCode() != consts.StatusNotFound {
		t.Fatalf("Expected 404, got %d", ctx.Response.StatusCode())
	}
}
