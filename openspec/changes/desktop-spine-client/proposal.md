## Why

Phase 3 shipped a fully implemented, blind Spine sync gateway (`services/sync-gateway/`), but the protocol has no client. Without a client, end-to-end E2EE behavior is unverified and Phase 4 (mobile capture) cannot land. PRD 004 (`docs/prd/004-desktop-spine-client.md`) defined US-020..US-030 for the desktop counterpart; this change implements that PRD so two desktops can cross-sync notes today and the protocol gets its first production validation.

## What Changes

- New Tauri Rust modules under `apps/desktop/src-tauri/src/spine/`: `identity`, `crypto`, `pairing`, `client`, `ws`, `bundle`, `inbox`, `commands`.
- New SolidJS Devices tab (`apps/desktop/src/components/DevicesTab.tsx`) plus tray menu entry and Tauri command registrations in `apps/desktop/src-tauri/src/lib.rs`.
- Extend `core/syncmind-core::Config` with a `[spine]` section (`url`, `paired_peer_fingerprint`, `paired_peer_device_type`, `paired_at`, `peer_device_id_uuid`, `trust_ca_path`).
- Add a new single-file indexing entry point in `core/syncmind-indexing` (`index_file_once`) so decrypted notes land synchronously without depending on file-watcher directory recursion.
- **BREAKING (server, additive on the wire):** `services/sync-gateway` accepts a required `device_uuid` field in `POST /v1/pairing/initiate` and `POST /v1/pairing/complete` and uses it as `devices.id` instead of `gen_random_uuid()`. Existing archived spec wording is superseded.
- Amend `docs/prd/003-the-spine.md` §Impl Note 1 to fix the Ed25519↔X25519 conversion as the protocol-level contract (dalek `to_scalar_bytes()` + `CompressedEdwardsY::to_montgomery()`).

## Capabilities

### New Capabilities
- `desktop-spine-client`: umbrella capability for the desktop side of the Spine protocol — Ed25519 identity in the OS keychain, pairing UI (display QR + short code + polling), `sync_key` derivation, AES-256-GCM bundle envelope, HTTPS client with idempotent upload and integrity-checked download, WebSocket notifications with exponential backoff and 30s polling fallback, sync-inbox materialization, and unpair/reset flows.

### Modified Capabilities
- `desktop-shell`: register the Devices tab, the "Sync devices…" tray item, and all `spine_*` Tauri commands; persist `[spine]` via existing `Config::load`/`Config::save`.
- `device-pairing`: pairing initiate/complete accepts a client-supplied `device_uuid` (UUIDv4) and persists it as `devices.id`; conflict returns `409 UUID_CONFLICT`.
- `device-auth`: JWT `sub` claim is the client-supplied UUID; server verifies the signature against the public key of the device whose `id` matches `sub`.
- `settings-indexing`: `[spine]` config section is recognized with default empty values; `trust_ca_path` (PEM file) is honored by the HTTP client builder via `reqwest::ClientBuilder::add_root_certificate`.

## Impact

- **Affected code**: `apps/desktop/src-tauri/*`, `apps/desktop/src/*`, `core/syncmind-core/src/config.rs`, `core/syncmind-indexing/*`, `services/sync-gateway/internal/handler/pairing.go`, `services/sync-gateway/internal/model/pairing.go`, `docs/prd/003-the-spine.md`.
- **New dependencies (Rust, `apps/desktop/src-tauri/Cargo.toml`)**: `keyring`, `ed25519-dalek`, `x25519-dalek`, `curve25519-dalek`, `aes-gcm`, `hkdf`, `sha2`, `qrcode`, `image`, `reqwest` (rustls), `tokio-tungstenite` (rustls + webpki-roots), `jsonwebtoken`, `uuid`, `base64`, `chrono`, `tracing`. License audit: all MIT/Apache-2.0.
- **Build & test gates**:
  - `cd apps/desktop/src-tauri && cargo check && cargo clippy --all-targets -- -D warnings && cargo test`
  - `cd services/sync-gateway && make test && go vet ./...`
  - `cd apps/desktop && pnpm typecheck && pnpm lint`
  - End-to-end: `docker compose -f services/sync-gateway/docker-compose.yml up` + two desktop instances against the local Spine; verify pair → send → receive → unpair; `pg_dump` grep audit confirms zero plaintext leakage.
- **Security audit (pre-archive)**: confirm no code path logs `sync_key`, `shared_secret`, raw Ed25519 private key, or `Authorization` header; confirm all `keyring` writes target `service = "syncmind"` with the documented account whitelist; confirm Tauri command registrations expose only public-safe types (fingerprints, UUIDs, status enums).
- **Non-impacts**: `core/storage` public API is unchanged; no new daemon process; no new transitive crates pulled into `core/syncmind-core`.
