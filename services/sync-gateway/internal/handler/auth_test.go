package handler

import (
	"context"
	"crypto/ed25519"
	"net/http"
	"testing"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/blkcor/syncmind/spine/internal/pkg/crypto"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
)

func setupAuthHandlerTestDB(t *testing.T) (*pgxpool.Pool, *redis.Client) {
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
		t.Skipf("Skipping test: redis unreachable: %v", err)
	}
	_ = rdb.FlushDB(context.Background()).Err()

	return pool, rdb
}

func TestTokenRevokeAndReuse(t *testing.T) {
	db, rdb := setupAuthHandlerTestDB(t)
	defer db.Close()
	defer rdb.Close()

	// Create a test device with Ed25519 keypair
	_, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("failed to generate key: %v", err)
	}
	deviceID := uuid.New()
	device := &model.Device{
		ID:                   deviceID,
		PublicKeyFingerprint: model.PublicKeyFingerprint(priv.Public().(ed25519.PublicKey)),
		PublicKey:            priv.Public().(ed25519.PublicKey),
		DeviceType:           "desktop",
		IsActive:             true,
	}
	store := model.NewDeviceStore(db)
	if err := store.Create(context.Background(), device); err != nil {
		t.Fatalf("failed to create device: %v", err)
	}

	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	token, err := crypto.SignDeviceJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Hour)
	if err != nil {
		t.Fatalf("failed to sign token: %v", err)
	}

	authMW := middleware.AuthMiddleware(cfg, db, rdb)
	authHandler := NewAuthHandler(rdb)

	// First request: verify token works
	ctx1 := app.NewContext(0)
	ctx1.Request.Header.Set("Authorization", "Bearer "+token)
	called1 := false
	next1 := func(c context.Context, h *app.RequestContext) { called1 = true }
	ctx1.SetHandlers([]app.HandlerFunc{authMW, next1})
	ctx1.Next(context.Background())
	if !called1 {
		t.Fatalf("expected token to be valid initially, got status %d: %s", ctx1.Response.StatusCode(), ctx1.Response.Body())
	}

	// Revoke the token
	ctxRevoke := app.NewContext(0)
	ctxRevoke.Request.Header.Set("Authorization", "Bearer "+token)
	revokeCalled := false
	revokeNext := func(c context.Context, h *app.RequestContext) {
		revokeCalled = true
		authHandler.Revoke(c, h)
	}
	ctxRevoke.SetHandlers([]app.HandlerFunc{authMW, revokeNext})
	ctxRevoke.Next(context.Background())
	if !revokeCalled {
		t.Fatalf("revoke handler not called, auth failed with status %d: %s", ctxRevoke.Response.StatusCode(), ctxRevoke.Response.Body())
	}
	if ctxRevoke.Response.StatusCode() != http.StatusNoContent {
		t.Fatalf("expected 204 for revoke, got %d: %s", ctxRevoke.Response.StatusCode(), ctxRevoke.Response.Body())
	}

	// Extract jti from the signed token for Redis verification.
	parsedToken, _, err := new(jwt.Parser).ParseUnverified(token, jwt.MapClaims{})
	if err != nil {
		t.Fatalf("failed to parse token for jti extraction: %v", err)
	}
	claims, _ := parsedToken.Claims.(jwt.MapClaims)
	extractedJTI, _ := claims["jti"].(string)

	// Verify jti is blacklisted in Redis
	if extractedJTI != "" {
		blacklistKey := "jwt:blacklist:" + extractedJTI
		ttl, err := rdb.TTL(context.Background(), blacklistKey).Result()
		if err != nil {
			t.Fatalf("failed to check blacklist ttl: %v", err)
		}
		if ttl <= 0 {
			t.Fatalf("expected blacklist entry to have positive ttl, got %v", ttl)
		}
	}

	// Second request: verify revoked token is rejected
	ctx2 := app.NewContext(0)
	ctx2.Request.Header.Set("Authorization", "Bearer "+token)
	called2 := false
	next2 := func(c context.Context, h *app.RequestContext) { called2 = true }
	ctx2.SetHandlers([]app.HandlerFunc{authMW, next2})
	ctx2.Next(context.Background())
	if called2 {
		t.Fatal("expected revoked token to be rejected")
	}
	if ctx2.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("expected 401 for revoked token, got %d", ctx2.Response.StatusCode())
	}
}
