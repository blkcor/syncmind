//go:build load

package load

import (
	"context"
	"net"
	"runtime"
	"sync"
	"testing"
	"time"

	spineWS "github.com/blkcor/syncmind/spine/internal/pkg/websocket"
	"github.com/cloudwego/hertz/pkg/app"
	"github.com/cloudwego/hertz/pkg/app/server"
	"github.com/google/uuid"
	"github.com/gorilla/websocket"
)

func TestWebSocketConnections10K(t *testing.T) {
	// Create a listener on an available port.
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("failed to create listener: %v", err)
	}
	defer ln.Close()

	addr := ln.Addr().String()
	hub := spineWS.NewHub()
	deviceID := uuid.MustParse("11111111-1111-1111-1111-111111111111")

	h := server.New(server.WithListener(ln))
	h.GET("/v1/sync/live", func(ctx context.Context, c *app.RequestContext) {
		hub.HandleUpgrade(deviceID)(ctx, c)
	})

	go func() {
		if err := h.Run(); err != nil {
			t.Logf("server run error: %v", err)
		}
	}()
	defer h.Close()

	// Give the server a moment to start serving.
	time.Sleep(100 * time.Millisecond)

	dialer := websocket.Dialer{
		HandshakeTimeout: 5 * time.Second,
	}

	conns := make([]*websocket.Conn, 0, 10000)
	var mu sync.Mutex
	var wg sync.WaitGroup
	sem := make(chan struct{}, 200)

	for i := 0; i < 10000; i++ {
		wg.Add(1)
		sem <- struct{}{}
		go func(idx int) {
			defer wg.Done()
			defer func() { <-sem }()

			url := "ws://" + addr + "/v1/sync/live"
			conn, _, err := dialer.Dial(url, nil)
			if err != nil {
				t.Logf("dial %d failed: %v", idx, err)
				return
			}
			mu.Lock()
			conns = append(conns, conn)
			mu.Unlock()
		}(i)
	}
	wg.Wait()

	// Ensure server processed all registrations.
	time.Sleep(500 * time.Millisecond)

	connCount := hub.ConnectionCount()
	if connCount != len(conns) {
		t.Logf("warning: hub connection count %d != established conns %d", connCount, len(conns))
	}

	// Force GC for a stable memory reading.
	runtime.GC()
	time.Sleep(500 * time.Millisecond)

	var m runtime.MemStats
	runtime.ReadMemStats(&m)

	t.Logf("Established connections: %d", len(conns))
	t.Logf("HeapAlloc: %.2f MB", float64(m.HeapAlloc)/1024/1024)
	t.Logf("HeapSys:   %.2f MB", float64(m.HeapSys)/1024/1024)
	t.Logf("Sys:       %.2f MB", float64(m.Sys)/1024/1024)

	const oneGB = 1024 * 1024 * 1024
	if m.Sys > oneGB {
		t.Fatalf("memory usage exceeded 1GB: Sys=%.2f MB", float64(m.Sys)/1024/1024)
	}

	// Clean up connections.
	for _, c := range conns {
		c.Close()
	}
}
