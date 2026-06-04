# mobile-capture-outbox-upload Specification

## Purpose

Defines the mobile capture outbox persistence, envelope encryption, ordered upload with retry, background flush, and capture screen integration for SyncMind mobile clients.
## Requirements
### Requirement: Capture payloads are securely serialized before encryption

The mobile app SHALL convert capture payload objects into UTF-8 plaintext bytes only through a `secureSerialize()` helper used by the bundle encryption path. For text captures, the inner plaintext payload serialized into `BundleEnvelope.content_utf8` SHALL contain `v: 1`, `kind: "capture-text"`, `id`, `text`, `source: "typed"`, `client_ts`, and `client_device_fingerprint`. The app MUST NOT log, persist, or attach plaintext capture payloads to diagnostics, retry metadata, or outbox rows beyond the bounded mini-preview metadata explicitly used by the capture screen.

#### Scenario: Text capture serialization produces plaintext bytes only transiently

- **WHEN** the user sends a non-empty text capture
- **THEN** the app constructs an inner capture payload object with `v = 1`, `kind = "capture-text"`, `id`, `text`, `source = "typed"`, `client_ts`, and `client_device_fingerprint`
- **AND** `client_device_fingerprint` is the local mobile device fingerprint string
- **AND** constructs an outer `BundleEnvelope` with `schema_version = 1`, `kind = "capture-text"`, `filename`, `content_utf8`, `captured_at`, and `sha256`
- **AND** `content_utf8` contains the serialized inner capture payload
- **AND** `secureSerialize()` returns UTF-8 JSON bytes of the outer envelope for encryption
- **AND** the outbox row stores only encrypted bytes and non-sensitive metadata

#### Scenario: Plaintext is not logged during enqueue

- **WHEN** a capture is enqueued
- **THEN** no call in the enqueue/encrypt/outbox path writes the plaintext text, payload object, or plaintext JSON to `console.log`, `console.warn`, `console.error`, breadcrumbs, or local retry metadata

#### Scenario: Direct stringify is rejected in development guard

- **WHEN** development/test code attempts to serialize a bundle payload in the outbox path without `secureSerialize()`
- **THEN** the guard fails fast before the payload can be persisted or uploaded

### Requirement: Mobile capture bundles use desktop-compatible AES-GCM wire format

The mobile app SHALL encrypt every capture bundle using AES-256-GCM with a 32-byte `syncKey`, a fresh 96-bit random nonce, and AAD derived from the paired peer fingerprint bytes. The `syncKey` and `pairedPeerFingerprint` values SHALL come from `PersistedPairingState` as defined by the `mobile-pairing-state-management` capability. The wire blob SHALL be `nonce(12) | ciphertext_and_tag`.

#### Scenario: Encrypted text capture matches bundle wire shape

- **WHEN** the app encrypts a `capture-text` payload
- **AND** restored pairing state contains `syncKey` and `pairedPeerFingerprint`
- **THEN** encryption uses `syncKey` as the AES-256-GCM key
- **AND** encryption uses a fresh 12-byte random nonce
- **AND** encryption uses AAD equal to the bytes decoded from `pairedPeerFingerprint` after the `sha256:` prefix
- **AND** the output blob starts with the 12-byte nonce followed by ciphertext and the 16-byte GCM tag

#### Scenario: Envelope content hash matches desktop validation

- **WHEN** the app constructs a `BundleEnvelope`
- **THEN** it computes `sha256` as lower-hex SHA-256 of `content_utf8` UTF-8 bytes
- **AND** the value is included in `BundleEnvelope.sha256`
- **AND** desktop `BundleEnvelope::validate()` can verify the hash after decryption

#### Scenario: Tampered encrypted blob fails fixture validation

- **WHEN** a test flips any byte in a deterministic encrypted fixture
- **THEN** decrypting the blob with the same key, nonce, and AAD fails authentication

### Requirement: Outbox is persisted in SQLite

The mobile app SHALL persist encrypted outbox rows in an `expo-sqlite` table named `outbox`. The table SHALL contain `id TEXT PRIMARY KEY`, `created_at TEXT NOT NULL`, `state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'failed', 'done'))`, `attempts INTEGER NOT NULL DEFAULT 0`, `last_error TEXT`, and `encrypted_blob BLOB NOT NULL`. The app SHALL create an index on `(state, created_at)` for queue scans.

#### Scenario: Outbox schema is created on first use

