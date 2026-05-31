## ADDED Requirements

### Requirement: Camera permission request with graceful degradation

The mobile app SHALL request camera permission through `expo-camera` before activating the QR scanner. When the user denies permission, the app SHALL display a manual raw JSON pairing payload input form as fallback.

The mobile app SHALL declare a native iOS camera usage description for QR pairing so that iOS TCC can present the camera permission prompt instead of terminating the app.

#### Scenario: Camera permission granted and scanner activates

- **WHEN** the user navigates to the pairing screen for the first time
- **AND** the user grants camera permission when prompted
- **THEN** the QR scanner viewfinder activates and begins detecting QR codes

#### Scenario: Native camera privacy description is configured

- **WHEN** the app is built for iOS
- **THEN** the generated Info.plist includes `NSCameraUsageDescription`
- **AND** the value explains that SyncMind uses the camera to scan desktop pairing QR codes
- **AND** QR pairing does not request microphone access

#### Scenario: Camera permission denied shows manual input fallback

- **WHEN** the user denies camera permission
- **THEN** the scanner viewfinder is replaced with a multiline text input labeled "Paste pairing payload"
- **AND** a "Submit" button is visible below the input
- **AND** the input accepts the raw JSON string copied from the desktop Devices panel

#### Scenario: Permission blocked permanently links to system settings

- **WHEN** the user has previously denied camera permission permanently
- **THEN** the UI displays a message "Camera access is blocked" with a button "Open Settings" that deep-links to the app's system settings page

### Requirement: QR payload validation on scan

The mobile app SHALL parse and validate the scanned QR code content as a corrected v1 pairing payload before proceeding to the pairing completion step. Validation errors SHALL produce user-readable error messages.

#### Scenario: Valid v1 payload passes validation

- **WHEN** the scanner decodes a QR code whose content is a JSON object with `v: 1`, `kind: "syncmind-pairing"`, `session_id: "<uuid-v4>"`, `spine_url: "https://spine.example.com:8443"`, `ca_fingerprint: null`, `pairing_token: "<opaque string>"`, `expires_at: "<RFC 3339 UTC>"`, `device_a_pubkey: "<base64url-no-pad ed25519 pubkey>"`, and `device_a_fingerprint: "sha256:<hex>"`
- **AND** `expires_at` is in the future (allowing ±60s clock drift)
- **AND** `device_a_fingerprint` equals `sha256:` plus the SHA-256 lower-hex digest of the decoded `device_a_pubkey`
- **THEN** validation succeeds and the payload fields are passed to the pairing completion step

#### Scenario: Payload missing session_id requires desktop update

- **WHEN** the scanned JSON has `v: 1` and `kind: "syncmind-pairing"` but does not contain `session_id`
- **THEN** validation fails with message "Desktop version too old — update SyncMind Desktop and generate a new QR code"
- **AND** the app does not attempt to use `pairing_token` as the session id

#### Scenario: Schema version mismatch produces error

- **WHEN** the scanned JSON has `v: 2` or any value other than `1`
- **THEN** an error screen displays "App version too old — please update SyncMind Mobile and try again"
- **AND** a "Scan again" button returns to the scanner

#### Scenario: Expired payload produces error

- **WHEN** the scanned JSON has `expires_at` more than 60 seconds in the past relative to device local time
- **THEN** an error screen displays "QR code expired — please generate a new one from the desktop Devices panel"
- **AND** a "Scan again" button returns to the scanner

#### Scenario: Non-https spine_url rejected in production

- **WHEN** the scanned JSON has `spine_url` starting with `http://` (not `https://`)
- **AND** `__DEV__` is `false`
- **THEN** validation fails with message "Insecure connection — HTTPS is required"
- **AND** the payload is rejected

#### Scenario: http spine_url accepted in dev mode

- **WHEN** the scanned JSON has `spine_url` starting with `http://`
- **AND** `__DEV__` is `true`
- **THEN** validation succeeds and the payload is accepted

#### Scenario: Malformed JSON produces error

- **WHEN** the scanned QR content is not valid JSON
- **THEN** an error screen displays "Invalid QR code — this doesn't look like a SyncMind pairing code"
- **AND** a "Scan again" button returns to the scanner

### Requirement: Stable mobile device UUID

The mobile app SHALL maintain a stable UUIDv4 device identifier for Spine pairing and future JWT `sub` values. This UUID SHALL be generated locally once and restored across app restarts.

