package middleware

import (
	"context"
	"crypto/ed25519"
	"net/http"
	"testing"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
)

func setupAuthTestDB(t *testing.T) (*pgxpool.Pool, *redis.Client) {
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

func createTestDevice(t *testing.T, db *pgxpool.Pool) (uuid.UUID, ed25519.PrivateKey) {
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
	return deviceID, priv
}

func signTestJWT(priv ed25519.PrivateKey, deviceID uuid.UUID, issuer, audience string, exp time.Time, jti string) string {
	claims := jwt.MapClaims{
		"sub": deviceID.String(),
		"iss": issuer,
		"aud": audience,
		"iat": time.Now().UTC().Add(-time.Minute).Unix(),
		"exp": exp.Unix(),
	}
	if jti != "" {
		claims["jti"] = jti
	}
	token := jwt.NewWithClaims(jwt.SigningMethodEdDSA, claims)
	s, _ := token.SignedString(priv)
	return s
}

func runAuthMiddleware(mw app.HandlerFunc, next app.HandlerFunc, ctx *app.RequestContext) {
	ctx.SetHandlers([]app.HandlerFunc{mw, next})
	ctx.Next(context.Background())
}

func TestAuthMiddlewareValidToken(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), uuid.New().String())

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) {
		called = true
		val, ok := h.Get(DeviceIDKey)
		if !ok || val.(uuid.UUID) != deviceID {
			t.Fatal("device_id not set correctly in context")
		}
	}
	runAuthMiddleware(mw, next, ctx)

	if !called {
		t.Fatalf("Expected next handler to be called, got status %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	// Verify last_seen_at was updated asynchronously.
	time.Sleep(200 * time.Millisecond)
	store := model.NewDeviceStore(db)
	device, err := store.GetByID(context.Background(), deviceID)
	if err != nil {
		t.Fatalf("failed to get device: %v", err)
	}
	if device.LastSeenAt == nil {
		t.Fatal("expected last_seen_at to be set after successful auth")
	}
	if device.LastSeenAt.Before(time.Now().UTC().Add(-time.Minute)) {
		t.Fatalf("last_seen_at seems stale: %v", *device.LastSeenAt)
	}
}

func TestAuthMiddlewareStoresJTI(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	jti := uuid.New().String()
	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), jti)

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	var gotJTI string
	next := func(c context.Context, h *app.RequestContext) {
		val, ok := h.Get("jti")
		if ok {
			gotJTI = val.(string)
		}
	}
	runAuthMiddleware(mw, next, ctx)

	if gotJTI != jti {
		t.Fatalf("expected jti %q in context, got %q", jti, gotJTI)
	}
}

func TestAuthMiddlewareStoresExp(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	expTime := time.Now().UTC().Add(time.Hour)
	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, expTime, uuid.New().String())

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	var gotExp int64
	next := func(c context.Context, h *app.RequestContext) {
		val, ok := h.Get("exp")
		if ok {
			gotExp = val.(int64)
		}
	}
	runAuthMiddleware(mw, next, ctx)

	if gotExp == 0 {
		t.Fatal("expected exp to be stored in context")
	}
	if gotExp != expTime.Unix() {
		t.Fatalf("expected exp %d in context, got %d", expTime.Unix(), gotExp)
	}
}

func TestAuthMiddlewareNoJTISkipsBlacklistCheck(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	// Sign a token without jti claim
	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), "")

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if !called {
		t.Fatalf("expected next handler to be called for token without jti, got status %d", ctx.Response.StatusCode())
	}
}

func TestAuthMiddlewareExpiredToken(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(-time.Hour), uuid.New().String())

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if called {
		t.Fatal("Expected next handler NOT to be called for expired token")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("Expected 401, got %d", ctx.Response.StatusCode())
	}
}

func TestAuthMiddlewareReplayedToken(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	jti := uuid.New().String()
	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), jti)

	// Blacklist the jti
	_ = rdb.Set(context.Background(), "jwt:blacklist:"+jti, "1", time.Hour).Err()

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if called {
		t.Fatal("Expected next handler NOT to be called for replayed token")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("Expected 401, got %d", ctx.Response.StatusCode())
	}
}

func TestAuthMiddlewareInvalidSignature(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, _ := createTestDevice(t, db)
	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	// Sign with a different key
	_, wrongPriv, _ := ed25519.GenerateKey(nil)
	token := signTestJWT(wrongPriv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), uuid.New().String())

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if called {
		t.Fatal("Expected next handler NOT to be called for invalid signature")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("Expected 401, got %d", ctx.Response.StatusCode())
	}
}

func TestAuthMiddlewareMissingHeader(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	ctx := app.NewContext(0)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if called {
		t.Fatal("Expected next handler NOT to be called for missing header")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("Expected 401, got %d", ctx.Response.StatusCode())
	}
}

func TestAuthMiddlewareDeactivatedDevice(t *testing.T) {
	db, rdb := setupAuthTestDB(t)
	defer db.Close()
	defer rdb.Close()

	deviceID, priv := createTestDevice(t, db)

	// Deactivate device
	_, _ = db.Exec(context.Background(), "UPDATE devices SET is_active = false WHERE id = $1", deviceID)

	cfg := &config.Config{JWTIssuer: "syncmind", JWTAudience: "spine"}
	mw := AuthMiddleware(cfg, db, rdb)

	token := signTestJWT(priv, deviceID, cfg.JWTIssuer, cfg.JWTAudience, time.Now().UTC().Add(time.Hour), uuid.New().String())

	ctx := app.NewContext(0)
	ctx.Request.Header.Set("Authorization", "Bearer "+token)

	called := false
	next := func(c context.Context, h *app.RequestContext) { called = true }
	runAuthMiddleware(mw, next, ctx)

	if called {
		t.Fatal("Expected next handler NOT to be called for deactivated device")
	}
	if ctx.Response.StatusCode() != http.StatusUnauthorized {
		t.Fatalf("Expected 401, got %d", ctx.Response.StatusCode())
	}
}
