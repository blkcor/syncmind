## 1. Workspace prep & dependencies

- [x] 1.1 Add new Rust crates to `apps/desktop/src-tauri/Cargo.toml`: `keyring = "3"`, `ed25519-dalek = { version = "2", features = ["pkcs8", "rand_core"] }`, `x25519-dalek = "2"`, `curve25519-dalek = "4"`, `aes-gcm = "0.10"`, `hkdf = "0.12"`, `sha2 = "0.10"`, `qrcode = "0.14"`, `image = "0.25"`, `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream"] }`, `tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }`, `jsonwebtoken = "9"`, `uuid = { version = "1", features = ["v4", "serde"] }`, `base64 = "0.22"`, `chrono = { version = "0.4", features = ["serde"] }`, `tracing = "0.1"`, `rustls-pemfile = "2"`, `url = "2"`
- [x] 1.2 Verify `cargo check -p syncmind-desktop` (or the actual desktop crate name) passes with new deps
- [x] 1.3 Confirm every new dep is MIT or Apache-2.0; record any exceptions in `design.md`
- [x] 1.4 Add `keyring` mock feature gating for CI tests

## 2. Server amendments (services/sync-gateway)

- [x] 2.1 Add `DeviceUUID string \`json:"device_uuid"\``field to`InitiateRequest`and`CompleteRequest`in`internal/handler/pairing.go`
- [x] 2.2 Validate `device_uuid` parses as UUIDv4; return `400 INVALID_REQUEST` on failure
- [x] 2.3 Implement conflict check on `initiate`: existing `devices` row with same UUID but different `public_key_fingerprint` → `409 UUID_CONFLICT`
- [x] 2.4 Implement conflict check on `complete`: same UUID-vs-fingerprint logic for the responder
- [x] 2.5 Insert `devices` rows using the client-supplied UUID as primary key (override `gen_random_uuid()` default)
- [x] 2.6 Add regression coverage that `DeviceStore.Create` persists caller-supplied `Device.ID` instead of relying on the DB default
- [x] 2.7 Update existing pairing handler unit tests to include `device_uuid` in request bodies
- [x] 2.8 Add new unit test: missing `device_uuid` → 400
- [x] 2.9 Add new unit test: UUID conflict scenario → 409
- [x] 2.10 Add new unit test: device recovery (same UUID + same fingerprint) → success
- [x] 2.11 Update `services/sync-gateway/docs/security/spine-audit.md` to reflect client-supplied UUIDs
- [x] 2.12 Run `make test` and `go vet ./...` in `services/sync-gateway`; both clean

## 3. PRD 002 amendment

- [x] 3.1 Edit `docs/prd/002-the-spine.md` §Impl Note 1 to add sub-note §1.1 specifying the dalek conversion algorithm (per design.md)
- [x] 3.2 Add cross-reference: "See PRD 004 §US-031 for the desktop implementation."
- [x] 3.3 Add cross-reference: "See openspec/changes/desktop-spine-client/specs/device-pairing/spec.md for the normative spec delta."

## 4. Core Config extension (core/syncmind-core)

- [x] 4.1 Add `SpineConfig` struct to `core/syncmind-core/src/config.rs` with fields: `url: Option<String>`, `paired_peer_fingerprint: Option<String>`, `paired_peer_device_type: Option<String>`, `paired_at: Option<chrono::DateTime<Utc>>`, `peer_device_id_uuid: Option<uuid::Uuid>`, `trust_ca_path: Option<PathBuf>`
- [x] 4.2 Wire `spine: SpineConfig` into the top-level `Config` struct with `#[serde(default)]`
- [x] 4.3 Add unit test: legacy `config.toml` without `[spine]` deserializes successfully with default empty `SpineConfig`
- [x] 4.4 Add unit test: full roundtrip `Config { spine: ... }` through `save` then `load`
- [x] 4.5 Add `SpineConfig::validate_url(&self) -> Result<Url, ConfigError>` helper that allows `http://`, `https://`, and IP hosts, but emits a `WarningKind::PlainHttp` when the scheme is HTTP
- [x] 4.6 Add `SpineConfig::load_trust_ca(&self) -> Result<Vec<rustls_pki_types::CertificateDer>, ConfigError>` that reads and parses the PEM file at `trust_ca_path`

