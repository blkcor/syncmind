## Why

PRD 005 US-043 makes text capture the mobile app's primary workflow: opening the app while paired should land on a focused text input, accept a typed capture, and hand it to the encrypted outbox immediately. The current mobile screen already enqueues encrypted text captures, but the codebase does not yet have an OpenSpec contract for the stricter US-043 home-screen behavior, character limit, connection state, or plaintext payload shape.

## What Changes

- Make the paired default mobile route a text capture home screen with:
  - an auto-focused multiline `TextInput`
  - a thin peer/outbox status row using green, gray, and red connection states
  - a prominent Send action
  - the latest 3 local capture statuses as the mini preview surface
- Enforce the text envelope limit before encryption:
  - reject input above 50,000 characters
  - disable Send and show `Too long - try splitting`
  - keep empty/whitespace sends local no-ops
- Align the `capture-text` plaintext payload with US-043 before encryption:
  - include `v: 1`, `kind: "capture-text"`, `id`, `text`, `source: "typed"`, `client_ts`, and `client_device_fingerprint`
  - continue encrypting and persisting only the bundle blob through the existing outbox path
- Preserve the existing optimistic send behavior:
  - clear the input immediately after successful encrypted enqueue
  - trigger best-effort `flushOutbox()` without blocking the UI on network success
- Keep voice-mode gesture handling limited to a placeholder transition:
  - support dismissing the keyboard from the capture surface
  - do not implement US-044 audio capture in this change

## Capabilities

### New Capabilities

- `mobile-text-capture-home`: Paired mobile text capture home screen behavior, input limits, connection status row, send ergonomics, and latest-3 local capture preview.

### Modified Capabilities

- `mobile-capture-outbox-upload`: Tighten the `capture-text` plaintext payload schema used before encryption so the queued encrypted bundle carries US-043 fields, including `v`, `kind`, `source: "typed"`, and `client_device_fingerprint`.

## Impact

- `apps/mobile/app/(tabs)/index.tsx` - refine the paired capture screen layout, autofocus, character-limit state, connection/status row, keyboard dismissal, send gating, and latest-3 status presentation.
- `apps/mobile/src/crypto/bundle.ts` - update `CaptureTextPayload` and envelope creation to match the US-043 plaintext schema before encryption.
- `apps/mobile/src/crypto/identity.ts` or `apps/mobile/src/spine/session.ts` - source the local device fingerprint needed by `client_device_fingerprint`.
- `apps/mobile/src/store.ts` - reuse or extend the existing paired/connection status state for the top status row.
- `apps/mobile/__tests__/capture-screen.test.ts` and bundle/outbox tests - cover autofocus, limit handling, optimistic clearing, status-row mapping, latest-3 metadata display, and payload schema.
- No Spine endpoint shape change is expected; uploads continue to use the existing encrypted `POST /v1/sync/bundle` flow.
