# desktop-spine-client Specification

## Purpose
Defines the desktop client's Spine subsystem capabilities: Ed25519 device identity management, secure key storage, Spine URL configuration, JWT authentication, end-to-end bundle encryption, WebSocket sync, pairing flow, sync-inbox materialization, and the Devices tab UI.

## Requirements

### Requirement: Ed25519 device identity is generated and stored in the OS keychain
The desktop client SHALL generate a persistent Ed25519 signing key at first launch and SHALL store the private key in the operating system's secure credential store (Keychain on macOS, Credential Manager on Windows, libsecret on Linux). The raw private key SHALL NOT cross any IPC boundary, SHALL NOT appear in any log, and SHALL NOT be persisted in any file other than the OS credential store.

#### Scenario: First-launch identity creation
- **WHEN** the desktop application starts and no `service="syncmind", account="device-identity"` entry exists in the OS keychain
- **THEN** the client generates a fresh Ed25519 signing key using `OsRng`
- **AND** the client persists the PKCS#8 v2 encoded private key under `service="syncmind", account="device-identity"`
- **AND** the client derives `fingerprint = lower_hex(SHA-256(public_key))`
- **AND** the client mints a fresh UUIDv4 as `device_uuid`
- **AND** the client writes `{ fingerprint, device_type: "desktop", device_uuid, created_at }` to `<data-dir>/device.json` (without the private key)

#### Scenario: Subsequent launch reuses existing identity
- **WHEN** the desktop application starts and an Ed25519 identity already exists in the keychain
- **THEN** the client loads the existing private key from the keychain
- **AND** the client verifies the derived fingerprint matches the fingerprint cached in `<data-dir>/device.json`
- **AND** if the fingerprints disagree, the client treats this as tampered state and refuses to start the Spine feature, surfacing a `KEYCHAIN_FINGERPRINT_MISMATCH` error

#### Scenario: Linux libsecret unavailable
- **WHEN** the desktop application starts on Linux and the `keyring` crate reports no available secret service
- **THEN** the client falls back to storing the private key at `<data-dir>/keys/device.ed25519` with file mode `0600` and directory mode `0700`
- **AND** the client emits a stderr warning instructing the user to install libsecret or use an encrypted home directory

#### Scenario: Private key never crosses IPC
- **WHEN** any Tauri command listed in `apps/desktop/src-tauri/src/lib.rs` returns a value to the frontend
- **THEN** the returned structure SHALL NOT contain the raw Ed25519 private key bytes
- **AND** the returned structure SHALL NOT contain the PKCS#8 encoded private key

### Requirement: Spine URL is user-configured with relaxed scheme and CA rules
The desktop client SHALL accept any `http://` or `https://` Spine URL, including URLs whose host is a raw IP address, and SHALL allow the user to supply a self-signed CA certificate (PEM) that is added to the HTTP client's root store via `reqwest::ClientBuilder::add_root_certificate`. The client SHALL NOT enable `danger_accept_invalid_certs` under any circumstance. The Devices tab SHALL display a non-blocking warning banner whenever the configured scheme is plain HTTP.

#### Scenario: HTTPS URL with public CA
- **WHEN** the user sets `spine.url = "https://spine.example.com"` and no `trust_ca_path` is set
- **THEN** the client constructs `reqwest::Client` with default rustls root store
- **AND** all Spine requests succeed if the server certificate validates against the public CA store

#### Scenario: HTTPS URL with self-signed CA
- **WHEN** the user sets `spine.trust_ca_path = "/home/me/spine-ca.pem"` and the file is a valid PEM certificate
- **THEN** the client calls `ClientBuilder::add_root_certificate(pem)` before building the client
- **AND** requests to the configured Spine URL succeed if the server certificate chains to that CA

#### Scenario: HTTP URL warning
- **WHEN** the user sets `spine.url = "http://192.168.1.10:8080"`
- **THEN** the Devices tab displays a yellow warning banner stating that traffic is not transport-encrypted
- **AND** Spine requests still succeed (the warning is non-blocking)