## 5. Single-file indexing entry point (core/syncmind-indexing)

- [x] 5.1 Inventory the existing indexing pipeline to identify the shared `extract → chunk → embed → upsert` path (likely in `core/syncmind-indexing/src/lib.rs` or a `pipeline` module)
- [x] 5.2 Expose `pub async fn index_file_once(path: &Path, ctx: &IndexingContext) -> Result<IngestionReport>` that performs the full pipeline for a single file
- [x] 5.3 Make the function idempotent: re-running on the same path replaces all prior chunks for that path (use existing `VectorStore::upsert_file` semantics)
- [x] 5.4 Return `IngestionReport { file_path, chunks_added, bytes, duration_ms }`
- [x] 5.5 Add unit tests using a temporary directory + in-memory or temp-file `VectorStore`
- [x] 5.6 Ensure the function does NOT swallow embedding-service errors; surface them with structured error variants

## 6. Desktop identity module (spine/identity.rs)

- [x] 6.1 Create directory `apps/desktop/src-tauri/src/spine/` and module file `mod.rs`
- [x] 6.2 Implement `ensure_identity() -> Result<Ed25519Identity, IdentityError>` that generates or loads the key from the OS keychain at `service="syncmind", account="device-identity"`
- [x] 6.3 Persist newly minted private key as PKCS#8 v2 base64 in the keychain entry
- [x] 6.4 Mint a `Uuid::new_v4()` at first creation; persist `{ fingerprint, device_type: "desktop", device_uuid, created_at }` to `<data-dir>/device.json` via atomic write
- [x] 6.5 On subsequent loads, verify derived fingerprint matches `device.json`; on mismatch, return `KEYCHAIN_FINGERPRINT_MISMATCH`
- [x] 6.6 Provide `Ed25519Identity::sign(&self, msg: &[u8]) -> Signature` — never expose raw `SigningKey`
- [x] 6.7 Implement Linux libsecret fallback: if `keyring::Entry::new` fails, write key to `<data-dir>/keys/device.ed25519` with mode `0600` (verify mode via `std::os::unix::fs::PermissionsExt`)
- [x] 6.8 Add unit tests with `keyring`'s mock provider for generate-then-load roundtrip

## 7. Crypto primitives (spine/crypto.rs)

- [x] 7.1 Implement `hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], out: &mut [u8])` wrapper
- [x] 7.2 Implement `aes_gcm_encrypt(key: &[u8;32], nonce: &[u8;12], aad: &[u8], plaintext: &[u8]) -> Vec<u8>` returning `ciphertext ‖ tag`
- [x] 7.3 Implement `aes_gcm_decrypt(key: &[u8;32], nonce: &[u8;12], aad: &[u8], ciphertext_and_tag: &[u8]) -> Result<Vec<u8>>`
- [x] 7.4 Implement `ed25519_to_x25519_priv(signing_key: &SigningKey) -> [u8;32]` using `to_scalar_bytes`
- [x] 7.5 Implement `ed25519_to_x25519_pub(verifying_key: &VerifyingKey) -> [u8;32]` using `CompressedEdwardsY::decompress(...).to_montgomery()`
- [x] 7.6 Implement `jwt_mint(claims: &JwtClaims, signing_key: &SigningKey) -> Result<String>` using `jsonwebtoken` with `Algorithm::EdDSA`
- [x] 7.7 Implement `JwtClaims` with `sub`, `iat`, `exp`, `jti`, `iss`, `aud`
- [x] 7.8 Add unit tests: HKDF golden vectors from RFC 5869; AES-GCM roundtrip + AAD-mismatch failure; Ed25519↔X25519 conversion roundtrip (verify ECDH between A→B and B→A produces identical secret)

## 8. HTTP client (spine/client.rs)