#### Scenario: Device UUID is generated before first pairing

- **WHEN** the mobile app starts pairing and no `self_device_uuid` exists in secure storage
- **THEN** the app generates a UUIDv4
- **AND** stores it in `expo-secure-store` under the pairing/session namespace
- **AND** uses that UUID as `device_uuid` in pairing completion

#### Scenario: Existing device UUID is reused

- **WHEN** `self_device_uuid` already exists in secure storage
- **THEN** the app reuses the stored UUID for pairing completion
- **AND** does not generate a new UUID

### Requirement: Pairing completion via Spine API

The mobile app SHALL call `POST {spine_url}/v1/pairing/complete` with the `session_id` from the QR payload, the mobile device UUID, the mobile Ed25519 identity public key, and `device_type: "mobile"`.

#### Scenario: Successful pairing completion

- **WHEN** the app sends a POST to `/v1/pairing/complete` with body `{ "session_id": "<uuid-v4>", "device_uuid": "<uuid-v4>", "responder_pubkey": "<base64url-no-pad ed25519 pubkey>", "device_type": "mobile" }`
- **AND** the Spine responds with HTTP 200 and body `{ "status": "completed", "session_id": "<uuid-v4>", "initiator_id": "<uuid>", "responder_id": "<uuid>", "initiator_pubkey": "<base64url-no-pad ed25519 pubkey>" }`
- **AND** `initiator_pubkey` matches the QR payload's `device_a_pubkey` when the response field is present
- **THEN** the handshake proceeds to sync_key derivation

#### Scenario: Pairing session expired on server

- **WHEN** the Spine responds with HTTP 410 Gone and error code `PAIRING_EXPIRED`
- **THEN** the UI displays "QR code expired — please generate a new one from the desktop Devices panel"

#### Scenario: Pairing session already completed

- **WHEN** the Spine responds with HTTP 409 Conflict and error code `PAIRING_ALREADY_COMPLETED`
- **THEN** the UI displays "This QR code has already been used — if someone else paired your desktop, check your Devices panel"
- **AND** the scanner returns to ready state

#### Scenario: Device UUID conflict

- **WHEN** the Spine responds with HTTP 409 Conflict and error code `UUID_CONFLICT`
- **THEN** the UI displays "This mobile identity is already registered differently — reset device identity before pairing again"
- **AND** the pairing state is not persisted

#### Scenario: Network error during pairing completion

- **WHEN** the fetch to `/v1/pairing/complete` throws a network error (timeout, DNS failure, connection refused)
- **THEN** the UI displays "Cannot reach {spine_url} — check your network connection"
- **AND** a "Retry" button re-attempts the completion call with the same `session_id` and `device_uuid`

### Requirement: Sync key derivation from Ed25519-to-X25519 shared secret

The mobile app SHALL derive `sync_key = HKDF-SHA256(shared_secret, salt=session_id, info="syncmind-v1")` where `shared_secret` is computed from the mobile native Ed25519 identity converted to X25519 and the desktop Ed25519 public key converted to X25519.

#### Scenario: Sync key derived successfully

- **WHEN** pairing completion succeeds with a valid `session_id`
- **AND** the desktop Ed25519 public key is available from the QR payload's `device_a_pubkey`
- **THEN** the app converts `device_a_pubkey` to an X25519 public key
- **AND** calls the native identity facade to derive a 32-byte X25519 shared secret without exposing the mobile private key to JS
- **AND** derives a 32-byte `sync_key` using HKDF-SHA256 with `salt=session_id` and `info="syncmind-v1"`

#### Scenario: Sync key derivation matches desktop derivation

- **WHEN** both mobile and desktop derive `sync_key` from the same converted identity keys, session_id, and info string
- **THEN** both sides produce byte-for-byte identical 32-byte keys
- **AND** the key is suitable for use as an AES-256-GCM symmetric key

### Requirement: Pairing state persistence in secure store

The mobile app SHALL persist pairing state to `expo-secure-store` after successful key derivation, replacing any previous pairing data.

#### Scenario: Pairing state written after successful handshake

