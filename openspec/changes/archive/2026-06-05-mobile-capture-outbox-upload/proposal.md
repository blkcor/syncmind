## Why

US-047 is the first point where mobile capture becomes durable instead of just local UI state. The app needs to encrypt each capture with the paired desktop's `sync_key`, persist the encrypted blob in an offline queue, and flush it through Spine without leaking plaintext to logs or transient retry code.

This also replaces the current in-memory `apps/mobile/src/outbox/service.ts` stub with the persistent outbox contract that later US-048 status UI and US-049 recent-capture list depend on.

## What Changes

- Add mobile capture bundle encryption that matches the desktop Spine envelope wire format:
  - plaintext is the UTF-8 JSON `BundleEnvelope`
  - `BundleEnvelope.sha256` is lower-hex SHA-256 of `content_utf8` bytes
  - Spine's relay-level `payload_hash` remains SHA-256 of the encrypted blob
  - encrypted blob is `nonce(12) | ciphertext_and_tag`
  - AES-256-GCM key is `PersistedPairingState.syncKey`
  - AAD is derived from the paired peer fingerprint, matching the desktop ingestion expectation
- Replace the mobile outbox stub with an `expo-sqlite` persistent queue table:
  - `id`, `created_at`, `state`, `attempts`, `last_error`, `encrypted_blob`
  - states: `pending`, `sending`, `failed`, `done`
  - queue limit: 1000 unfinished rows (`pending`, `sending`, `failed`); `done` rows are excluded and cleaned after 7 days
- Add secure serialization guardrails:
  - `secureSerialize()` is the only path from capture payload object to plaintext bytes
  - plaintext is never written to console, breadcrumbs, persistent storage, or retry metadata
  - development builds fail fast if plain `JSON.stringify()` is used for bundle payload serialization in the outbox path
- Add ordered flush/upload behavior:
  - use `authenticatedFetch()` for Ed25519 JWT Bearer auth and stale-pairing handling
  - upload with `POST /v1/sync/bundle`, `Content-Type: application/octet-stream`, `X-Syncmind-Content-Type`, and stable `Idempotency-Key: <bundle.id>`
  - retry `429` and `5xx` up to 3 attempts with 1s/4s/16s backoff
  - non-retryable failures become `failed`
  - process restart/background transition resets stale `sending` rows to `pending`
- Register background queue flush:
  - iOS and Android use Expo background fetch/task manager where available
  - no promise of second-level background delivery; foreground and app-start flush remain the deterministic paths
- Preserve lifecycle integration with existing pairing state:
  - unpair/device reset continues to clear the outbox
  - pairing loss pauses queued sends without deleting encrypted rows
- Add a minimal capture-screen outbox status surface so US-047 is user-observable:
  - show the most recent 3 outbox rows with `done`, `sending`, `pending`, or `failed`
  - refresh from local SQLite on outbox state changes and with a 10s polling fallback
  - keep full retry/delete/copy controls and the Recent tab in later US-048/US-049 scope

## Capabilities

### New Capabilities

- `mobile-capture-outbox-upload`: Mobile capture envelope encryption, secure serialization, persistent encrypted outbox, ordered flush, retry, and background upload scheduling.

### Modified Capabilities

- `sync-bundle-relay`: Clarifies that mobile capture upload consumes the existing `POST /v1/sync/bundle` opaque bundle contract and uses the same idempotency semantics as desktop uploads; no server endpoint shape change is expected.

## Impact

- `apps/mobile/package.json` — add `expo-sqlite`, `expo-task-manager`, `expo-background-fetch`, and `@noble/ciphers`.
- `apps/mobile/src/outbox/service.ts` — replace the memory-only stub with SQLite-backed queue creation, enqueue, state transitions, flush, startup recovery, and clear behavior.
- `apps/mobile/src/spine/client.ts` — add bundle upload helper on top of `authenticatedFetch()` and share in-flight abort handling with unpair.
- `apps/mobile/src/spine/session.ts` — read `syncKey` and peer identifiers already persisted by pairing; no schema change expected.
- `apps/mobile/src/crypto/` or `apps/mobile/src/outbox/` — add mobile envelope encryption and secure serialization helpers.
- `apps/mobile/app/(tabs)/index.tsx` — after send, enqueue encrypted capture immediately, trigger a best-effort flush, and show a minimal local status list for the latest 3 outbox rows.
- `apps/mobile/__tests__/` — add focused tests for encryption shape, secure serialization guardrails, outbox persistence/state transitions, retry behavior, stale `sending` recovery, queue cap, and unpair clearing.
