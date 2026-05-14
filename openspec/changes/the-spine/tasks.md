## 1. Service Scaffolding & Configuration

- [x] 1.1 Initialize Go module at `services/sync-gateway/` (`go mod init github.com/blkcor/syncmind/spine`)
- [x] 1.2 Install core dependencies: `go-zero`, `hertz`, `pgx/v5`, `go-redis/v9`, `goose`, `jwt/v5`, `zap`
- [x] 1.3 Create directory structure: `cmd/`, `internal/config/`, `internal/handler/`, `internal/logic/`, `internal/model/`, `internal/middleware/`, `internal/pkg/crypto/`, `internal/pkg/websocket/`, `internal/scheduler/`, `migrations/`
- [x] 1.4 Implement config loading from `spine.yaml` and environment variables (Go-Zero `conf.Load`)
- [x] 1.5 Define config schema with all required fields: `database_url`, `redis_addr`, `bind_addr`, `tls_cert`, `tls_key`, `pairing_session_ttl`, `bundle_retention_days`, `max_bundle_size_mb`, `jwt_issuer`, `jwt_audience`
- [x] 1.6 Implement health check endpoint `GET /health` with PostgreSQL and Redis connectivity checks
- [x] 1.7 Verify `go build` and `go vet` pass

## 2. Database Schema & Migrations

- [x] 2.1 Install `pressly/goose` as a tool dependency
- [x] 2.2 Create migration `00001_create_devices_table.sql`
- [x] 2.3 Create migration `00002_create_pairing_sessions_table.sql`
- [x] 2.4 Create migration `00003_create_sync_bundles_table.sql`
- [x] 2.5 Create migration `00004_create_indexes.sql` (`idx_sync_bundles_to_device_acked`, `idx_pairing_sessions_expires`)
- [x] 2.6 Implement `make migrate-up` and `make migrate-down` Makefile targets
- [x] 2.7 Write integration test that runs migrations against a test PostgreSQL container and verifies schema

## 3. Device Pairing

- [x] 3.1 Implement `POST /v1/pairing/initiate` handler: generate UUID session, store initiator pubkey, return `session_id` + `qr_payload` + 6-digit short code
- [x] 3.2 Implement `POST /v1/pairing/complete` handler: validate session, store responder pubkey, update status to `completed`, create `devices` records with mutual `paired_device_id`
- [x] 3.3 Implement `GET /v1/pairing/:session_id/status` handler: return current status and paired device ID
- [x] 3.4 Implement pairing session expiration validation middleware
- [x] 3.5 Implement cron job / scheduler to mark expired pending sessions and delete old expired records
- [x] 3.6 Write unit tests for pairing handlers with mocked DB
- [x] 3.7 Write integration test for full pairing flow (initiate → complete → status)

## 4. Device Authentication

- [x] 4.1 Implement Ed25519 key pair generation helper in `internal/pkg/crypto/`
- [x] 4.2 Implement JWT signing middleware for outgoing device requests (client-side helper)
- [x] 4.3 Implement JWT validation middleware: extract `sub`, load public key from DB, verify Ed25519 signature, check `exp` and `jti`
- [x] 4.4 Implement Redis-based `jti` blacklist with TTL matching JWT expiration
- [x] 4.5 Implement `is_active` enforcement: reject requests from deactivated devices
- [x] 4.6 Integrate auth middleware into all protected routes (`/v1/sync/*`, `/v1/media/*`, `/v1/pairing/complete`)
- [x] 4.7 Write unit tests for JWT validation with valid, expired, replayed, and invalid-signature tokens
- [x] 4.8 Write integration test for device deactivation flow

## 5. Sync Bundle Relay