- **WHEN** the sync_key is derived and the Spine returns `initiator_id` and `responder_id`
- **THEN** the following items are written to `expo-secure-store`:
  - `self_device_uuid` (the mobile UUID used as `device_uuid`)
  - `sync_key` (base64 encoded 32-byte key)
  - `paired_peer_fingerprint` (e.g., `"sha256:abcdef..."`)
  - `paired_peer_device_id` (`initiator_id` from Spine response)
  - `paired_peer_device_type` = `"desktop"`
  - `paired_at` (ISO 8601 UTC timestamp)
  - `spine_url` (the validated URL)
  - `ca_fingerprint` (string or `"null"`)

#### Scenario: Previous pairing data is overwritten

- **WHEN** the device was previously paired and the user scans a new QR code
- **THEN** any existing `sync_key`, `paired_peer_*`, and `spine_url` entries in `expo-secure-store` are overwritten with the new values
- **AND** `self_device_uuid` is preserved unless the user performs a full device reset
- **AND** no old session data remains

#### Scenario: Zustand store reflects paired state

- **WHEN** pairing state is persisted
- **THEN** `useAppStore.getState().isPaired` is `true`
- **AND** `peerDeviceFingerprint` equals the pairing peer's fingerprint
- **AND** `connectionStatus` is `"connected"`

### Requirement: Pairing session restoration on app restart

The mobile app SHALL restore pairing state from `expo-secure-store` on app startup, without requiring re-pairing.

#### Scenario: Session restored after app restart

- **WHEN** the app launches and `expo-secure-store` contains a valid `self_device_uuid`, `sync_key`, `spine_url`, and `paired_peer_fingerprint`
- **THEN** `spine/session.ts` loads these values into the runtime session
- **AND** `useAppStore` enters `isPaired: true` without showing the pairing screen

#### Scenario: Missing or corrupted session falls back to unpaired state

- **WHEN** the app launches and any required key (`self_device_uuid`, `sync_key`, `spine_url`, `paired_peer_fingerprint`) is missing
- **THEN** the app enters unpaired state
- **AND** the pairing screen is shown

### Requirement: CA fingerprint metadata for self-signed certificates

When the QR payload's `ca_fingerprint` field is not `null`, the mobile app SHALL validate its format and persist it with pairing state. MVP SHALL NOT claim TLS certificate pinning unless the runtime can inspect the presented certificate chain.

#### Scenario: CA fingerprint present and format is valid

- **WHEN** `ca_fingerprint` is `"sha256:ABCD1234..."`
- **AND** the hex portion is a valid SHA-256 fingerprint
- **THEN** the value is accepted and persisted with pairing state

#### Scenario: CA fingerprint present but format is invalid

- **WHEN** `ca_fingerprint` is not `null` and does not match `sha256:<64 hex chars>`
- **THEN** validation fails with message "Invalid certificate fingerprint in QR code"
- **AND** pairing is aborted

#### Scenario: Certificate chain is inspectable and mismatched

- **WHEN** the runtime can access the TLS certificate chain for `spine_url`
- **AND** the presented certificate SHA-256 fingerprint does not match `ca_fingerprint`
- **THEN** an error screen displays "Certificate doesn't match — possible network tampering"
- **AND** pairing is aborted

#### Scenario: Certificate chain inspection unavailable in MVP

- **WHEN** the runtime cannot access the raw TLS certificate chain
- **THEN** the app logs a warning: "CA fingerprint validation skipped — not available in this environment"
- **AND** pairing proceeds only if standard system trust accepts the TLS connection
- **AND** the `ca_fingerprint` value is still persisted for future native re-validation

#### Scenario: No CA fingerprint (system trust)

- **WHEN** `ca_fingerprint` is JSON `null`
- **THEN** standard system CA trust is used
- **AND** no additional certificate validation is performed

### Requirement: Post-pairing navigation to first capture guide

After successful pairing, the mobile app SHALL navigate the user to the capture screen, and on first pairing, display a brief introduction.

#### Scenario: First pairing shows guide

- **WHEN** pairing completes successfully
- **AND** no previous pairing has been recorded (no prior `paired_peer_fingerprint` in secure store)
- **THEN** the app navigates to the capture screen
- **AND** a one-time overlay or card reads "Send your first note! Type anything and hit Send — your desktop will index it."

#### Scenario: Re-pairing skips guide

- **WHEN** pairing completes successfully
- **AND** there was a previous pairing (indicated by the now-overwritten `paired_peer_fingerprint`)
- **THEN** the app navigates directly to the capture screen without the introduction overlay
