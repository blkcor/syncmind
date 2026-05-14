package middleware

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/blkcor/syncmind/spine/internal/config"
	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/model"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

const DeviceIDKey = "device_id"

const maxConcurrentLastSeenUpdates = 50

// AuthMiddleware creates a Hertz middleware that validates Ed25519-signed JWTs.
func AuthMiddleware(cfg *config.Config, db *pgxpool.Pool, rdb *redis.Client) app.HandlerFunc {
	deviceStore := model.NewDeviceStore(db)
	updateSem := make(chan struct{}, maxConcurrentLastSeenUpdates)

	return func(ctx context.Context, c *app.RequestContext) {
		tokenStr := extractBearer(c)
		if tokenStr == "" {
			c.AbortWithStatusJSON(http.StatusUnauthorized, map[string]any{
				"code":    "AUTH_MISSING",
				"message": "missing authorization header",
			})
			return
		}

		token, err := jwt.Parse(tokenStr, func(t *jwt.Token) (any, error) {
			if _, ok := t.Method.(*jwt.SigningMethodEd25519); !ok {
				return nil, jwt.ErrSignatureInvalid
			}
			claims, ok := t.Claims.(jwt.MapClaims)
			if !ok {
				return nil, jwt.ErrTokenInvalidClaims
			}
			sub, ok := claims["sub"].(string)
			if !ok {
				return nil, jwt.ErrTokenInvalidClaims
			}
			deviceID, err := uuid.Parse(sub)
			if err != nil {
				return nil, jwt.ErrTokenInvalidClaims
			}
			device, err := deviceStore.GetByID(ctx, deviceID)
			if err != nil {
				return nil, jwt.ErrTokenInvalidClaims
			}
			if !device.IsActive {
				return nil, jwt.ErrTokenInvalidClaims
			}
			return device.PublicKey, nil
		}, jwt.WithIssuer(cfg.JWTIssuer), jwt.WithAudience(cfg.JWTAudience))

		if err != nil || !token.Valid {
			c.AbortWithStatusJSON(http.StatusUnauthorized, map[string]any{
				"code":    "AUTH_INVALID",
				"message": "invalid or expired token",
			})
			return
		}

		claims, ok := token.Claims.(jwt.MapClaims)
		if !ok {
			c.AbortWithStatusJSON(http.StatusUnauthorized, map[string]any{
				"code":    "AUTH_INVALID",
				"message": "invalid token claims",
			})
			return
		}

		sub, _ := claims["sub"].(string)
		deviceID, err := uuid.Parse(sub)
		if err != nil {
			c.AbortWithStatusJSON(http.StatusUnauthorized, map[string]any{
				"code":    "AUTH_INVALID",
				"message": "invalid token subject",
			})
			return
		}

		jti, _ := claims["jti"].(string)
		if jti != "" {
			blacklistKey := "jwt:blacklist:" + jti
			exists, err := rdb.Exists(ctx, blacklistKey).Result()
			if err != nil {
				logger.L().Error("redis blacklist check failed", zap.Error(err), zap.String("device_id", deviceID.String()))
				c.AbortWithStatusJSON(http.StatusServiceUnavailable, map[string]any{
					"code":    "AUTH_CHECK_UNAVAILABLE",
					"message": "token revocation check temporarily unavailable",
				})
				return
			}
			if exists > 0 {
				c.AbortWithStatusJSON(http.StatusUnauthorized, map[string]any{
					"code":    "AUTH_REPLAYED",
					"message": "token has been revoked",
				})
				return
			}
		}

		c.Set(DeviceIDKey, deviceID)
		if jti != "" {
			c.Set("jti", jti)
		}
		if exp, ok := claims["exp"].(float64); ok {
			c.Set("exp", int64(exp))
		}

		// Update last_seen_at asynchronously so auth latency is not impacted.
		// A bounded semaphore prevents unbounded goroutine growth under load.
		select {
		case updateSem <- struct{}{}:
			go func(ts time.Time) {
				defer func() { <-updateSem }()
				bgCtx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
				defer cancel()
				if err := deviceStore.UpdateLastSeen(bgCtx, deviceID, ts); err != nil {
					logger.L().Warn("failed to update last_seen_at", zap.String("device_id", deviceID.String()), zap.Error(err))
				}
			}(time.Now().UTC())
		default:
			logger.L().Warn("last_seen_at update dropped, semaphore full", zap.String("device_id", deviceID.String()))
		}

		c.Next(ctx)
	}
}

func extractBearer(c *app.RequestContext) string {
	h := string(c.GetHeader("Authorization"))
	if !strings.HasPrefix(h, "Bearer ") {
		return ""
	}
	return strings.TrimSpace(h[len("Bearer "):])
}