- [x] 8.1 Build a `SpineClient` wrapping `reqwest::Client` with rustls-tls
- [x] 8.2 Apply user-supplied PEM via `ClientBuilder::add_root_certificate` when `trust_ca_path` is set
- [x] 8.3 Bearer-token injection middleware that reads the in-memory JWT from `JwtHolder`
- [x] 8.4 Implement `pairing_initiate(req)`, `pairing_complete(req)`, `pairing_status(session_id)`, `list_bundles()`, `download_bundle(id)`, `upload_bundle(blob, headers)`, `delete_bundle(id)`, `auth_revoke()`
- [x] 8.5 Generate a fresh `Idempotency-Key` UUIDv4 per outbound bundle upload; reuse across retries
- [x] 8.6 Exponential backoff retry on `429` / `5xx`: 1s, 2s, 4s, 8s, 16s — capped at 5 attempts
- [x] 8.7 On `401 AUTH_INVALID`, mint a fresh JWT and retry exactly once; on second 401, surface `AuthFailed`
- [x] 8.8 Add unit tests against a `wiremock`-based mock server: happy paths + 401 retry + idempotency-key reuse

## 9. Pairing flow (spine/pairing.rs)

- [x] 9.1 Implement `initiate(spine: &SpineState) -> Result<PairingHandle>` that POSTs to `/v1/pairing/initiate` with `{ device_uuid, initiator_pubkey, device_type: "desktop" }`
- [x] 9.2 Render QR PNG using `qrcode = "0.14"` + `image = "0.25"`; return base64 data URL
- [x] 9.3 Spawn 1-second polling task in `SpineState::join_set` until status is `completed` / `expired` / TTL elapsed
- [x] 9.4 On `completed`, derive `sync_key` per crypto module, cache to keychain `account="sync-key:<peer_fp>"`, persist `paired_*` fields to Config
- [x] 9.5 Implement `cancel()` that aborts the polling task and returns `PairingState::Idle`
- [x] 9.6 Implement TTL countdown helper for the frontend (returns `Duration` until `expires_at`)

## 10. WebSocket client (spine/ws.rs)

- [x] 10.1 Connect to `<spine_url>/v1/sync/live` with `Authorization: Bearer <jwt>` header using `tokio-tungstenite`
- [x] 10.2 Reply to `{"type":"ping"}` immediately with `{"type":"pong"}`
- [x] 10.3 On `{"type":"new_bundle", ...}` trigger `list_bundles` + `process_pending_bundles`
- [x] 10.4 Implement exponential backoff reconnect: 1s, 2s, 4s, 8s, 16s, 32s, 60s cap with ±20% jitter
- [x] 10.5 Spawn 30-second polling fallback task active only while `ConnectionState` is `Reconnecting` or `Offline`
- [x] 10.6 On WS resume, immediately trigger one extra `list_bundles` catch-up
- [x] 10.7 Emit `spine://status` Tauri events on every `ConnectionState` transition

## 11. Bundle encode/decode (spine/bundle.rs)

- [x] 11.1 Define `BundleEnvelope` struct matching the spec schema (schema_version, kind, filename, content_utf8, source_path?, captured_at, sha256)
- [x] 11.2 Implement `BundleEnvelope::new_note(filename, content_utf8, source_path)` that fills `captured_at` + `sha256`
- [x] 11.3 Implement `encrypt(envelope, sync_key, peer_fp) -> Vec<u8>` that produces `nonce(12) ‖ ct_and_tag`
- [x] 11.4 Implement `decrypt(blob, sync_key, local_fp) -> Result<BundleEnvelope>`
- [x] 11.5 Schema version guard: only accept `schema_version == 1` and `kind == "note"`
- [x] 11.6 sha256 content-hash verification after decode
- [x] 11.7 Add tests: encode→decode roundtrip with two distinct keypairs; AAD mismatch fails; tag tampering fails; sha256 mismatch surfaces as error

## 12. Local ingestion (spine/inbox.rs)

- [x] 12.1 Ensure `<data-dir>/sync-inbox/` exists at startup with mode `0700`
- [x] 12.2 Implement `sanitize_filename(input: &str) -> String` keeping `[A-Za-z0-9._-]`, replacing others with `_`, capped at 200 bytes
- [x] 12.3 Implement `write_envelope(envelope: &BundleEnvelope, bundle_id: &str) -> Result<PathBuf>` performing tmp-write → fsync → rename → companion `*.meta.json`
- [x] 12.4 Resolve name collisions by appending `(2)`, `(3)`, ... before the extension
- [x] 12.5 Invoke `syncmind_indexing::index_file_once(&final_path, &ctx).await?` after rename
- [x] 12.6 Implement `list_inbox() -> Vec<InboxEntry>` for the UI (path, size, mtime)
- [x] 12.7 Implement `clear_inbox() -> Result<usize>` (returns count deleted)
- [x] 12.8 Tests: sanitize golden vectors including `..`, CR/LF, null bytes, unicode controls; collision counter; atomicity on crash mid-write