- **WHEN** any outbox API is called for the first time
- **THEN** the app creates the `outbox` table if it does not exist
- **AND** creates the `(state, created_at)` index if it does not exist
- **AND** does not create columns for plaintext text, previews, response bodies, stack traces, or request URLs

#### Scenario: Enqueued capture survives process restart

- **WHEN** a capture is encrypted and enqueued
- **AND** the app process is restarted before upload succeeds
- **THEN** reopening the outbox returns the row with the same `id`, `created_at`, `state`, `attempts`, `last_error`, and `encrypted_blob`
- **AND** plaintext capture content is not present in the table

#### Scenario: Queue state values are constrained

- **WHEN** code attempts to write an outbox row
- **THEN** `state` MUST be one of `pending`, `sending`, `failed`, or `done`
- **AND** invalid states are rejected before or by SQLite

#### Scenario: Queue cap rejects new unfinished captures

- **WHEN** the outbox already contains 1000 rows whose state is `pending`, `sending`, or `failed`
- **AND** the user attempts to enqueue another capture
- **THEN** the app rejects the new capture before upload
- **AND** the capture screen can surface "Capture queue is full - connect to upload or retry failed captures"
- **AND** no plaintext fallback row is created

#### Scenario: Done rows do not consume queue capacity

- **WHEN** the outbox contains `done` rows
- **THEN** those rows do not count toward the 1000-row enqueue cap
- **AND** successfully uploaded history cannot block new captures by itself

#### Scenario: Done rows are cleaned up

- **WHEN** the app initializes or foregrounds the outbox
- **THEN** it deletes `done` rows older than 7 days
- **AND** future configurable retention belongs to US-049

#### Scenario: Clear outbox deletes persisted rows

- **WHEN** `clearOutbox()` is called by unpair or device reset
- **THEN** all persisted outbox rows are deleted
- **AND** subsequent `getOutboxItems()` or status queries return an empty queue

### Requirement: Outbox flush uploads rows in order with idempotency

The mobile app SHALL flush pending outbox rows in FIFO order through Spine using `POST /v1/sync/bundle`, authenticated with `authenticatedFetch()`, and each upload SHALL reuse `Idempotency-Key: <outbox.id>` for all attempts of the same row.

#### Scenario: Successful upload marks row done

- **WHEN** `flushOutbox()` processes the oldest `pending` row
- **AND** Spine returns HTTP 201 Created
- **THEN** the app marks the row `done`
- **AND** stores no plaintext response or payload content
- **AND** continues to the next eligible row

#### Scenario: Upload request uses required headers

- **WHEN** the app uploads an encrypted bundle
- **THEN** it sends `POST {spineUrl}/v1/sync/bundle`
- **AND** the request body is the raw `encrypted_blob`
- **AND** the request includes `Authorization: Bearer <jwt>` through `authenticatedFetch()`
- **AND** the request includes `Content-Type: application/octet-stream`
- **AND** the request includes `X-Syncmind-Content-Type: application/syncmind.capture-text+json` for `capture-text` bundles
- **AND** the request includes `Idempotency-Key` equal to the outbox row id

#### Scenario: Retryable upload errors use bounded backoff

- **WHEN** an upload attempt returns HTTP 429 or any 5xx response
- **THEN** the app increments `attempts`
- **AND** retries with delays of 1s, 4s, and 16s while reusing the same `Idempotency-Key`
- **AND** after the third failed attempt marks the row `failed` with a whitelisted `last_error`

#### Scenario: Non-retryable upload errors fail the row

- **WHEN** an upload attempt returns a non-429 4xx response
- **THEN** the app marks the row `failed`
- **AND** records only a whitelisted `last_error`
- **AND** does not retry automatically

#### Scenario: last_error uses a whitelist

- **WHEN** the app records an outbox error
- **THEN** `last_error` MUST be one of `HTTP_400`, `HTTP_401`, `HTTP_403`, `HTTP_404`, `HTTP_409`, `HTTP_413`, `HTTP_415`, `HTTP_422`, `HTTP_429`, `HTTP_500`, `HTTP_502`, `HTTP_503`, `HTTP_504`, `NETWORK_ERROR`, `UNPAIRED`, `QUEUE_FULL`, `BUNDLE_TOO_LARGE`, or `UNKNOWN_ERROR`
- **AND** `last_error` MUST NOT include plaintext payload content, request URLs, response bodies, stack traces, JWTs, encrypted blob bytes, or sync keys

#### Scenario: Single-flight prevents duplicate concurrent flushes

