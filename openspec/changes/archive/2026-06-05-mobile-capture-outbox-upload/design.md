## Context

PRD 005 US-047 requires mobile captures to become encrypted, durable, and uploadable while offline. The current mobile app already has pairing state, a native Ed25519 identity, Ed25519 JWT-authenticated Spine requests, unpair cleanup, and a capture screen with a send placeholder. It also has `apps/mobile/src/outbox/service.ts`, but that service is currently an in-memory array and cannot survive process death.

The desktop side already defines the bundle envelope and ingestion contract. Spine already accepts opaque encrypted bundles at `POST /v1/sync/bundle` and handles idempotent upload. This change should make the mobile client consume those contracts without adding a new server API.

The main constraint is privacy: raw capture text and decrypted envelope bytes must not be persisted, logged, or placed in retry metadata. The durable queue stores encrypted bytes only.

## Goals / Non-Goals

**Goals:**

- Encrypt mobile capture payloads using the same AES-256-GCM wire format the desktop client decrypts.
- Persist encrypted outbox rows in `expo-sqlite` with deterministic recovery after restart.
- Upload outbox rows in FIFO order with stable idempotency keys and bounded retries.
- Pause safely while offline or unpaired, and resume from the queue head after connectivity returns.
- Reset stale `sending` rows to `pending` after app startup or foreground recovery.
- Keep unpair/device reset behavior clearing the outbox.
- Show a minimal local outbox status surface for the latest 3 captures so queued/failed uploads are visible immediately.

**Non-Goals:**

- No full recent-capture list; US-049 owns that UI and retention policy.
- No per-row retry/delete/copy UI; later US-048 work owns those controls.
- No plaintext capture preview in the outbox UI; preview storage belongs to the later Recent-list design.
- No mobile WebSocket listener.
- No Spine server endpoint changes.
- No on-device STT, OCR, embedding, or media transformation.
- No bidirectional desktop indexing ACK.

## Decisions

### 1. Queue stores encrypted blobs only

**Choice:** The outbox table stores `encrypted_blob` and status metadata. It does not store plaintext payload JSON, text previews, media bytes, or serialized capture objects.

**Rationale:** US-047's privacy requirement is stricter than ordinary offline caching. Storing encrypted bytes lets offline retry work without creating a second plaintext persistence layer.

**Alternatives considered:**

- Store plaintext then encrypt on flush: simpler retry path, but violates the "encrypt before persistence" requirement.
- Store both plaintext preview and ciphertext: useful for US-049, but preview retention belongs in a later, explicitly scoped local-cache design.

### 2. Use `expo-sqlite` as the outbox persistence boundary

**Choice:** Replace the in-memory service with a SQLite table named `outbox`:

```sql
CREATE TABLE IF NOT EXISTS outbox (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'failed', 'done')),
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  encrypted_blob BLOB NOT NULL
);
```

Add an index on `(state, created_at)` for queue scans.

**Rationale:** The PRD explicitly names `expo-sqlite`, and SQLite gives process-death recovery, ordered scans, and future status queries for US-048/US-049 without adding another storage abstraction.

**Alternatives considered:**

- `AsyncStorage`: easier to mock, but worse for ordered state transitions and row limits.
- SecureStore: inappropriate for many queue rows and large encrypted blobs.

### 3. Mobile envelope format mirrors desktop wire expectations

**Choice:** Mobile builds the same plaintext `BundleEnvelope` shape used by desktop: `schema_version = 1`, outer `kind` such as `capture-text`, `filename`, `content_utf8`, optional `source_path`, `captured_at`, and `sha256`. For `capture-text`, `content_utf8` is an inner JSON body with `id`, `text`, `source`, and `client_ts`. `sha256` is lower-hex SHA-256 of `content_utf8.as_bytes()`, matching `apps/desktop/src-tauri/src/spine/bundle.rs`. The envelope serializes through `secureSerialize()`, encrypts with AES-256-GCM using `syncKey`, uses a fresh 96-bit random nonce, and stores `nonce | ciphertext_and_tag`.

AAD is the peer fingerprint's SHA-256 bytes decoded from `PersistedPairingState.pairedPeerFingerprint` (`sha256:<hex>`). This matches the desktop rule where inbound decrypt uses AAD derived from the receiver's own public-key fingerprint.

**Rationale:** The desktop ingestion path already expects this wire shape. Mobile should not define a separate envelope dialect for capture text.

**Alternatives considered:**

- Reuse desktop `kind: "note"`: compatible with old ingestion, but loses capture-type routing already specified in PRD 005 US-053.
- Add a mobile-only envelope version: more flexible, but forces desktop parser changes for no current benefit.

### 4. Add a small mobile bundle crypto module

**Choice:** Implement encryption in a focused mobile module, likely `apps/mobile/src/outbox/bundle.ts` or `apps/mobile/src/crypto/bundle.ts`, using existing `@noble/hashes` for SHA-256 and `@noble/ciphers` for AES-256-GCM.

**Rationale:** `expo-crypto` covers hashing/randomness but not a complete AES-GCM API for this use case. Keeping bundle crypto isolated makes it easier to test against desktop fixtures and avoids spreading key material handling through UI code.

**Alternatives considered:**

- Native module for AES-GCM: stronger platform integration, but expands native surface area before the mobile app needs it.
- WebCrypto: not consistently available across Expo native targets in the current codebase.
- `react-native-quick-crypto`: broad Node-compatible crypto surface, but heavier than needed for this narrow AES-GCM path.

### 5. `secureSerialize()` is the only plaintext serialization path