#### Scenario: Invalid URL rejected
- **WHEN** the user attempts to save a Spine URL that fails to parse via `url::Url::parse`
- **THEN** the `spine_set_url` Tauri command returns error code `INVALID_URL`
- **AND** the previous configured URL is preserved

### Requirement: Pairing displays QR + short code and polls for completion
The desktop client SHALL initiate a pairing session by calling `POST /v1/pairing/initiate` with the device's Ed25519 public key and client-minted UUID, render the returned QR payload as a PNG, display the server-issued 6-digit short code, and poll `GET /v1/pairing/:session_id/status` at one-second intervals until the session reaches a terminal state (`completed`, `expired`, or `cancelled`) or the TTL elapses.

#### Scenario: Initiation succeeds
- **WHEN** the user clicks "Start pairing" in the Devices tab and the Spine URL is configured
- **THEN** the client sends `POST /v1/pairing/initiate` with body `{ device_uuid, initiator_pubkey, device_type: "desktop" }`
- **AND** on `200 OK` the client renders a QR PNG of `qr_payload` at >= 256 px square with error-correction level M
- **AND** the client displays the `short_code` and a `mm:ss` countdown to `expires_at`
- **AND** the client starts a polling task that calls `GET /v1/pairing/:session_id/status` every second

#### Scenario: Pairing completed
- **WHEN** the polling task receives a status of `completed` with a peer fingerprint
- **THEN** the client derives `sync_key` per the sync-key derivation requirement
- **AND** the client persists `spine.paired_peer_fingerprint` and `spine.paired_at` to the Config
- **AND** the client transitions `PairingState` to `Paired`
- **AND** the polling task terminates

#### Scenario: Pairing expired
- **WHEN** the polling task receives a status of `expired` or the TTL elapses with no terminal status
- **THEN** the client transitions `PairingState` to `Failed { code: "PAIRING_EXPIRED" }`
- **AND** the QR window emits a `spine://pairing/expired` event to the frontend

#### Scenario: User cancels mid-pairing
- **WHEN** the user closes the pairing modal before completion
- **THEN** the client stops the polling task
- **AND** the client transitions `PairingState` back to `Idle`
- **AND** the server-side session is left to expire naturally (no explicit cancel endpoint)

### Requirement: Sync key is derived via HKDF-SHA256 and cached per peer
After pairing completes, the desktop client SHALL convert its Ed25519 private key and the peer's Ed25519 public key to Curve25519 form, compute `shared_secret = X25519(local_x25519_priv, peer_x25519_pub)`, derive `sync_key = HKDF-SHA256(ikm=shared_secret, salt=session_id_bytes, info=b"syncmind-v1")` of length 32 bytes, and cache the result in the OS keychain under `service="syncmind", account="sync-key:<peer_fingerprint>"`. The conversion SHALL use `ed25519_dalek::SigningKey::to_scalar_bytes()` on the private side and `curve25519_dalek::edwards::CompressedEdwardsY::decompress(...).to_montgomery()` on the public side.

#### Scenario: Successful derivation
- **WHEN** pairing transitions to `completed` with a known peer Ed25519 public key
- **THEN** the client computes `x25519_priv = ed25519_signing_key.to_scalar_bytes()`
- **AND** the client computes `x25519_peer_pub = CompressedEdwardsY(peer_ed25519_pub).decompress()?.to_montgomery().to_bytes()`
- **AND** the client computes `shared_secret = x25519_dalek::x25519(x25519_priv, x25519_peer_pub)`
- **AND** the client computes `sync_key = HKDF-SHA256(shared_secret, salt=session_id.as_bytes(), info=b"syncmind-v1").expand(32)`
- **AND** the client writes `base64(sync_key)` to the keychain under `account="sync-key:<peer_fingerprint>"`

#### Scenario: Sync key never leaves the backend
- **WHEN** any Tauri command returns to the frontend
- **THEN** the returned payload SHALL NOT contain `sync_key` bytes in any form (raw, base64, hex, or embedded in another field)

#### Scenario: Sync key wiped on unpair
- **WHEN** the `spine_unpair` Tauri command completes
- **THEN** the keychain SHALL NOT contain any entry whose `account` field starts with `"sync-key:"`