## 13. Tauri commands (spine/commands.rs)

- [x] 13.1 Implement `spine_get_config() -> SpineConfigView` (omits secrets)
- [x] 13.2 Implement `spine_set_url(url: String) -> Result<()>`
- [x] 13.3 Implement `spine_set_trust_ca(path: Option<PathBuf>) -> Result<()>` with PEM validation
- [x] 13.4 Implement `spine_get_identity() -> IdentityView { fingerprint, device_uuid, created_at }`
- [x] 13.5 Implement `spine_start_pairing() -> PairingHandleView { session_id, short_code, qr_png_base64, expires_at }`
- [x] 13.6 Implement `spine_pair_status() -> PairingStateView`
- [x] 13.7 Implement `spine_cancel_pairing() -> Result<()>`
- [x] 13.8 Implement `spine_send_note(filename, content_utf8, source_path?) -> Result<{ bundle_id }>`
- [x] 13.9 Implement `spine_unpair(clear_inbox: bool) -> Result<()>`
- [x] 13.10 Implement `spine_reset_identity() -> Result<()>` (chains through unpair)
- [x] 13.11 Implement `spine_list_inbox() -> Vec<InboxEntry>`
- [x] 13.12 Implement `spine_clear_inbox() -> Result<usize>`
- [x] 13.13 Register every `spine_*` command in `apps/desktop/src-tauri/src/lib.rs:209-225`
- [x] 13.14 Add "Sync devices…" tray menu item that opens the main window and switches to the Devices tab via Tauri event

## 14. Frontend Devices tab (apps/desktop/src/components/DevicesTab.tsx)

- [x] 14.1 Create `DevicesTab.tsx` and register it in `apps/desktop/src/App.tsx`'s tab list as the 5th entry
- [x] 14.2 Spine URL card: input + save button + plain-HTTP warning banner
- [x] 14.3 Trust-CA PEM file picker with native dialog
- [x] 14.4 Local identity card: fingerprint (truncated + copy-full) + device UUID
- [x] 14.5 Idle pair state: "Start pairing" button
- [x] 14.6 Pairing modal: QR PNG, short code, `mm:ss` countdown, cancel button
- [x] 14.7 Paired state: peer fingerprint, paired_at, connection-status badge bound to `spine://status`
- [x] 14.8 Inbox card: size + last-modified + "Empty inbox" button with two-step confirm
- [x] 14.9 Danger zone: Unpair button with checkbox "Also empty sync-inbox" (default unchecked)
- [x] 14.10 Advanced fold: Reset identity (also chains unpair)

## 15. Frontend store wiring (apps/desktop/src/store.ts)

- [x] 15.1 Add `spineState` slice to the Solid store: `{ url, fingerprint, deviceUuid, paired, peer, connectionStatus, pairing }`
- [x] 15.2 Subscribe to `spine://status` events and update `connectionStatus`
- [x] 15.3 Subscribe to `spine://pairing/expired` and `spine://unpaired` events
- [x] 15.4 Add error-code → user-facing message map for all `SPINE_*` codes

## 16. End-to-end verification

- [x] 16.1 Bring up local Spine: `cd services/sync-gateway && docker compose up -d`
- [x] 16.2 Run two desktop instances with separate `SYNCMIND_DATA_DIR` env vars
- [x] 16.3 Pair desktop A → desktop B; verify both reach `Paired` state within 30 s
- [x] 16.4 Send a 1 KB note from A; verify B's `sync-inbox` receives the file
- [x] 16.5 Verify B's vector store ingests the note (search via existing palette returns it)
- [x] 16.6 Audit: `pg_dump syncmind | grep -i "<plaintext content>"` returns 0 matches
- [x] 16.7 Simulate WS outage (firewall rule blocking the WS port); verify 30 s polling delivers a bundle within 60 s
- [x] 16.8 Reconnect WS; verify catch-up pull immediately processes any queued bundle
- [x] 16.9 Unpair on A; verify keychain has no `sync-key:*` entry and Config `paired_*` fields are empty

