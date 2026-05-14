package middleware

import (
	"context"
	"fmt"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/redis/go-redis/v9"
	"go.uber.org/zap"
)

// RateLimiter creates a Hertz middleware that limits requests per key using Redis.
func RateLimiter(rdb *redis.Client, limit int, window time.Duration, keyFn func(*app.RequestContext) string) app.HandlerFunc {
	return func(ctx context.Context, c *app.RequestContext) {
		key := keyFn(c)
		if key == "" {
			c.Next(ctx)
			return
		}

		redisKey := fmt.Sprintf("ratelimit:%s:%s", key, time.Now().Truncate(window).Format(time.RFC3339))
		current, err := rdb.Incr(ctx, redisKey).Result()
		if err != nil {
			logger.L().Warn("rate limiter redis error", zap.Error(err))
			c.Next(ctx)
			return
		}

		if current == 1 {
			_ = rdb.Expire(ctx, redisKey, window).Err()
		}

		if current > int64(limit) {
			c.AbortWithStatusJSON(http.StatusTooManyRequests, map[string]any{
				"code":    "RATE_LIMITED",
				"message": "rate limit exceeded",
			})
			return
		}

		c.Next(ctx)
	}
}
