package metrics

import (
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
)

var (
	// SyncBundlesTotal counts total sync bundles uploaded.
	SyncBundlesTotal = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "spine_sync_bundles_total",
		Help: "Total number of sync bundles uploaded",
	}, []string{"content_type"})

	// SyncBundleSizeBytes tracks bundle payload sizes.
	SyncBundleSizeBytes = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "spine_sync_bundle_size_bytes",
		Help:    "Size of sync bundle payloads in bytes",
		Buckets: prometheus.ExponentialBuckets(1024, 2, 15),
	}, []string{"content_type"})

	// ActiveWebsockets tracks current WebSocket connections.
	ActiveWebsockets = promauto.NewGauge(prometheus.GaugeOpts{
		Name: "spine_active_websockets",
		Help: "Number of active WebSocket connections",
	})

	// PairingSessionsTotal counts pairing sessions initiated.
	PairingSessionsTotal = promauto.NewCounter(prometheus.CounterOpts{
		Name: "spine_pairing_sessions_total",
		Help: "Total number of pairing sessions initiated",
	})
)
