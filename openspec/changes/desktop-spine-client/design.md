## Context

PRD 004 (`docs/prd/004-desktop-spine-client.md`) defines US-020..US-030 for the desktop side of the Spine protocol. PRD 003 and the OpenSpec archive `2026-05-20-the-spine/` shipped the server: a deliberately blind sync gateway that stores opaque AES-GCM ciphertext, exposes pairing + bundle + WebSocket APIs over HTTPS, and verifies EdDSA JWTs against per-device Ed25519 public keys.

The desktop app today (`apps/desktop/`) is a Tauri v2 + SolidJS shell with four tabs (Search / Pinned / RAG Lab / Settings), a tray icon, and direct Cargo `path` dependencies on `syncmind-core`, `syncmind-storage`, `syncmind-rag-engine`, `syncmind-indexing`, and `syncmind-file-watcher`. It has no secrets storage, no networking, no QR rendering, no WebSocket client. PRD 003 §Impl Notes §1 confirmed that all key generation, ECDH, encryption, JWT signing, and bundle interpretation are explicit client responsibilities.

Constraints (from CLAUDE.md and PRD 004):
- Privacy is absolute — vectors and plaintext stay on-device; the server never sees keys.
- The Rust headless core daemon must remain ≤ 100 MB idle; the desktop app already exceeds that envelope and gets a separate 20 MB allowance for sync.
- No new daemon process; everything runs in the existing Tauri Tokio runtime.
- `core/storage::VectorStore` public API stays unchanged.
- `core/file-watcher` does NOT gain directory recursion in this change (deferred per the user).

## Goals / Non-Goals

**Goals:**
- Implement PRD 004's US-020..US-030: identity, pairing UI, sync_key derivation, JWT, bundle encryption/decryption with integrity verification, WebSocket + polling fallback, sync-inbox materialization, unpair/reset.
- Keep the desktop binary's RSS increase below 20 MB at steady state and below 80 MB during a 100-bundle catch-up burst.
- Make all crypto operations testable with deterministic golden vectors so the protocol can be cross-verified against a future mobile client.
- Coordinate the small server amendment (client-supplied `device_uuid` in pairing) inside this same change so the protocol contract evolves atomically.
- Amend PRD 003 §Impl Note 1 with the exact Ed25519↔X25519 conversion contract so future clients implement it identically.

**Non-Goals:**
- No QR scanning / camera support — desktop is the QR-displaying initiator only (PRD 004 §NG-15).
- No directory-recursive file-watcher — the user wants to think about that separately (PRD 004 §Open Q 2 answer).
- No mobile-media bundles — `application/syncmind.note+json` only (PRD 004 §NG-20).
- No multi-peer pairing — single `paired_peer` (PRD 004 §NG-16).
- No Double Ratchet / forward secrecy — fixed `sync_key` per pairing lifecycle (PRD 004 §NG-18).
- No automatic sync-inbox cleanup — manual UI only (PRD 004 §Open Q 5 answer).
- No `core/storage` API changes.

## Decisions

### 1. Single Tauri process; long-running tasks share the existing Tokio runtime via `JoinSet`
**Rationale:** Adding a separate sync daemon would duplicate the OS-level resident process and violate the "no new daemon" constraint. The existing `AppState` in `apps/desktop/src-tauri/src/lib.rs` already manages a Tokio runtime for the indexing pipeline; extending it with a `SpineState` containing a `JoinSet<()>` is the minimal-disruption path.

**Alternatives considered:** Spawning a sidecar binary via Tauri's shell plugin — rejected: doubles RSS and complicates IPC.

### 2. OS keychain via the `keyring` crate; Linux fallback to a 0600 file
**Rationale:** macOS Keychain, Windows Credential Manager, and libsecret-backed providers on Linux give strong at-rest protection without inventing a passphrase UX. The `keyring` crate (v3) wraps all three with a single API and supports a `mock` provider for tests.

