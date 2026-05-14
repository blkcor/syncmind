## Why

SyncMind Phase 1 (Headless MCP Core) delivers a fully local, privacy-first context engine — but it is trapped on a single device. Users generate high-value knowledge on both desktop (IDE, notes) and mobile (voice memos, photos of whiteboards). Without a secure sync mechanism, these assets remain isolated, defeating the core promise of an "omnipresent brain."

The Spine is the cross-device sync gateway that breaks this isolation while maintaining SyncMind's absolute privacy guarantee: **the server is a blind relay that never holds decryption keys.** By pairing devices via QR codes and using X25519 ECDH + AES-256-GCM end-to-end encryption, we enable seamless multi-device sync without accounts, passwords, or cloud trust.

## What Changes

- Introduce a new Go microservice (`services/sync-gateway/`) using Go-Zero + Hertz + PostgreSQL + Redis.
- Implement QR-code device pairing with X25519 ECDH key exchange (no user accounts).
- Implement Ed25519-signed JWT device authentication.
- Implement blind E2EE sync relay: encrypted bundle upload, storage, WebSocket notification, download, and acknowledgment.
- Implement encrypted mobile media ingestion (images/voice) as opaque blobs.
- Provide self-hosted Docker Compose deployment with Prometheus metrics and structured logging.
- Add database migration system (goose) for schema management.
- **BREAKING:** New infrastructure dependency (PostgreSQL + Redis) for users who choose to self-host The Spine.

## Capabilities

### New Capabilities
- `device-pairing`: QR-code and short-code device pairing with X25519 ECDH key exchange. No accounts or passwords.
- `device-auth`: Ed25519-signed JWT authentication for all device requests.
- `sync-bundle-relay`: Encrypted sync bundle upload, blind storage, WebSocket + Redis real-time notification, paginated download, and client acknowledgment with retention cleanup.
- `mobile-media-ingestion`: Encrypted image and audio blob upload from mobile, treated as opaque data routed to the paired desktop.
- `spine-deployment`: Self-hosted Docker Compose stack with health checks, Prometheus metrics, and structured logging.

### Modified Capabilities
<!-- No existing specs modified — this is a net-new subsystem. -->

## Impact

- **New service:** `services/sync-gateway/` (Go module, ~8 internal packages).
- **New infrastructure:** PostgreSQL 17, Redis 7 (managed via Docker Compose).
- **New API surface:** REST + WebSocket endpoints under `/v1/` (pairing, sync, media).
- **New build commands:** `make build`, `make migrate-up`, `make dev`, `make test` in `services/sync-gateway/`.
- **No impact on Phase 1 Rust Core:** The Spine is opt-in; desktop core gains sync capabilities via new API client code (future PRD).
- **Security audit requirement:** Code must be audited to confirm Spine has zero access to plaintext or decryption keys (FR-20).