### Requirement: JWTs are signed by the device's Ed25519 identity and held only in memory
The desktop client SHALL mint Ed25519-signed JWTs (`alg=EdDSA`) for all authenticated Spine requests. JWT claims SHALL include `sub` (= `device_uuid` from `device.json`), `iat`, `exp` (= `iat + 3600`), `jti` (UUIDv4), `iss = "syncmind-client"`, and `aud = "syncmind-spine"`. The signed token SHALL be kept only in process memory and SHALL never be persisted to disk. The client SHALL automatically refresh the token 5 minutes before `exp`.

#### Scenario: First JWT mint
- **WHEN** the client needs to make its first authenticated Spine request after startup
- **THEN** the client mints a JWT with the claims listed above
- **AND** the client signs it with the Ed25519 private key from the keychain
- **AND** the client holds the token in a `tokio::sync::RwLock<Option<MintedJwt>>`

#### Scenario: Automatic refresh
- **WHEN** the current JWT's `exp` is less than 5 minutes in the future
- **THEN** a background task mints a new JWT with a fresh `jti` and `iat`
- **AND** the new token replaces the previous one atomically

#### Scenario: 401 triggers single re-mint
- **WHEN** any Spine HTTPS or WebSocket request receives a `401` response with code `AUTH_INVALID` (or any 401 from the server)
- **THEN** the client mints a fresh JWT and retries the request once
- **AND** if the retry also returns 401, the client transitions to `ConnectionState::Offline` and emits a `spine://auth/failed` event

#### Scenario: JWT not persisted
- **WHEN** the desktop application restarts
- **THEN** no file under `<data-dir>` or `<config-dir>` contains the previous JWT
- **AND** the first authenticated request after restart triggers a fresh mint

### Requirement: Outbound bundles are AES-256-GCM encrypted with peer-fingerprint AAD
The desktop client SHALL encrypt every outbound bundle using AES-256-GCM with key = `sync_key`, a fresh 12-byte random nonce per bundle, and AAD = the 32-byte SHA-256 of the peer's raw Ed25519 public key. The wire format SHALL be `nonce(12) | ciphertext_and_tag`. The plaintext SHALL be a UTF-8 JSON envelope with fields `schema_version` (= 1), `kind` (= "note"), `filename`, `content_utf8`, `source_path` (optional), `captured_at`, and `sha256` (lower-hex SHA-256 of `content_utf8.as_bytes()`).

#### Scenario: Note encryption and upload
- **WHEN** the user invokes `spine_send_note(filename, content_utf8, source_path?)`
- **THEN** the client builds the plaintext envelope with `captured_at = Utc::now().to_rfc3339()` and the computed `sha256`
- **AND** the client serializes the envelope as UTF-8 JSON bytes
- **AND** the client generates a 12-byte random nonce with `OsRng`
- **AND** the client encrypts using AES-256-GCM with the cached `sync_key` and AAD = `SHA-256(peer_ed25519_pubkey)`
- **AND** the client sends `POST /v1/sync/bundle` with body `nonce | ciphertext_and_tag`, headers `X-Syncmind-Content-Type: application/syncmind.note+json`, `Idempotency-Key: <uuid_v4>`, `Authorization: Bearer <jwt>`
- **AND** on `201 Created` the client returns `{ bundle_id }` to the frontend

#### Scenario: Empty note rejected pre-flight
- **WHEN** `spine_send_note` is invoked with `content_utf8.is_empty()`
- **THEN** the command returns error code `EMPTY_NOTE` without making any network call

#### Scenario: Oversized bundle rejected pre-flight
- **WHEN** the constructed `bundle_blob` exceeds the configured `max_bundle_size_mb` (default 50 MiB)
- **THEN** the command returns error code `BUNDLE_TOO_LARGE` without making any network call

#### Scenario: Retry preserves idempotency
- **WHEN** an upload fails with `429` or any `5xx` response
- **THEN** the client retries up to 5 times with exponential backoff (1s -> 2s -> 4s -> 8s -> 16s)
- **AND** every retry uses the same `Idempotency-Key` value