**Alternatives considered:**
- Passphrase-protected file via Argon2id KDF — rejected: forces a passphrase prompt on every cold start, hurts UX, and the on-disk threat model isn't actually stronger than the OS keychain.
- Plaintext 0600 file always — rejected as the default: too weak for a key that grants access to all synced content. Retained as the Linux fallback when libsecret is absent, with a stderr warning.

### 3. Ed25519 ↔ X25519 conversion uses dalek's native helpers
**Rationale:** PRD 003 §Impl Note 1 leaves the conversion algorithm unspecified. The dalek family (`ed25519-dalek 2`, `curve25519-dalek 4`) ships the canonical conversion: `SigningKey::to_scalar_bytes()` on the private side and `CompressedEdwardsY::decompress()?.to_montgomery().to_bytes()` on the public side. Fixing this now via a PRD 003 amendment makes the contract reproducible for the future mobile client.

**Alternatives considered:** Third-party crates like `ed25519-to-curve25519` — rejected: pulls in extra transitive deps; dalek's API already covers it.

### 4. Client mints its own UUIDv4 at identity creation; server amended to accept it
**Rationale:** PRD 003 §US-013 made `JWT.sub = devices.id` and the server currently generates `devices.id` server-side at pairing. That forces the client to either (a) round-trip to learn the UUID after pairing before it can sign anything, or (b) keep two identifiers in sync. Letting the client supply the UUID at `pairing/initiate` and `pairing/complete` and having the server persist it as `devices.id` removes a state-coupling round-trip and matches the user's answer to Open Question 3.

**Alternatives considered:**
- Server-assigned, client-fetches-via-/me — rejected: requires a new `/v1/me` endpoint and adds latency before the first signed request.
- Use `public_key_fingerprint` as `device_id` — rejected: leaks the key fingerprint into JWT subject claims and surfaces in all logs.

**Conflict semantics:** if the client-supplied UUID is already bound to a different `public_key_fingerprint`, server returns `409 UUID_CONFLICT` and the client surfaces it to the user. This is rare (UUIDv4 collision) and recoverable by re-running pairing.

### 5. Bundle plaintext is a versioned JSON envelope sealed inside AES-256-GCM
**Rationale:** A JSON envelope (`schema_version`, `kind`, `filename`, `content_utf8`, `source_path?`, `captured_at`, `sha256`) is human-debuggable, easy to extend (`kind: "media"` in the future), and lets us include a content hash for cross-end integrity verification without relying on server-visible headers. The sha256 is over `content_utf8.as_bytes()` only — not the whole envelope — so the receiver can verify the content even if it later re-serializes the envelope.

**Alternatives considered:**
- Raw bytes + side-channel `X-Syncmind-Content-Type` — rejected: leaks content type semantics to the server (already known to the server, but raw bytes have no schema versioning).
- Tar/zip with manifest — rejected: overkill for single-note v1; revisit if `kind: "media"` lands.

### 6. AAD is `SHA-256(peer_ed25519_pubkey_raw_32_bytes)` (32 bytes)
**Rationale:** Authenticated additional data binds each ciphertext to a specific pairing context. Even if two distinct pairings somehow shared a `sync_key` (impossible by HKDF, but the threat model is "even then"), AAD mismatch causes GCM tag verification to fail. The sender's AAD = SHA-256(receiver's pubkey); the receiver's AAD = SHA-256(its own pubkey). Symmetric and computable from data each side already has.

**Alternatives considered:** empty AAD — rejected: surrenders this defense layer for no gain. Including session_id — rejected: session_id changes on every re-pair while pubkeys persist, complicating bundle re-decryption from local storage.

### 7. Decrypted notes land in a sync-inbox directory and are indexed via a new `index_file_once` entry point
**Rationale:** `core/file-watcher` currently registers a fixed file list at startup and does not recursively watch directories. The user explicitly deferred adding directory recursion to a separate change. The minimal path is to expose `pub async fn index_file_once(path: &Path) -> Result<IngestionReport>` from `core/syncmind-indexing` that reuses the existing extract → chunk → embed → upsert pipeline for a single file. The spine `inbox` module calls this synchronously after the atomic `tmp → fsync → rename`, only ACK'ing the bundle once indexing succeeds.

