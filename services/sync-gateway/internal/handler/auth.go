package handler

import (
	"context"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

// AuthHandler handles authentication-related endpoints.
type AuthHandler struct {
	rdb *redis.Client
}

// NewAuthHandler creates a new AuthHandler.
func NewAuthHandler(rdb *redis.Client) *AuthHandler {
	return &AuthHandler{rdb: rdb}
}

// Revoke handles POST /v1/auth/revoke.
// It blacklists the current JWT's jti in Redis with a TTL matching the token's remaining lifetime.
func (h *AuthHandler) Revoke(ctx context.Context, c *app.RequestContext) {
	log := logger.WithContext(ctx, c)

	jti, ok := c.Get("jti")
	if !ok || jti == "" {
		log.Warn("revoke called without jti")
		c.JSON(http.StatusBadRequest, map[string]any{
			"code":    "MISSING_JTI",
			"message": "token does not contain a jti claim",
		})
		return
	}

	jtiStr, ok := jti.(string)
	if !ok {
		log.Warn("jti in context is not a string")
		c.JSON(http.StatusBadRequest, map[string]any{
			"code":    "INVALID_JTI",
			"message": "invalid jti type in context",
		})
		return
	}
	blacklistKey := "jwt:blacklist:" + jtiStr

	// Default TTL of 24h if we cannot determine remaining lifetime.
	ttl := 24 * time.Hour

	// The middleware stores 'exp' as int64 in the request context after successful
	// validation. Read it here to compute the exact remaining TTL for the blacklist
	// entry so it expires naturally when the token itself expires.
	if expVal, ok := c.Get("exp"); ok {
		if expUnix, ok := expVal.(int64); ok {
			remaining := time.Until(time.Unix(expUnix, 0))
			if remaining > 0 {
				ttl = remaining
			}
		}
	}

	if err := h.rdb.Set(ctx, blacklistKey, "1", ttl).Err(); err != nil {
		log.Error("failed to blacklist jti", zap.String("jti", jtiStr), zap.Error(err))
		c.JSON(http.StatusInternalServerError, map[string]any{
			"code":    "INTERNAL_ERROR",
			"message": "internal server error",
		})
		return
	}

	log.Info("token revoked", zap.String("jti", jtiStr))
	c.SetStatusCode(http.StatusNoContent)
}
