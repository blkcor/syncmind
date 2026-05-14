package logger

import (
	"context"
	"fmt"
	"os"
	"sync"

	"github.com/cloudwego/hertz/pkg/app"
	"go.uber.org/zap"
	"go.uber.org/zap/zapcore"
)

var (
	globalLogger *zap.Logger
	once         sync.Once
)

// Init initializes the global structured logger.
func Init() *zap.Logger {
	once.Do(func() {
		cfg := zap.NewProductionConfig()
		cfg.EncoderConfig.TimeKey = "timestamp"
		cfg.EncoderConfig.EncodeTime = zapcore.ISO8601TimeEncoder
		cfg.EncoderConfig.MessageKey = "message"
		cfg.EncoderConfig.CallerKey = "caller"
		cfg.EncoderConfig.StacktraceKey = "stacktrace"

		l, err := cfg.Build(zap.AddCallerSkip(1))
		if err != nil {
			// Fallback to a no-op logger if production config fails.
			_, _ = fmt.Fprintln(os.Stderr, "zap logger init failed, using no-op fallback")
			globalLogger = zap.NewNop()
			return
		}
		globalLogger = l
	})
	return globalLogger
}

// L returns the global logger instance.
func L() *zap.Logger {
	if globalLogger == nil {
		return Init()
	}
	return globalLogger
}

// WithContext returns a logger enriched with fields extracted from the Hertz request context.
func WithContext(ctx context.Context, c *app.RequestContext) *zap.Logger {
	log := L()

	if traceID := c.GetString("trace_id"); traceID != "" {
		log = log.With(zap.String("trace_id", traceID))
	}
	if deviceID := c.GetString("device_id"); deviceID != "" {
		log = log.With(zap.String("device_id", deviceID))
	}

	return log
}

// Sync flushes any buffered log entries.
func Sync() error {
	if globalLogger != nil {
		return globalLogger.Sync()
	}
	return nil
}