**Alternatives considered:**
- Append inbox files to `Config.registered_files` live + reload the watcher — rejected: write amplifies the config file, races with the watcher's startup snapshot, and surfaces sync-internal paths into the user-visible "registered files" list.
- Add directory recursion to `core/file-watcher` — rejected for this change; deferred per the user.

### 8. WebSocket-first with 30-second polling fallback
**Rationale:** WebSocket gives sub-second latency on the happy path; polling guarantees eventual delivery during reconnect storms. Exponential backoff on WS reconnect (1 → 60 s cap, ±20% jitter) plus a 30-second polling loop while WS is `Reconnecting` or `Offline` matches PRD 004 §US-027. On WS resume, an immediate `GET /v1/sync/bundles` catches up anything queued during the gap.

**Alternatives considered:**
- WS-only — rejected: stale data during outages.
- 5-second aggressive polling — rejected: server pressure; 30 s is plenty for the human-attention timescale this product targets.

### 9. Spine URL allows HTTP, IP addresses, and operator-supplied self-signed CA PEM
**Rationale:** Self-hosting users on home NAS often lack public DNS or public CAs. Rejecting `http://` or self-signed certs outright would push them to ngrok/tunneling. The trade-off: surface a non-blocking UI banner when scheme is `http://`, and ship a "Trust self-signed CA (PEM file)" picker that hands the certificate to `reqwest::ClientBuilder::add_root_certificate`. `danger_accept_invalid_certs` is never enabled — the user explicitly supplies a CA cert; verification is still performed.

**Alternatives considered:**
- HTTPS-only in production builds — rejected per user's answer to Open Question 4.
- `danger_accept_invalid_certs(true)` toggle — rejected: too easy to leave on, silently disables verification.

### 10. sync-inbox cleanup is manual
**Rationale:** Decrypted notes are local files the user may want to keep as audit copies. Per the user's answer to Open Question 5, the Devices tab shows total size + last-modified for the inbox and an "Empty inbox" button gated by a two-step confirmation. No background LRU.

**Alternatives considered:**
- LRU by age (e.g., delete after 90 days) — rejected: surprises users; if they need a cap, they can run it themselves or we revisit later with telemetry.

## Architecture

### Rust module layout (`apps/desktop/src-tauri/src/spine/`)

```
spine/
├── mod.rs        - SpineState singleton (OnceCell + tokio::sync::Mutex), JoinSet management
├── identity.rs   - Ed25519 key generation, keychain I/O, fingerprint, sign() interface
├── crypto.rs     - HKDF-SHA256, AES-256-GCM, Ed25519↔X25519, EdDSA JWT mint/verify
├── pairing.rs    - initiate / status polling / QR PNG render / sync_key derivation
├── client.rs     - reqwest HTTPS client + endpoint wrappers + idempotency / retry
├── ws.rs         - tokio-tungstenite long connection, heartbeat, exp backoff, polling fallback
├── bundle.rs     - envelope JSON schema + encrypt/decrypt + AAD computation
├── inbox.rs      - sync-inbox file writing (atomic), sanitize, calls index_file_once
└── commands.rs   - Tauri command wrappers (registered from lib.rs)
```

### State machines

Three independent machines, each backed by a `tokio::sync::watch::Sender` so the frontend can subscribe via Tauri events:

- `IdentityState`: `Uninitialized | Loaded { fingerprint, device_uuid }`
- `PairingState`: `Idle | Pending { session_id, expires_at } | Completing | Paired { peer_fingerprint } | Failed { code, message }`
- `ConnectionState`: `Disabled | Connecting | Connected | Reconnecting { attempt } | Offline`

All transitions go through `SpineState::transition()` which atomically updates the state and emits a Tauri `spine://status` event.