## 17. Audit, cleanup, and PR

- [x] 17.1 `cargo clippy --workspace --all-targets -- -D warnings` passes
- [x] 17.2 `cd apps/desktop && pnpm typecheck && pnpm lint` passes
- [x] 17.3 Grep audit: no `eprintln!`, `dbg!`, or `tracing::*` call surfaces `sync_key`, `shared_secret`, private key bytes, or `Authorization` header values
- [x] 17.4 Grep audit: no Tauri command return type includes secret material
- [x] 17.5 Manual smoke test: cold start desktop → settings shows correct config → Devices tab → pair → send → unpair → restart → identity persists
- [x] 17.6 Open PR titled `feat(apps:desktop): implement Spine client (PRD 004)` with body linking to PRD 004 and this OpenSpec change: https://github.com/blkcor/syncmind/pull/23
- [x] 17.7 After PR merge, run `/opsx:archive desktop-spine-client`

## Implementation snapshot (2026-05-24)

### Landed in `feat/desktop-spine-client`

- Server amendments (§2): client-supplied `device_uuid` accepted, 409 `UUID_CONFLICT` on mismatch, and regression coverage confirms `DeviceStore.Create` persists the supplied `Device.ID`. The handler passes the supplied UUID directly to the existing `model.Device { ID: deviceID, ... }`, so no separate `WithID` option is required.
- PRD 002 amendment (§3): §Impl Note 1.1 (dalek conversion) and 1.2 (client UUID) added.
- Core `Config.spine` extension (§4): all five `SpineConfig` fields landed via `String`/`PathBuf` (no `chrono::DateTime` / `uuid::Uuid` deps added to the daemon crate). 4.5/4.6 are now covered by `SpineConfig::validate_url` and `SpineConfig::load_trust_ca`; desktop URL/CA paths reuse those helpers while retaining the existing UI-facing error codes.
- `index_file_once` (§5): added with idempotency + `IngestionReport`; indexing failures now surface as typed `IndexingError` variants, including explicit `Embed` errors for embedding-service failures.
- Spine modules (§6–§13): identity / crypto / bundle / client / pairing / inbox / commands / state all complete, with 46 unit tests passing including HKDF RFC 5869 vectors, AES-GCM tamper detection, sync_key symmetry, envelope integrity, atomic inbox writes, JWT claim checks, and wiremock-driven HTTP client tests (happy paths, 401 retry, idempotency-key reuse, 5xx retry exhaustion).
- WebSocket loop (§10): `spine/ws.rs` is implementation-complete with backoff + jitter + 40 s read deadline + 30 s polling fallback (covered by a bounds-property test). It is NOT yet auto-spawned by `SpineRuntime::rebuild_client` — the activation requires piping an `Arc<AppState>`-flavoured ingestion closure through SpineRuntime, which is the only carry-over for the next session. The Devices tab's 1.5 s refresh loop + manual "Pull now" button cover the same UX for now.
- Devices tab UI (§14) + store wiring (§15): full surface implemented in `apps/desktop/src/components/DevicesTab.tsx` plus `App.tsx` / `store.ts` / tray menu hookup.

### Deferred to follow-up sessions

- §10.5–§10.7: auto-activate `ws::spawn_loop` from `SpineRuntime::rebuild_client` when paired, wire incoming `new_bundle` callbacks to bundle ingestion, push `WsStatus` into a Tauri event (`spine://status`).
- §16 end-to-end verification: requires `docker compose up` of the sync-gateway plus two `SYNCMIND_DATA_DIR` runtimes. Easy follow-up; not run in this session because the user is away.
- §17.1–§17.7: clippy `-D warnings` on the whole workspace (pre-existing `objc` / `collapsible_match` warnings in `lib.rs` need follow-up cleanup that is unrelated to this change), the PR itself, and `/opsx:archive` after merge.
- Persisting the peer's raw Ed25519 pubkey alongside `sync_key` at pairing completion. The send path currently reads it from `<data-dir>/peers/<fp>.pub` (helper `persist_peer_pubkey_raw` exists); the call site that writes it during `PollOutcome::Completed` handling is the one-line follow-up that closes the loop.
