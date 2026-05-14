package websocket

import (
	"context"
	"sync"
	"time"

	"github.com/blkcor/syncmind/spine/internal/logger"
	"github.com/blkcor/syncmind/spine/internal/metrics"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/cloudwego/hertz/pkg/common/utils"
	"github.com/cloudwego/hertz/pkg/protocol/consts"
	"github.com/google/uuid"
	"github.com/hertz-contrib/websocket"
	"go.uber.org/zap"
)

var upgrader = websocket.HertzUpgrader{
	CheckOrigin: func(_ *app.RequestContext) bool {
		return true
	},
}

// Hub maintains the set of active WebSocket connections per device.
type Hub struct {
	mu          sync.RWMutex
	connections map[uuid.UUID]map[*websocket.Conn]bool
}

// NewHub creates a new Hub.
func NewHub() *Hub {
	return &Hub{
		connections: make(map[uuid.UUID]map[*websocket.Conn]bool),
	}
}

// Register adds a connection for a device.
func (h *Hub) Register(deviceID uuid.UUID, conn *websocket.Conn) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.connections[deviceID] == nil {
		h.connections[deviceID] = make(map[*websocket.Conn]bool)
	}
	h.connections[deviceID][conn] = true
	metrics.ActiveWebsockets.Inc()
}

// Unregister removes a connection for a device.
func (h *Hub) Unregister(deviceID uuid.UUID, conn *websocket.Conn) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if conns, ok := h.connections[deviceID]; ok {
		delete(conns, conn)
		if len(conns) == 0 {
			delete(h.connections, deviceID)
		}
	}
	if err := conn.Close(); err != nil {
		logger.L().Warn("websocket close failed", zap.Error(err))
	}
	metrics.ActiveWebsockets.Dec()
}

// Send delivers a message to all connections for a device.
func (h *Hub) Send(deviceID uuid.UUID, message []byte) {
	h.mu.RLock()
	conns := h.connections[deviceID]
	h.mu.RUnlock()
	for conn := range conns {
		if err := conn.WriteMessage(websocket.TextMessage, message); err != nil {
			logger.L().Warn("websocket write failed", zap.String("device_id", deviceID.String()), zap.Error(err))
		}
	}
}

// ConnectionCount returns the total number of active connections.
func (h *Hub) ConnectionCount() int {
	h.mu.RLock()
	defer h.mu.RUnlock()
	count := 0
	for _, conns := range h.connections {
		count += len(conns)
	}
	return count
}

// HandleUpgrade handles WebSocket upgrade requests.
func (h *Hub) HandleUpgrade(deviceID uuid.UUID) app.HandlerFunc {
	return func(ctx context.Context, c *app.RequestContext) {
		err := upgrader.Upgrade(c, func(conn *websocket.Conn) {
			h.Register(deviceID, conn)
			defer h.Unregister(deviceID, conn)

			// Start heartbeat ticker
			ticker := time.NewTicker(30 * time.Second)
			defer ticker.Stop()

			deadline := make(chan struct{})
			go func() {
				for range ticker.C {
					if err := conn.WriteMessage(websocket.TextMessage, []byte(`{"type":"ping"}`)); err != nil {
						close(deadline)
						return
					}
				}
			}()

			for {
				select {
				case <-deadline:
					return
				default:
				}

				if err := conn.SetReadDeadline(time.Now().Add(40 * time.Second)); err != nil {
					return
				}
				msgType, msg, err := conn.ReadMessage()
				if err != nil {
					return
				}
				if msgType == websocket.TextMessage {
					// Handle pong
					if string(msg) == `{"type":"pong"}` {
						continue
					}
				}
			}
		})
		if err != nil {
			logger.L().Warn("websocket upgrade failed", zap.String("device_id", deviceID.String()), zap.Error(err))
			c.JSON(consts.StatusBadRequest, utils.H{"error": "websocket upgrade failed"})
		}
	}
}
