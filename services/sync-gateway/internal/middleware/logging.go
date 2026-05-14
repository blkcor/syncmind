package middleware

import (
	"context"
	"net/http"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/google/uuid"
	"go.uber.org/zap"
)

const TraceIDKey = "trace_id"

// RequestLogger returns a Hertz middleware that logs every HTTP request with structured fields.
func RequestLogger() app.HandlerFunc {
	return func(ctx context.Context, c *app.RequestContext) {
		start := time.Now()
		traceID := uuid.New().String()
		c.Set(TraceIDKey, traceID)

		c.Next(ctx)

		duration := time.Since(start)
		statusCode := c.Response.StatusCode()
		method := string(c.Request.Method())
		path := string(c.Request.Path())

		fields := []zap.Field{
			zap.String("trace_id", traceID),
			zap.String("method", method),
			zap.String("path", path),
			zap.Int("status_code", statusCode),
			zap.Duration("duration", duration),
		}

		if deviceIDVal, ok := c.Get(DeviceIDKey); ok {
			if deviceID, ok := deviceIDVal.(uuid.UUID); ok {
				fields = append(fields, zap.String("device_id", deviceID.String()))
				c.Set("device_id", deviceID.String())
			}
		}

		// Redact Authorization header if present.
		authHeader := string(c.GetHeader("Authorization"))
		if authHeader != "" {
			fields = append(fields, zap.String("authorization", "[REDACTED]"))
		}

		log := logger.L()
		if statusCode >= http.StatusInternalServerError {
			log.Error("request completed", fields...)
		} else if statusCode >= http.StatusBadRequest {
			log.Warn("request completed", fields...)
		} else {
			log.Info("request completed", fields...)
		}
	}
}