### Requirement: Inbound bundles are integrity-checked before local materialization
For every bundle the desktop client downloads from the Spine, the client SHALL verify (1) that `SHA-256(bundle_blob)` equals the `X-Syncmind-Payload-Hash` response header, (2) that AES-GCM decryption succeeds with AAD = `SHA-256(local Ed25519 pubkey)`, (3) that the decoded envelope has `schema_version == 1` and `kind == "note"`, and (4) that `SHA-256(envelope.content_utf8.as_bytes())` equals `envelope.sha256`. If any check fails, the client SHALL skip the bundle, record its ID in the local `failed_bundles` set, and SHALL NOT send `DELETE /v1/sync/bundles/:id`.

#### Scenario: Happy-path receive
- **WHEN** the client downloads a bundle whose payload hash matches, decryption succeeds, schema is recognized, and the content hash matches
- **THEN** the client writes the note to the sync-inbox per the sync-inbox materialization requirement
- **AND** the client sends `DELETE /v1/sync/bundles/:id` to ACK
- **AND** the client adds the bundle ID to the local `processed_bundle_ids` set

#### Scenario: Transport hash mismatch
- **WHEN** the downloaded `bundle_blob`'s SHA-256 differs from the `X-Syncmind-Payload-Hash` header
- **THEN** the client records the bundle ID in `failed_bundles`
- **AND** the client does NOT call `DELETE /v1/sync/bundles/:id`
- **AND** the client logs a warning (without exposing ciphertext bytes)

#### Scenario: GCM tag verification fails
- **WHEN** AES-GCM decryption fails (tag mismatch, AAD mismatch, or corrupted ciphertext)
- **THEN** the client records the bundle ID in `failed_bundles` and does NOT ACK

#### Scenario: Unknown schema version
- **WHEN** the decoded envelope's `schema_version` is not 1
- **THEN** the client records the bundle ID in `failed_bundles`, logs a `SCHEMA_VERSION_UNSUPPORTED` warning, and does NOT ACK

#### Scenario: Envelope content hash mismatch
- **WHEN** the SHA-256 of the decoded `content_utf8` does not equal `envelope.sha256`
- **THEN** the client records the bundle ID in `failed_bundles` and does NOT ACK

### Requirement: WebSocket connection self-heals with exponential backoff and 30 s polling fallback
The desktop client SHALL maintain a WebSocket connection to `<spine_url>/v1/sync/live` whenever a valid pairing exists, respond to server `ping` messages with `pong` within the server's read deadline, reconnect with exponential backoff (1 s -> 60 s cap with +/-20% jitter) on disconnect, and run a 30-second polling loop against `GET /v1/sync/bundles` whenever the WebSocket is in `Reconnecting` or `Offline` state. On WebSocket resume, the client SHALL immediately perform a catch-up `GET /v1/sync/bundles`.

#### Scenario: WebSocket connected, notification triggers pull
- **WHEN** the WebSocket receives a `{"type":"new_bundle", ...}` message
- **THEN** the client triggers a `GET /v1/sync/bundles?limit=50` and processes every returned bundle

#### Scenario: Heartbeat reply
- **WHEN** the WebSocket receives `{"type":"ping"}`
- **THEN** the client immediately replies with `{"type":"pong"}`

#### Scenario: Reconnect with backoff
- **WHEN** the WebSocket disconnects (any reason)
- **THEN** the client transitions `ConnectionState` to `Reconnecting`
- **AND** the client waits `base * (0.8 + rand * 0.4)` seconds before the next attempt, with `base` doubling from 1 to a cap of 60
- **AND** the client emits a `spine://status` event on every state transition

#### Scenario: Polling fallback active during outage
- **WHEN** `ConnectionState` is `Reconnecting` or `Offline` for >= 30 seconds
- **THEN** the client performs a `GET /v1/sync/bundles?limit=50` every 30 seconds
- **AND** the client processes any new bundles per the inbound integrity-check requirement

#### Scenario: Catch-up pull on reconnect
- **WHEN** the WebSocket transitions from `Reconnecting` to `Connected`
- **THEN** the client immediately performs one additional `GET /v1/sync/bundles?limit=50` without waiting for the next notification