- **WHEN** two app paths call `flushOutbox()` at the same time
- **THEN** only one flush loop uploads rows
- **AND** the other call observes the in-flight flush or returns without starting a second uploader

### Requirement: Outbox recovers safely after process death or background transition

The mobile app SHALL reset stale `sending` rows to `pending` on startup and foreground recovery so interrupted uploads can resume from the queue head.

#### Scenario: Startup resets sending rows

- **WHEN** the app starts
- **AND** the outbox contains rows in `sending`
- **THEN** the app changes those rows to `pending`
- **AND** the next flush attempts them in `created_at` order

#### Scenario: Background transition does not lose encrypted rows

- **WHEN** the app is backgrounded or killed while a row is `sending`
- **THEN** the encrypted row remains persisted
- **AND** the row is eligible for retry after the next startup or foreground recovery

### Requirement: Background flush is opportunistic

The mobile app SHALL register an Expo background task named `SYNCMIND_OUTBOX_FLUSH` that attempts to flush the outbox when the operating system schedules background work, while preserving foreground/app-start flush as the deterministic delivery path.

#### Scenario: Background task invokes flush

- **WHEN** the OS invokes `SYNCMIND_OUTBOX_FLUSH`
- **THEN** the task initializes the outbox if needed
- **AND** calls `flushOutbox()`
- **AND** returns Expo's `BackgroundFetchResult.NewData` when at least one upload was attempted
- **AND** returns `BackgroundFetchResult.NoData` when no eligible row exists
- **AND** returns `BackgroundFetchResult.Failed` only when initialization or flush throws unexpectedly

#### Scenario: Background upload is not promised as immediate

- **WHEN** a capture is enqueued while the app later goes to background
- **THEN** the app does not promise upload within a fixed number of seconds
- **AND** the encrypted row remains queued until a foreground or OS-scheduled flush succeeds

### Requirement: Pairing loss pauses flush without deleting queued captures

The mobile app SHALL stop flushing when pairing state is missing or `authenticatedFetch()` reports `UnpairedError`, and it SHALL leave encrypted queued rows intact unless an explicit unpair/device-reset cleanup clears the outbox.

#### Scenario: Missing pairing state pauses queue

- **WHEN** `flushOutbox()` runs without restored pairing state
- **THEN** it performs no upload
- **AND** leaves `pending` and `failed` rows in the outbox

#### Scenario: Authenticated fetch unpairs during upload

- **WHEN** an upload attempt triggers `UnpairedError`
- **THEN** `flushOutbox()` stops processing further rows
- **AND** does not delete encrypted queued rows
- **AND** relies on explicit unpair/device reset to clear the queue

### Requirement: Capture screen enqueues and triggers best-effort flush

The paired capture screen SHALL enqueue a text capture immediately when the user taps Send, clear the text input optimistically after successful enqueue, and trigger a best-effort flush.

#### Scenario: Send enqueues text capture

- **WHEN** the app is paired
- **AND** the user enters non-whitespace text and taps Send
- **THEN** the app encrypts and enqueues a `capture-text` bundle
- **AND** clears the text input after enqueue succeeds
- **AND** starts a best-effort `flushOutbox()`

#### Scenario: Empty text is rejected locally

- **WHEN** the app is paired
- **AND** the user taps Send with empty or whitespace-only text
- **THEN** no outbox row is created
- **AND** no upload is attempted

### Requirement: Capture screen shows recent outbox status

The paired capture screen SHALL show a minimal local status list for the 3 most recent outbox rows using only non-sensitive outbox metadata. The status list SHALL refresh from SQLite when outbox state changes and SHALL also poll every 10 seconds as a fallback.

#### Scenario: Recent outbox statuses are visible without plaintext previews

- **WHEN** the app is paired
- **AND** the outbox contains rows in `done`, `sending`, `pending`, or `failed`
- **THEN** the capture screen shows up to 3 rows ordered by `created_at DESC`
- **AND** each row shows the state label or icon for `done`, `sending`, `pending`, or `failed`
- **AND** failed rows may show only the whitelisted `last_error` and attempt count
- **AND** the screen does not read, decrypt, display, or persist plaintext capture content

#### Scenario: Status UI refreshes after local outbox changes

- **WHEN** enqueue or flush changes an outbox row
- **THEN** the capture screen refreshes its status list from SQLite through an in-process change notification
- **AND** a 10-second polling fallback refreshes the same local query while the screen remains paired

