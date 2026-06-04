## 1. Test Baseline

- [x] 1.1 Update bundle crypto tests to expect the US-043 `capture-text` inner payload schema with `v`, `kind`, `source: "typed"`, and `client_device_fingerprint`.
- [x] 1.2 Add capture screen tests for paired launch rendering a multiline auto-focused input on the existing first tab.
- [x] 1.3 Add capture screen tests for empty text no-op, 50,000-character limit rejection, and `Too long - try splitting` copy.
- [x] 1.4 Add capture screen tests for status-row mapping from `connectionStatus` to connected, queued/offline, and pairing-invalid states.
- [x] 1.5 Add capture screen tests proving the latest-3 preview uses outbox status metadata and does not read `encrypted_blob`.

## 2. Payload Schema

- [x] 2.1 Update `CaptureTextPayload` in `apps/mobile/src/crypto/bundle.ts` to include `v: 1`, `kind: "capture-text"`, `source: "typed"`, and `client_device_fingerprint`.
- [x] 2.2 Update `createCaptureTextPayload()` callers and tests to pass the local mobile device fingerprint.
- [x] 2.3 Ensure `buildCaptureTextEnvelope()` still wraps the inner payload in outer `BundleEnvelope.kind = "capture-text"` and preserves secure serialization guardrails.

## 3. Capture Home UI

- [x] 3.1 Refine `apps/mobile/app/(tabs)/index.tsx` so the paired branch renders an auto-focused multiline text input.
- [x] 3.2 Add a top status row that uses existing app store peer/connection state and outbox metadata without decrypting queued rows.
- [x] 3.3 Enforce send eligibility in the screen: non-whitespace text, `note.length <= 50_000`, disabled Send while invalid, and visible limit copy when too long.
- [x] 3.4 Preserve optimistic clearing only after encrypted enqueue succeeds, followed by best-effort `flushOutbox()`.
- [x] 3.5 Keep keyboard dismissal behavior on the capture surface, including drag dismissal, while preserving draft text.
- [x] 3.6 Keep voice-mode gesture/audio recording out of this change; do not request microphone permission.

## 4. Recent Preview

- [x] 4.1 Reuse `getRecentOutboxStatuses(3)` and `subscribeToOutboxChanges()` for the mini preview.
- [x] 4.2 Render up to 3 rows ordered by latest local outbox status with preview, relative time, state, attempts, or whitelisted error metadata.
- [x] 4.3 Ensure preview rendering does not query or pass `encrypted_blob` into the Capture screen.

## 5. Verification

- [x] 5.1 Run `pnpm --filter mobile test --runInBand`.
- [x] 5.2 Run `pnpm --filter mobile typecheck`.
- [x] 5.3 Run `pnpm --filter mobile lint`.
- [x] 5.4 Run `openspec validate mobile-text-capture-home`.