### Requirement: Decrypted notes are written to sync-inbox and indexed via single-file pipeline
The desktop client SHALL materialize every successfully verified inbound note to `<data-dir>/sync-inbox/<captured_at_unix_ms>-<sanitized_filename>` using an atomic write (`tmp` write -> `fsync` -> `rename`), THEN invoke `syncmind_indexing::index_file_once(path)`, THEN send `DELETE /v1/sync/bundles/:id` to ACK. The ACK SHALL NOT be sent if any of write, fsync, rename, or indexing fails. The filename SHALL be sanitized to ASCII letters, digits, `-`, `_`, `.`, with all other bytes replaced by `_` and the total length capped at 200 bytes.

#### Scenario: Successful materialization and indexing
- **WHEN** a verified envelope is ready for materialization
- **THEN** the client computes the target path `<data-dir>/sync-inbox/<captured_at_unix_ms>-<sanitized_filename>`
- **AND** the client writes to `<target>.tmp`, calls `fsync`, then renames to `<target>`
- **AND** the client calls `syncmind_indexing::index_file_once(&target).await?`
- **AND** only after both succeed does the client send `DELETE /v1/sync/bundles/:id`

#### Scenario: Name collision is disambiguated
- **WHEN** the target path already exists
- **THEN** the client appends `(2)`, `(3)`, ... before the extension until the path is free

#### Scenario: Path traversal in filename is neutralized
- **WHEN** an inbound envelope's `filename` contains `..` or path separators
- **THEN** the sanitizer replaces those bytes with `_` before constructing the target path
- **AND** the final path SHALL be confined under `<data-dir>/sync-inbox/`

#### Scenario: Indexing failure does not ACK
- **WHEN** `index_file_once` returns an error after the file has been renamed into place
- **THEN** the client does NOT send `DELETE /v1/sync/bundles/:id`
- **AND** the client records the bundle ID in `failed_bundles`
- **AND** the sync-inbox file remains on disk for manual inspection

### Requirement: Unpair and reset cleanly tear down local secrets and server-side session
The desktop client SHALL provide a `spine_unpair` command that (1) sends `POST /v1/auth/revoke` with the current JWT on a best-effort basis, (2) closes the WebSocket, (3) wipes every keychain entry with `account` starting with `"sync-key:"`, (4) clears `spine.paired_peer_fingerprint`, `spine.paired_peer_device_type`, `spine.paired_at`, and `spine.peer_device_id_uuid` from the Config, and (5) emits `spine://unpaired`. The desktop client SHALL also provide a `spine_reset_identity` command that performs unpair and additionally wipes `service="syncmind", account="device-identity"` and deletes `<data-dir>/device.json`.

#### Scenario: Unpair leaves no sync_key behind
- **WHEN** the user confirms unpair
- **THEN** the client calls `POST /v1/auth/revoke` (failures are logged but do not block the rest of unpair)
- **AND** the client closes the WebSocket
- **AND** the client wipes every keychain entry whose account starts with `sync-key:`
- **AND** the client saves the Config with all `paired_*` fields set to `None`
- **AND** the client emits `spine://unpaired` to the frontend
- **AND** subsequent `spine_send_note` calls return `NOT_PAIRED`

#### Scenario: Reset identity regenerates UUID
- **WHEN** the user confirms reset identity via the Advanced section of the Devices tab
- **THEN** unpair is performed first
- **AND** the keychain entry `service="syncmind", account="device-identity"` is removed
- **AND** the file `<data-dir>/device.json` is deleted
- **AND** the next application launch generates a fresh Ed25519 keypair and UUIDv4

#### Scenario: sync-inbox preserved by default
- **WHEN** the user confirms unpair without checking the "Empty sync-inbox" option
- **THEN** the `<data-dir>/sync-inbox/` directory and its contents remain on disk

#### Scenario: sync-inbox cleared on demand
- **WHEN** the user confirms unpair with the "Empty sync-inbox" option checked
- **THEN** every file under `<data-dir>/sync-inbox/` is deleted
- **AND** the directory itself is recreated with mode `0700`