**Choice:** Capture enqueue code calls `secureSerialize(payload)` to produce UTF-8 bytes. The helper does not log payloads and can set a short-lived development sentinel that test code uses to reject direct payload `JSON.stringify()` in the outbox path.

**Rationale:** JavaScript cannot force immediate object memory zeroization, but it can prevent accidental plaintext persistence/logging and centralize the only allowed plaintext-to-bytes transition.

**Alternatives considered:**

- Rely on lint-only checks: useful but too weak for a security-sensitive path.
- Deep-freeze payloads after serialization: does not solve logging and adds noise to capture construction.

### 6. Flush is single-flight and FIFO

**Choice:** `flushOutbox()` obtains a module-level single-flight guard, resets stale `sending` rows before scanning, then processes rows ordered by `created_at ASC`. It marks one row `sending`, uploads it, and transitions it to `done`, `pending`, or `failed`. The 1000-row queue cap applies only to unfinished rows (`pending`, `sending`, `failed`), not `done` rows.

**Rationale:** Single-flight avoids double uploads from foreground send, app-start flush, and background fetch firing together. FIFO keeps user captures ordered and makes retry state easier to reason about.

**Alternatives considered:**

- Parallel uploads: faster for large backlogs, but ordering and retry idempotency become more complex.
- Keep failed rows at the head forever: preserves strict ordering but can starve later captures. This change marks exhausted rows `failed` and continues to later `pending` rows; US-048 can expose retry.

### 7. Retry policy follows US-047 rather than desktop's five-attempt policy

**Choice:** Mobile retries `429` and `5xx` with 1s, 4s, and 16s delays, using the same `Idempotency-Key` value equal to the outbox row id. Non-429 `4xx` responses are non-retryable.

**Rationale:** US-047 explicitly calls for three attempts. This differs from the desktop client's five-attempt policy but stays inside the same server idempotency contract.

### 8. Background fetch is opportunistic only

**Choice:** Register Expo task-manager/background-fetch hooks to call `flushOutbox()`, but the deterministic triggers are app startup, foreground resume, manual refresh hooks, and immediate send.

**Rationale:** iOS and Android schedule background work opportunistically. The app must never present background upload as guaranteed within seconds.

### 9. Pairing loss pauses upload without queue deletion

**Choice:** If `authenticatedFetch()` throws `UnpairedError` or pairing state is missing, `flushOutbox()` stops and leaves rows encrypted in `pending`/`failed` state unless the explicit unpair/device-reset path calls `clearOutbox()`.

**Rationale:** Automatic stale-pairing detection should not destroy queued captures by itself. Explicit user unpair still clears the queue per US-042.

### 10. Done-row retention is minimal in US-047

**Choice:** US-047 adds fixed cleanup for `done` rows older than 7 days and excludes `done` rows from the queue cap. US-049 can later replace the fixed retention with the configurable 7/30/90/permanent policy.

**Rationale:** Keeping recent `done` rows supports US-048/US-049 status surfaces, but letting them count toward the offline queue cap or grow forever would block capture send after enough successful uploads.

### 11. Capture screen shows minimal outbox status now

**Choice:** The capture screen queries the latest 3 outbox rows ordered by `created_at DESC` and displays only local metadata: state, attempts, and whitelisted `last_error`. The screen refreshes after enqueue/flush transitions through an in-process event listener and also polls every 10 seconds while paired.

**Rationale:** US-047 creates durable offline state; hiding that state makes offline capture look broken. A small metadata-only status list gives users immediate feedback without pulling the full US-048 retry/delete/copy controls or US-049 Recent tab into this change.

**Alternatives considered:**

- Wait for US-048: keeps this change narrower, but ships a queue whose failures and pending rows are invisible.
- Store plaintext previews now: more useful UI, but violates this change's encrypted-only persistence boundary and belongs in the later local-cache/Recent design.
- Poll only: simpler, but state changes after foreground send would feel stale for up to 10 seconds.

## Risks / Trade-offs

- **[Risk] AES-GCM library mismatch with Rust desktop fixtures** -> Add cross-platform deterministic fixtures with fixed key, nonce, AAD, plaintext, ciphertext, and tamper cases before implementation proceeds.
- **[Risk] SQLite blob support differs across Expo runtimes** -> Add a narrow smoke test for insert/read/delete of binary blobs before building the full queue.
- **[Risk] Sending rows can get stuck if the process dies mid-upload** -> Reset all `sending` rows to `pending` on app startup and foreground recovery.
- **[Risk] Background task APIs vary by platform and Expo SDK version** -> Keep background registration thin and non-critical; foreground flush remains sufficient for correctness.
- **[Risk] Queue cap rejects captures while offline** -> Enforce the 1000-row cap before encryption/upload and surface a short error to the capture screen.
- **[Risk] Plaintext appears in tests or logs** -> Test only stable hashes, ids, and ciphertext; do not snapshot plaintext payloads outside the secure serialization unit tests.

## Migration Plan

1. Add mobile dependencies and run a narrow smoke test for SQLite plus AES-GCM.
2. Create the SQLite `outbox` table lazily on first outbox call.
3. Existing in-memory outbox rows are not migrated; the current stub has no process-durable data.
4. On app startup, initialize the outbox and reset `sending` rows to `pending`.
5. If the change must roll back, remove the new dependencies and restore the memory-only service; no persisted plaintext cleanup is needed because the new queue stores encrypted blobs only.

## Open Questions

- The exact inner `content_utf8` payload fields for audio/image/link will be refined in later capture-kind implementation tasks; this change should at minimum support `capture-text` from the current screen and keep the shared encryption/outbox path kind-agnostic.