### Background task orchestration

`SpineState` owns a `JoinSet<()>` for:
- WebSocket connection loop (reconnect + heartbeat)
- 30 s polling fallback (active only while WS down)
- JWT refresh task (wakes 5 minutes before `exp`)
- Pairing status polling (only while a session is active)

On unpair or app shutdown, `JoinSet::abort_all()` is called and all tasks drain cleanly.

## Crypto wire format

### Bundle blob (over the wire, server-visible)

```
bundle_blob = nonce (12 bytes) ‖ ciphertext_and_tag (N + 16 bytes)
```

- `nonce`: 12 random bytes from `OsRng`.
- `ciphertext_and_tag`: AES-256-GCM(`sync_key`, `nonce`, AAD, plaintext).
- AAD: 32-byte SHA-256 of the **peer's** raw Ed25519 public key (32 bytes input → 32 bytes hash).

The server stores `bundle_blob` opaquely in `sync_bundles.encrypted_payload` and computes `payload_hash = SHA-256(bundle_blob)` for transport-integrity verification.

### Plaintext envelope (inside the ciphertext)

```json
{
  "schema_version": 1,
  "kind": "note",
  "filename": "<sanitized, UTF-8 NFC, ≤ 255 bytes>",
  "content_utf8": "<the note body>",
  "source_path": "<optional, sender's local path>",
  "captured_at": "<RFC3339 UTC>",
  "sha256": "<lower-hex SHA-256 of content_utf8.as_bytes()>"
}
```

Receiver MUST verify:
1. `payload_hash` header equals SHA-256 of the downloaded blob.
2. AES-GCM decryption succeeds with AAD = SHA-256(local Ed25519 pubkey).
3. `schema_version == 1` and `kind == "note"`.
4. `lower_hex(SHA-256(content_utf8.as_bytes())) == envelope.sha256`.

Any failure → skip + record in `failed_bundles` + do NOT send DELETE.

### sync_key derivation

```
ed25519_priv (32-byte scalar bytes)  ← SigningKey::to_scalar_bytes()
peer_ed25519_pub (32-byte CompressedEdwardsY)
                                     ↓ to_montgomery()
peer_x25519_pub (32-byte Curve25519 point)
shared_secret = X25519(ed25519_priv_as_x25519, peer_x25519_pub)  (32 bytes)
sync_key = HKDF-SHA256(
    ikm  = shared_secret,
    salt = session_id_string.as_bytes(),
    info = b"syncmind-v1"
).expand_to(32 bytes)
```

`sync_key` is cached in the OS keychain as `service = "syncmind"`, `account = "sync-key:<peer_fingerprint>"`, value = base64(`sync_key`). Wiped on unpair.

## Server amendments

### `services/sync-gateway/internal/handler/pairing.go`

```go
type InitiateRequest struct {
    DeviceUUID      string `json:"device_uuid"`       // NEW, required, UUIDv4
    InitiatorPubkey string `json:"initiator_pubkey"`  // unchanged
    DeviceType      string `json:"device_type"`       // unchanged
}

type CompleteRequest struct {
    SessionID       string `json:"session_id"`        // unchanged
    DeviceUUID      string `json:"device_uuid"`       // NEW, required, UUIDv4
    ResponderPubkey string `json:"responder_pubkey"`  // unchanged
    DeviceType      string `json:"device_type"`       // unchanged
}
```

Server logic:
1. Parse `device_uuid`; reject `400 INVALID_REQUEST` if missing or not a valid UUIDv4.
2. On `initiate`: ensure no existing `devices` row has this UUID with a different `public_key_fingerprint`. If the row exists with the SAME fingerprint, treat as device-recovery (allow). If the row exists with a DIFFERENT fingerprint, return `409 UUID_CONFLICT`.
3. On `complete`: same conflict check for the responder; insert the new device row using the supplied UUID as the primary key. (The existing `gen_random_uuid()` DEFAULT on `devices.id` stays for any non-pairing inserts; pairing handlers pass the UUID explicitly.)

