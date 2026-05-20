## Context

SyncMind Phase 1 (`core/`) is a headless Rust daemon that indexes local files into a semantic vector store and exposes MCP `search_knowledge`. It is single-device by design. Phase 3 (The Spine) extends this to a multi-device architecture where a mobile "capture" device can push encrypted knowledge to a desktop "index" device.

The Spine is intentionally **not** an intelligent node. It is a blind relay: it stores and routes encrypted payloads but cannot decrypt them. This preserves SyncMind's core privacy guarantee (Privacy is Absolute) while adding cross-device reach.

Constraints:
- Self-hosted first (Docker Compose). No managed cloud dependency.
- Zero accounts / zero passwords. Device identity is cryptographic.
- Go-based (Go-Zero scaffold, Hertz HTTP server) per the vision doc.
- Must coexist with Phase 1 Rust Core without coupling.

## Goals / Non-Goals

**Goals:**
- Provide a QR-code pairing flow that establishes a shared symmetric key between two devices without the server ever seeing the key.
- Authenticate devices via Ed25519-signed JWTs validated against registered public keys.
- Accept encrypted sync bundles and media blobs, store them opaquely, and route real-time notifications to the paired device.
- Support paginated pull and acknowledgment for reliable delivery.
- Provide Docker Compose self-hosting with observability (Prometheus + structured logs).

**Non-Goals:**
- No user account system, OAuth, or password management.
- No content processing on Spine (OCR, STT, embedding, thumbnail generation).
- No conflict resolution or multi-version merging (handled by desktop Rust Core after decryption).
- No P2P / NAT traversal (all traffic goes through Spine relay).
- No multi-device groups (1:1 pairing only in Phase 3).

## Decisions

### 1. Go-Zero + Hertz instead of pure Gin/Echo
**Rationale:** Go-Zero provides service scaffolding (config, logging, metrics, middleware chain) out of the box, which reduces boilerplate for a self-hosted microservice. Hertz (CloudWeGo) offers higher-performance HTTP/WebSocket handling than Go-Zero's built-in `rest` server, which matters for maintaining 10k+ concurrent WebSocket connections.
**Alternative considered:** Standard library `net/http` + `gorilla/websocket`. Rejected because we'd re-invent config loading, metrics, and middleware that Go-Zero already provides.

### 2. PostgreSQL BYTEA for encrypted payload storage
**Rationale:** Phase 3 bundles are expected to be small (text/markdown chunks < 1MB, occasional images < 50MB). Storing in PostgreSQL keeps the self-hosted stack simple (one stateful service). Object storage (MinIO/S3) adds operational complexity.
**Alternative considered:** Filesystem storage. Rejected because it complicates backups, replication, and multi-instance horizontal scaling.
**Future escape hatch:** If bundle sizes grow beyond ~100MB regularly, migrate to object storage without changing the API schema (store object key instead of BYTEA).

### 3. Redis Pub/Sub for WebSocket broadcast across Spine instances
**Rationale:** When scaling Spine horizontally, a sync bundle uploaded to instance A must notify a WebSocket connected to instance B. Redis Pub/Sub is the simplest broadcast backbone; each instance subscribes to `sync:notify:{device_id}` and pushes to its local WebSocket connections.
**Alternative considered:** NATS / RabbitMQ. Rejected because Redis is already required for session/cache use cases, and Pub/Sub is sufficient for fan-out (no persistence needed — Spine only signals "pull now").

### 4. X25519 ECDH for key agreement, Ed25519 for identity/auth
**Rationale:** X25519 is the standard for ECDH key exchange (used by Signal, WireGuard). Ed25519 is the standard for digital signatures. Using both keeps pairing crypto (ephemeral, per-session) separate from identity crypto (persistent, per-device), limiting blast radius if one key is compromised.
**Alternative considered:** Single RSA keypair for both encryption and signing. Rejected because RSA key generation is slower and larger, and mixing encryption/signing concerns violates crypto hygiene.

### 5. Fixed `sync_key` per pair (no Double Ratchet in Phase 3)
**Rationale:** A Double Ratchet (like Signal Protocol) provides forward secrecy but adds significant protocol complexity (ratchet steps, out-of-order message handling, initial X3DH). For Phase 3's 1:1 sync use case with short-lived bundles, the complexity outweighs the benefit.
**Mitigation:** Devices can rotate the `sync_key` by re-pairing. Future PRDs may introduce per-bundle key rotation.

## Risks / Trade-offs

- **[Risk]** PostgreSQL BYTEA table grows unbounded if desktop is offline for weeks.  
  **→ Mitigation:** `bundle_retention_days` default 30 days; physical cleanup cron. Future: per-user storage quota.

- **[Risk]** WebSocket connection leaks if clients disconnect uncleanly (mobile background kill).  
  **→ Mitigation:** Server-side heartbeat (ping every 30s, close if no pong in 10s). Connection mapping uses sync.Map with periodic stale sweep.

- **[Risk]** Self-hosted users may not have TLS certificates, leading to plaintext transport.  
  **→ Mitigation:** Provide ACME/Let's Encrypt auto-TLS option. Document that self-signed certs are acceptable for LAN-only use but strongly discouraged for WAN.

- **[Risk]** If a device private key is leaked, an attacker can impersonate the device and read future sync bundles.  
  **→ Mitigation:** Pair re-initialization generates a new shared key. Add `is_active = FALSE` soft-delete on devices to immediately invalidate JWTs.

- **[Risk]** Mobile offline queue could grow large if Spine is unreachable.  
  **→ Mitigation:** Out of scope for Spine (NG-11). Mobile client must implement local queue with size limits.

## Migration Plan

1. **Deploy:** `cd services/sync-gateway && make dev` brings up Postgres + Redis + Spine.
2. **Database:** `make migrate-up` creates schema V1.
3. **Pairing:** Desktop generates QR → Mobile scans → shared key established.
4. **Desktop Core integration (future PRD):** Rust Core gains a Spine sync client module that polls/downloads bundles and injects them into the local RAG pipeline.
5. **Rollback:** Stop containers. No data loss on desktop (all data is local). Spine data is transient by design.

## Open Questions

1. Should we enforce a maximum total storage per device pair to prevent PostgreSQL disk exhaustion?
2. Should the pairing ceremony include a visual confirmation (e.g., both devices show a matching 4-digit number) to prevent MITM during the brief pairing window?
3. How should the desktop Rust Core handle bundle decryption failures (wrong key, corrupted payload)? Should Spine be notified, or should the client just retry?
