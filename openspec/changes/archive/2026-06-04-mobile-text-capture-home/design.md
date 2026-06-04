## Context

PRD 005 US-043 defines the paired mobile app's first screen as a text capture surface. The current Expo SDK 56 app already routes the first tab to `apps/mobile/app/(tabs)/index.tsx`, restores pairing in `app/_layout.tsx`, initializes the encrypted outbox, and sends text through `createCaptureTextPayload()`, `encryptCaptureText()`, `enqueueOutboxItem()`, and `flushOutbox()`.

That means this change is not a new queue or upload feature. It closes the gap between the existing implementation and US-043: autofocus, clear status semantics, the 50,000-character limit, and the exact plaintext payload schema before encryption.

## Goals / Non-Goals

**Goals:**

- Keep `index` as the default paired capture tab and make its paired state a focused text capture home screen.
- Enforce US-043 text input rules before encryption or enqueue.
- Map existing pairing/outbox state into a thin top status row: connected, queued/offline, or pairing invalid.
- Show the latest 3 local outbox rows as the capture mini preview using metadata already exposed by `getRecentOutboxStatuses()`.
- Update `CaptureTextPayload` so encrypted bundles contain the US-043 plaintext schema.
- Keep optimistic clearing and best-effort flushing after successful encrypted enqueue.

**Non-Goals:**

- No implementation of US-044 audio recording, STT, waveform UI, or audio payloads.
- No full capture list or Recent tab; US-049 owns full browsing and retention controls.
- No plaintext outbox persistence beyond the existing bounded `preview_text` metadata already introduced for the mini preview.
- No Spine API changes.
- No new global navigation structure.

## Decisions

### 1. Refine the existing `index` tab instead of adding a new screen

`apps/mobile/app/(tabs)/index.tsx` is already the first tab and already branches between unpaired pairing scanner and paired capture UI. This change SHALL keep that route and make the paired branch comply with US-043.

Alternative considered: create a separate `CaptureScreen` route and redirect after pairing restore. That adds routing state without changing the user's entry point.

### 2. Treat the 50,000-character limit as a pre-encryption UI gate

The screen SHALL compute `note.length > 50_000` and disable Send before creating a payload. The visible message is `Too long - try splitting`. Empty or whitespace-only input remains a local no-op.

Alternative considered: reject only in `createCaptureTextPayload()`. The crypto helper should still be testable for schema, but users need immediate feedback while typing and the app should avoid constructing oversized plaintext payloads.

### 3. Use existing app store and outbox metadata for the status row

The status row SHALL derive peer validity from `useAppStore().connectionStatus` and queue/offline state from recent outbox rows. A connected state maps to green, queued/offline to gray, and invalid pairing/error to red. The row can show a shortened peer fingerprint and compact queue state, but it MUST NOT decrypt outbox rows.

Alternative considered: add a new network monitor dependency. Current pairing health checks and outbox state are sufficient for US-043, and Expo network semantics can be handled later if needed.

### 4. Keep the mini preview metadata-only

The latest-3 preview SHALL use `getRecentOutboxStatuses(3)` and `subscribeToOutboxChanges()` as the current screen already does. Rows display `preview_text`, relative time, state, and whitelisted error metadata where available. The screen does not read `encrypted_blob` or derive content from ciphertext.

Alternative considered: decrypt the latest local captures for richer previews. That would broaden key handling into UI code and conflicts with the encrypted outbox boundary.

### 5. Update the inner plaintext payload schema before encryption

`CaptureTextPayload` SHALL become:

```ts
{
  v: 1;
  kind: "capture-text";
  id: string;
  text: string;
  source: "typed";
  client_ts: string;
  client_device_fingerprint: string;
}
```

The screen SHALL use `Crypto.randomUUID()` for `id` and the existing native identity cache, via `ensureIdentity()` or `getDeviceFingerprint()`, for `client_device_fingerprint`. `buildCaptureTextEnvelope()` continues to wrap this inner JSON in a `BundleEnvelope` with outer `kind: "capture-text"` before AES-GCM encryption.

Alternative considered: keep `source: "mobile"` and infer typed capture from the bundle kind. US-043 explicitly defines `source: "typed"`, and later capture modes need a stable source discriminator.

### 6. Keyboard gestures stay minimal

The capture surface SHALL support dismissing the keyboard by tapping or dragging outside the input. The up-swipe voice transition can be represented by a no-op/placeholder gesture boundary if the implementation needs one for testability, but it MUST NOT start audio recording in this change.

Alternative considered: implement the full upward voice mode switch now. That belongs to US-044 and would add permissions, media lifecycle, and payload-size handling outside this change.

## Risks / Trade-offs

- [Risk] `autoFocus` can be flaky during pairing restoration on native devices -> Mitigation: render paired capture only after root pairing restoration and keep a ref-based focus fallback in `useEffect` if needed.
- [Risk] status color semantics can drift from Settings -> Mitigation: reuse the existing `connectionStatus` values and keep mapping tests close to `capture-screen.test.ts`.
- [Risk] changing `CaptureTextPayload` breaks existing bundle tests -> Mitigation: update fixtures and assert the full inner JSON schema before encryption.
- [Risk] storing `preview_text` conflicts with strict plaintext minimization -> Mitigation: keep preview bounded at the existing 100-character limit and do not expand persistent plaintext in this change.

## Migration Plan

1. Update tests for US-043 screen behavior and capture payload schema.
2. Update `CaptureTextPayload` construction and bundle tests.
3. Refine the paired capture UI in `index.tsx`.
4. Run the mobile unit tests and typecheck.

Rollback is limited to restoring the old payload type and paired screen layout. No persisted encrypted rows require migration because the upload envelope remains `capture-text` and Spine treats bundle contents as opaque ciphertext.

## Open Questions

- Whether `preview_text` should remain available after US-049 introduces the full Recent list should be decided in the US-049 spec, not here.