- [x] 5.1 Implement `POST /v1/sync/bundle` handler: validate size, verify pairing, compute SHA-256, store in `sync_bundles`, return `bundle_id`
- [x] 5.2 Implement idempotency key handling: check Redis cache for `Idempotency-Key` within 24h window
- [x] 5.3 Implement Redis Pub/Sub publisher on successful bundle upload
- [x] 5.4 Implement `GET /v1/sync/bundles` handler: paginated metadata listing (no payload) for authenticated device
- [x] 5.5 Implement `GET /v1/sync/bundles/:id` handler: return encrypted payload as `application/octet-stream` with integrity hash header
- [x] 5.6 Implement `DELETE /v1/sync/bundles/:id` handler: set `acked_at = NOW()`
- [x] 5.7 Implement WebSocket Hub: connection registry (`sync.Map`), device-to-connections mapping, heartbeat ping/pong
- [x] 5.8 Implement WebSocket upgrade handler `WS /v1/sync/live` with JWT auth via query param or subprotocol
- [x] 5.9 Implement Redis Subscriber goroutine: subscribe to `sync:notify:*`, fan out to local WebSocket connections
- [x] 5.10 Implement rate limiting: 100 bundles/minute per device, 20 pairing requests/minute globally
- [x] 5.11 Implement scheduled cleanup job: hard-delete bundles past retention or already acknowledged > 7 days
- [x] 5.12 Write integration tests for full sync flow: upload → Redis notify → WS delivery → download → ack

## 6. Mobile Media Ingestion

- [x] 6.1 Implement `POST /v1/media/upload` handler: parse `multipart/form-data`, validate content type whitelist, validate size, store as `sync_bundles`
- [x] 6.2 Implement content type validation: allow `image/jpeg`, `image/png`, `image/heic`, `audio/m4a`, `audio/wav`
- [x] 6.3 Integrate media upload into Redis notification pipeline (treated as standard bundle)
- [x] 6.4 Write integration test for media upload → listing → download flow
- [x] 6.5 Verify no image/audio parsing libraries are imported (security audit check)

## 7. Deployment & Observability

- [x] 7.1 Write `Dockerfile` with multi-stage build (`golang:1.24-alpine` → `alpine:latest`)
- [x] 7.2 Write `docker-compose.yml` with `spine`, `postgres`, `redis` services and health checks
- [x] 7.3 Write `Makefile` with targets: `build`, `test`, `lint`, `migrate-up`, `migrate-down`, `dev`, `dev-down`
- [x] 7.4 Integrate Go-Zero Prometheus metrics handler on `/metrics`
- [x] 7.5 Register custom metrics: `spine_sync_bundles_total`, `spine_sync_bundle_size_bytes`, `spine_active_websockets`, `spine_pairing_sessions_total`
- [x] 7.6 Configure structured logging with `zap` / `logx`: include `trace_id`, `device_id`, redact sensitive fields
- [x] 7.7 Write `docs/examples/spine-docker-compose.md` quick-start guide
- [x] 7.8 Verify `make dev` starts all services and `/health` returns 200

## 8. Security & Compliance

- [x] 8.1 Run `grep -r "decrypt\|Decrypt\|unencrypt\|Unencrypt" services/sync-gateway/` and verify zero matches in source code
- [x] 8.2 Run `grep -r "encrypted_payload" services/sync-gateway/` and confirm it only appears in DB write/read paths, never in logs
- [x] 8.3 Verify TLS 1.3 is enforced in production configuration
- [x] 8.4 Add security headers middleware (HSTS, X-Content-Type-Options, X-Frame-Options)
- [x] 8.5 Run `gosec` static analysis and resolve all high/severity findings
- [x] 8.6 Document E2EE audit checklist in `docs/security/spine-audit.md`

## 9. Final Validation

- [x] 9.1 Run `make test` — all unit and integration tests pass
- [x] 9.2 Run `make lint` — `go vet` and `gofmt` clean
- [x] 9.3 Run `go test -race ./...` — no data races detected
- [x] 9.4 Run load test: simulate 10,000 concurrent WebSocket connections, verify memory < 1GB (~542 MB Sys, 1.59s)
- [ ] 9.5 End-to-end manual test: desktop initiates pairing → mobile scans → mobile uploads bundle → desktop receives WS notification → desktop downloads and acks (blocked: requires desktop/mobile client apps)
- [x] 9.6 Update `002-the-spine.md` PRD if any implementation diverged from spec
