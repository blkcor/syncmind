## Why

PRD 005 US-044 is the next mobile capture increment after text capture and encrypted outbox upload. Users need a fast press-and-hold voice capture path that keeps raw audio private on-device until encrypted, then lets the paired desktop transcribe and index it.

## What Changes

- Add a paired Capture screen voice mode reached from the existing text capture surface by a defined upward swipe, with an explicit accessible mode-toggle fallback.
- Add press-and-hold recording with the Expo SDK 56-supported audio recording API, microphone permission handling, metering-driven waveform feedback, an accessible toggle recording path, and a 60-second maximum recording duration.
- Encode recorded `.m4a` bytes into the existing encrypted bundle envelope as `kind: "capture-audio"` with the US-044 payload schema before encryption.
- Persist queued audio captures through the existing encrypted SQLite outbox and upload them through `POST /v1/sync/bundle`.
- Add local size validation for the US-044 hard cap: 8 MB raw audio bytes / 11 MB base64 payload.
- Handle recording interruption by preserving the partial segment and asking the user whether to keep or discard it.
- Keep desktop transcription implementation, Spine relay internals, image capture, share capture, and recent-capture management out of scope, while requiring desktop `capture-audio` compatibility to be verified before mobile implementation is accepted.

## Capabilities

### New Capabilities

- `mobile-audio-capture`: Voice-mode capture screen behavior, recording lifecycle, audio payload construction, privacy constraints, limits, and interruption handling for US-044.

### Modified Capabilities

- `mobile-capture-outbox-upload`: Kind-aware encrypted bundle construction and upload metadata for `capture-audio` rows in the existing outbox.
- `sync-bundle-relay`: Mobile capture uploads may use `X-Syncmind-Content-Type: application/syncmind.capture-audio+json` for encrypted `capture-audio` bundles while remaining opaque to Spine.

## Impact

- Affected mobile code: `apps/mobile/app/(tabs)/index.tsx`, `apps/mobile/src/crypto/bundle.ts`, `apps/mobile/src/outbox/service.ts`, mobile tests, and mobile package dependencies.
- Adds whichever Expo SDK 56-supported recording package/API is confirmed by versioned docs; the app currently has no audio dependency.
- Reuses existing pairing state, AES-GCM bundle encryption, SQLite outbox persistence, and authenticated Spine upload.
- No server endpoint changes are expected; Spine already relays opaque sync bundles. Desktop `capture-audio` dispatch is treated as a compatibility dependency to verify, not as part of this mobile-only implementation scope.