No DB migration is required — `devices.id` is already UUID PRIMARY KEY; the handler just supplies the value instead of relying on Postgres default.

### `services/sync-gateway/internal/model/device.go`

`CreateDevice` gains a `WithID(uuid.UUID)` option; the existing default-creation path is unchanged.

### PRD 003 §Impl Note 1 amendment

The note currently reads (paraphrased): "pairing sessions store Ed25519 pubkeys, not X25519; clients derive sync_key locally." It will be extended with a normative sub-note pinning the conversion:

> **§1.1 (added):** Clients SHALL convert Ed25519 keys to X25519 using `ed25519-dalek::SigningKey::to_scalar_bytes()` on the private side and `curve25519_dalek::edwards::CompressedEdwardsY::decompress(...).to_montgomery().to_bytes()` on the public side. The shared secret is `X25519(local_x25519_priv, peer_x25519_pub)`; `sync_key = HKDF-SHA256(shared_secret, salt = session_id_bytes, info = b"syncmind-v1")`. See PRD 004 §US-023.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| **libsecret missing on Linux CI runners** | `keyring` mock provider for tests; Linux fallback to `<data-dir>/keys/device.ed25519` (0600) for first-run desktop builds; document `apt install libsecret-1-dev` for full integration tests. |
| **UUID collision between independently-installed desktops on the same Spine** | `409 UUID_CONFLICT` from server; client surfaces and forces re-pair (regenerates `device.json`). UUIDv4 collision probability is negligible in practice. |
| **rustls + user-supplied self-signed CA edge cases** | `reqwest::ClientBuilder::add_root_certificate` accepts a single PEM; if user supplies a chain, document that intermediates concatenate; never enable `danger_accept_invalid_certs`. |
| **WebSocket reconnect storm against unreachable Spine** | Backoff cap at 60 s + jitter prevents thundering-herd; 30 s polling stays passive (single GET per cycle). |
| **AES-GCM nonce reuse if `OsRng` is broken** | 96-bit random nonce has birthday bound ~2³² before collision matters; spec documents "rotate sync_key (re-pair) after 10⁷ bundles". |
| **Plaintext leakage in logs** | All `tracing` instrumentation filters `Authorization` header, `bundle_blob`, `sync_key`, `shared_secret`; CI grep audit gate in section 17 of tasks.md. |
| **Server amendment breaks the existing archived spec** | The `device-pairing` and `device-auth` spec deltas explicitly use MODIFIED requirements and ship in the same change; CHANGELOG documents that no in-the-wild client exists yet so the on-the-wire amendment is safe. |
| **Sync-inbox grows unbounded for long-running installs** | Manual cleanup UI shows total size + last-modified; user can wipe. Add LRU later if telemetry shows accumulation. |

## Migration Plan

There is no production deployment yet (the Spine has no clients; the desktop has no sync feature). Migration is therefore a single coordinated change with no rollout sequencing:

1. Land server amendments (section 2 of `tasks.md`) first so the server accepts the new pairing payload.
2. Land the PRD 003 §Impl Note 1 amendment.
3. Land core/syncmind-core Config extension + core/syncmind-indexing `index_file_once`.
4. Land the desktop `spine/` modules.
5. Land the SolidJS Devices tab.
6. End-to-end test against `docker compose up` Spine.

**Rollback:** Revert the merge commit. No external state to clean up (no Spine has been pointed at by any user yet). The amended server still accepts the old behavior because the new `device_uuid` field is treated as required only when the handler is in "client-UUID mode"; rolling back the server reverts to `gen_random_uuid()` default and rejects requests sending `device_uuid`.

## Open Questions

None blocking this change. PRD 004 §Open Questions 1–5 were answered by the user; all five are reflected in the decisions above. The deferred-but-acknowledged item is directory-recursive file-watching (PRD 004 §OQ 2), which is intentionally out of scope and tracked as future work in `core/file-watcher`.
