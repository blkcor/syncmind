## 1. Dependency Smoke Tests

- [x] 1.1 Add mobile dependencies for `expo-sqlite`, `expo-task-manager`, `expo-background-fetch`, and `@noble/ciphers`.
- [x] 1.2 Add a narrow Jest smoke test proving SQLite can create `outbox`, insert/read/delete a BLOB, and preserve byte equality.
- [x] 1.3 Add a narrow crypto smoke test proving the chosen AES-GCM implementation can encrypt/decrypt with 32-byte key, 12-byte nonce, and AAD.

## 2. Bundle Crypto and Secure Serialization

- [x] 2.1 Create the mobile bundle/envelope module under `apps/mobile/src/outbox/` or `apps/mobile/src/crypto/`.
- [x] 2.2 Implement `secureSerialize(payload)` as the only payload-to-UTF-8 JSON path for capture bundles.
- [x] 2.3 Implement `BundleEnvelope.sha256` calculation as lower-hex SHA-256 over `content_utf8` bytes.
- [x] 2.4 Implement peer-fingerprint AAD decoding from `sha256:<hex>` to raw bytes.
- [x] 2.5 Implement AES-256-GCM encryption returning `nonce(12) | ciphertext_and_tag`.
- [x] 2.6 Add deterministic crypto fixtures with fixed key, nonce, AAD, plaintext, expected hash, ciphertext, and tamper failure.
- [x] 2.7 Add tests proving plaintext is not written through console calls or outbox metadata during enqueue/encrypt.

## 3. SQLite Outbox Service

- [x] 3.1 Replace the in-memory `apps/mobile/src/outbox/service.ts` implementation with lazy `expo-sqlite` initialization.
- [x] 3.2 Create the `outbox` table with `id`, `created_at`, `state`, `attempts`, `last_error`, and `encrypted_blob`.
- [x] 3.3 Add an index for queue scans by `state` and `created_at`.
- [x] 3.4 Implement enqueue with a 1000-row cap over `pending`/`sending`/`failed` rows only and encrypted-blob-only persistence.
- [x] 3.5 Implement status/state helpers for `pending`, `sending`, `failed`, and `done`.
- [x] 3.6 Implement `resetSendingToPending()` for startup/foreground recovery.
- [x] 3.7 Implement fixed cleanup of `done` rows older than 7 days.
- [x] 3.8 Preserve `clearOutbox()` as the unpair/device-reset cleanup boundary.
- [x] 3.9 Add tests for persistence across service re-open, state constraints, unfinished-row queue cap rejection, done rows excluded from cap, done cleanup, and clear behavior.

## 4. Upload and Retry Flow

- [x] 4.1 Add a mobile bundle upload helper in `spine/client.ts` or an outbox uploader module using `authenticatedFetch()`.
- [x] 4.2 Send `POST {spineUrl}/v1/sync/bundle` with raw encrypted blob body.
- [x] 4.3 Include `Content-Type: application/octet-stream`, `X-Syncmind-Content-Type: application/syncmind.capture-text+json`, and `Idempotency-Key: <outbox.id>`.
- [x] 4.4 Implement single-flight `flushOutbox()` that processes eligible rows in `created_at ASC` order.
- [x] 4.5 Mark each row `sending` before upload and `done` after HTTP 201 Created.
- [x] 4.6 Retry HTTP 429 and 5xx with 1s/4s/16s backoff and stable idempotency key.
- [x] 4.7 Mark non-429 4xx and exhausted retry rows `failed` with a whitelisted `last_error`.
- [x] 4.8 Implement `last_error` whitelist mapping and reject plaintext/URL/body/stack-bearing error strings.
- [x] 4.9 Stop flush on missing pairing state or `UnpairedError` without deleting encrypted queued rows.
- [x] 4.10 Add tests for success, retry reuse of idempotency key, failed-row transition, last_error whitelist, single-flight behavior, and unpaired pause.

## 5. Lifecycle and Background Flush Hooks

- [x] 5.1 Wire outbox initialization and `resetSendingToPending()` into app startup.
- [x] 5.2 Wire foreground recovery to reset stale `sending` rows and trigger best-effort flush.
- [x] 5.3 Register Expo background task/fetch hooks under `SYNCMIND_OUTBOX_FLUSH` that initialize the outbox and call `flushOutbox()`.
- [x] 5.4 Ensure background task code treats OS scheduling as opportunistic and does not expose fixed-time delivery promises.
- [x] 5.5 Add tests for stale `sending` recovery, background task flush invocation, and BackgroundFetchResult mapping.

## 6. Capture Screen Integration

- [x] 6.1 Replace the `// TODO: wire to spine_send_note` send placeholder in `apps/mobile/app/(tabs)/index.tsx`.
- [x] 6.2 Build a `capture-text` payload from trimmed input while the app is paired.
- [x] 6.3 Encrypt and enqueue the capture before clearing the input.
- [x] 6.4 Clear the text input optimistically after enqueue succeeds and call `Keyboard.dismiss()`.
- [x] 6.5 Trigger best-effort `flushOutbox()` after enqueue without blocking UI on network success.
- [x] 6.6 Surface queue-cap rejection with the copy "Capture queue is full - connect to upload or retry failed captures".
- [x] 6.7 Add tests for send enqueue, empty text rejection, optimistic clear after enqueue, and queue-cap error.
- [x] 6.8 Add a recent-status outbox query that returns latest local metadata without encrypted blobs or plaintext previews.
- [x] 6.9 Add in-process outbox change notifications plus 10s polling fallback for CaptureScreen status refresh.
- [x] 6.10 Render the latest 3 outbox states on CaptureScreen and add focused UI tests.

## 7. Verification

- [x] 7.1 Run `pnpm --filter mobile typecheck`.
- [x] 7.2 Run `pnpm --filter mobile lint`.
- [x] 7.3 Run `pnpm --filter mobile test --runInBand`.
- [x] 7.4 Run focused manual smoke: paired text capture -> encrypted row created -> Spine 201 -> row marked `done`.
- [x] 7.5 Run focused manual smoke: offline text capture -> restart app -> row remains `pending` -> reconnect -> upload succeeds.
- [x] 7.6 Run focused manual smoke: unpair clears persisted outbox rows and preserves Ed25519 identity.
- [x] 7.7 Run focused tests for recent outbox status query and CaptureScreen status UI.
- [x] 7.8 Re-run `pnpm --filter mobile typecheck`.
- [x] 7.9 Re-run `pnpm --filter mobile lint`.
- [x] 7.10 Re-run `pnpm --filter mobile test --runInBand`.
