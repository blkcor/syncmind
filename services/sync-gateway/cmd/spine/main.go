package main

import (
	"bytes"
	"context"
	"crypto/tls"
	"net/http"
	"os"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/handler"
	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/middleware"
	"github.com/blkcor/syncmind/spine/internal/pkg/websocket"
	"github.com/blkcor/syncmind/spine/internal/scheduler"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/cloudwego/hertz/pkg/app/server"
	hertzconfig "github.com/cloudwego/hertz/pkg/common/config"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/common/expfmt"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

func main() {
	log := logger.Init()
	defer logger.Sync()

	cfg, err := config.Load("spine.yaml")
	if err != nil {
		log.Error("failed to load config", zap.Error(err))
		os.Exit(1)
	}

	ctx := context.Background()

	dbPool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Error("failed to connect to database", zap.Error(err))
		os.Exit(1)
	}
	defer dbPool.Close()

	rdb := redis.NewClient(&redis.Options{
		Addr: cfg.RedisAddr,
	})
	defer rdb.Close()

	serverOpts := []hertzconfig.Option{server.WithHostPorts(cfg.BindAddr)}
	if cfg.TLSCert != "" && cfg.TLSKey != "" {
		cert, err := tls.LoadX509KeyPair(cfg.TLSCert, cfg.TLSKey)
		if err != nil {
			log.Error("failed to load TLS cert", zap.Error(err))
			os.Exit(1)
		}
		serverOpts = append(serverOpts, server.WithTLS(
			&tls.Config{
				MinVersion:   tls.VersionTLS13,
				Certificates: []tls.Certificate{cert},
			}))
	}
	h := server.Default(serverOpts...)

	// Request logging middleware (must be early to capture all requests)
	h.Use(middleware.RequestLogger())

	// Security headers
	h.Use(middleware.SecurityHeaders())

	// Health check
	h.GET("/health", func(c context.Context, ctx *app.RequestContext) {
		healthy := true
		status := map[string]any{
			"status":    "healthy",
			"timestamp": time.Now().UTC().Format(time.RFC3339),
		}

		if err := dbPool.Ping(c); err != nil {
			healthy = false
			status["postgres"] = "unreachable"
		} else {
			status["postgres"] = "ok"
		}

		if err := rdb.Ping(c).Err(); err != nil {
			healthy = false
			status["redis"] = "unreachable"
		} else {
			status["redis"] = "ok"
		}

		if !healthy {
			status["status"] = "unhealthy"
			ctx.JSON(http.StatusServiceUnavailable, status)
			return
		}

		ctx.JSON(http.StatusOK, status)
	})

	// Initialize handlers
	pairingHandler := handler.NewPairingHandler(dbPool)
	syncHandler := handler.NewSyncHandler(dbPool, rdb)
	mediaHandler := handler.NewMediaHandler(dbPool, rdb)
	authHandler := handler.NewAuthHandler(rdb)
	wsHub := websocket.NewHub()

	// Rate limiters
	pairingRateLimit := middleware.RateLimiter(rdb, 20, time.Minute, func(c *app.RequestContext) string {
		return "global:pairing"
	})
	syncRateLimit := middleware.RateLimiter(rdb, 100, time.Minute, func(c *app.RequestContext) string {
		deviceIDVal, ok := c.Get(middleware.DeviceIDKey)
		if !ok {
			return ""
		}
		return deviceIDVal.(uuid.UUID).String() + ":sync"
	})

	// Pairing routes (no auth required)
	h.POST("/v1/pairing/initiate", pairingRateLimit, pairingHandler.Initiate)
	h.POST("/v1/pairing/complete", pairingRateLimit, pairingHandler.Complete)
	h.GET("/v1/pairing/:session_id/status", pairingHandler.Status)

	// Auth middleware
	authMW := middleware.AuthMiddleware(cfg, dbPool, rdb)

	// Protected routes
	h.POST("/v1/sync/bundle", authMW, syncRateLimit, syncHandler.Upload)
	h.GET("/v1/sync/bundles", authMW, syncHandler.List)
	h.GET("/v1/sync/bundles/:id", authMW, syncHandler.Download)
	h.DELETE("/v1/sync/bundles/:id", authMW, syncHandler.Ack)
	h.POST("/v1/media/upload", authMW, syncRateLimit, mediaHandler.Upload)
	h.POST("/v1/auth/revoke", authMW, authHandler.Revoke)

	// Metrics endpoint
	h.GET("/metrics", func(c context.Context, ctx *app.RequestContext) {
		gatherers := prometheus.Gatherers{prometheus.DefaultGatherer}
		mfs, err := gatherers.Gather()
		if err != nil {
			ctx.SetStatusCode(http.StatusInternalServerError)
			_, _ = ctx.WriteString("failed to gather metrics")
			return
		}
		var buf bytes.Buffer
		enc := expfmt.NewEncoder(&buf, expfmt.NewFormat(expfmt.TypeTextPlain))
		for _, mf := range mfs {
			if err := enc.Encode(mf); err != nil {
				continue
			}
		}
		ctx.Data(http.StatusOK, "text/plain; version=0.0.4; charset=utf-8", buf.Bytes())
	})

	// WebSocket route
	h.GET("/v1/sync/live", authMW, func(c context.Context, ctx *app.RequestContext) {
		deviceIDVal, _ := ctx.Get(middleware.DeviceIDKey)
		deviceID := deviceIDVal.(uuid.UUID)
		wsHub.HandleUpgrade(deviceID)(c, ctx)
	})

	// Start Redis subscriber for sync notifications
	go startRedisSubscriber(ctx, rdb, wsHub)

	// Start cleanup scheduler
	cleanupScheduler := scheduler.NewCleanupScheduler(dbPool)
	cleanupScheduler.Start(ctx)

	log.Info("Spine listening", zap.String("bind_addr", cfg.BindAddr))
	h.Spin()
}

func startRedisSubscriber(ctx context.Context, rdb *redis.Client, hub *websocket.Hub) {
	pubsub := rdb.PSubscribe(ctx, "sync:notify:*")
	defer pubsub.Close()

	ch := pubsub.Channel()
	for msg := range ch {
		deviceIDStr := msg.Channel[len("sync:notify:"):]
		deviceID, err := uuid.Parse(deviceIDStr)
		if err != nil {
			logger.L().Warn("failed to parse device id from redis channel", zap.String("channel", msg.Channel), zap.Error(err))
			continue
		}
		hub.Send(deviceID, []byte(msg.Payload))
	}
}
