## Context

PRD 005 US-044 follows the completed US-043 text capture home and US-047 encrypted outbox upload work. The current mobile app is an Expo SDK 56 app with a paired Capture tab, AES-GCM bundle encryption, SQLite-backed encrypted outbox rows, and authenticated `POST /v1/sync/bundle` upload.

The current screen intentionally excludes audio recording. Spine stores sync bundles opaquely and preserves the `X-Syncmind-Content-Type` header, so US-044 should not introduce a new server API. Desktop handling for decrypted `capture-audio` envelopes is an integration dependency: the mobile implementation must verify that the paired desktop can accept the payload shape, and any missing or incompatible desktop dispatch work must be handled before US-044 is accepted.

US-044 names `expo-av`, while `apps/mobile/AGENTS.md` requires checking the exact Expo SDK 56 docs before code work and the app pins Expo `~56.0.5`. The package/API choice is not settled by this design. Implementation must verify the versioned SDK 56 recording API first, then use the supported package for `Audio.Recording`-equivalent behavior.

## Goals / Non-Goals

**Goals:**

- Add a voice mode on the existing paired Capture screen without changing the pair-first unpaired state.
- Define voice-mode entry as an upward swipe from the capture composer area, plus an explicit accessible microphone mode-toggle control.
- Record `.m4a` audio with AAC LC, 16 kHz, mono, and 32 kbps when the platform honors those encoder settings.
- Request microphone permission at first recording use and provide a system-settings path when denied.
- Show a stable press-and-hold recording control with metering-driven waveform feedback and an accessible double-tap-to-toggle recording fallback.
- Enforce a 60-second maximum duration and the 8 MB raw / 11 MB base64 size cap before encrypted enqueue.
- Build `capture-audio` payloads with `audio_base64`, `audio_mime: "audio/mp4"`, `duration_ms`, `client_ts`, and `client_device_fingerprint`.
- Reuse the existing bundle envelope, AES-GCM encryption, SQLite outbox, and `POST /v1/sync/bundle` upload path.
- Preserve a partial recording on interruption and ask the user to keep or discard it.

**Non-Goals:**

- No on-device transcription, embeddings, waveform persistence, or audio search.
- No changes to Spine routing or database schema.
- No desktop STT implementation changes unless compatibility verification proves the current desktop tree cannot accept `capture-audio` bundles.
- No image capture, share extension, full Recent tab, or retry/delete queue UI.
- No durable plaintext audio storage.

## Decisions

### 1. Resolve the SDK 56 recording API before implementation

The mobile app SHALL use the audio recording package/API supported by the exact Expo SDK 56 docs. The first implementation task is to verify whether SDK 56 expects `expo-av` `Audio.Recording`, a standalone `expo-audio` API, or another package naming pattern. The spec requires the behavior, not a preselected package.

Alternative considered: hard-code `expo-audio` or `expo-av` in the design. That would turn a versioned-docs check into an assumption and could make the implementation start with the wrong dependency.

### 2. Keep audio capture inside the existing Capture route

The existing `apps/mobile/app/(tabs)/index.tsx` route remains the capture home. The primary gesture is an upward swipe from the lower capture composer/action area with at least 48 px of upward drag before release. The same mode change must also be available through a microphone mode-toggle control with an accessibility label so users who cannot perform the gesture can reach voice mode. The unpaired branch continues to render pairing controls only.

Alternative considered: add a separate audio route. That creates navigation state without helping the one-handed capture workflow US-044 describes.

### 3. Treat press-and-hold as primary, not exclusive

Voice mode has explicit states: idle, requesting permission, recording, stopping, review-after-interrupt, enqueuing, and error. Press-in starts recording after permission is granted. Press-out stops and sends. For accessibility, the same circular control must expose a double-tap-to-toggle path: activate once to start recording and activate again to stop/send. Timeout, app interruption, and permission denial are handled as explicit transitions rather than incidental UI errors.

Alternative considered: tap once to start and tap again to stop. That is easier to implement, but it does not match the fast "hold to capture, release to send" behavior.

### 4. Encrypt before durable queueing and delete recorder files promptly

The recorder will produce a native temporary file. The app reads that file to base64 only after stop, validates duration and size, builds the `capture-audio` JSON payload, wraps it in a `BundleEnvelope`, encrypts it, and enqueues only encrypted bytes. After successful enqueue, failed validation, discard, or send cancellation, the recorder temp file is deleted best-effort.

Alternative considered: store the raw `.m4a` in a durable local media directory and enqueue a pointer. That would create plaintext media persistence and weaken the existing outbox privacy boundary.

### 5. Extend bundle helpers to be kind-aware

`apps/mobile/src/crypto/bundle.ts` currently focuses on `capture-text`. This change should add shared envelope construction for capture payload kinds and a specific `createCaptureAudioPayload()` / `encryptCaptureAudio()` path. The existing `secureSerialize()` guard remains the only payload-to-UTF-8 path.

Alternative considered: duplicate the text encryption path in UI code. That would spread key handling, hashing, and serialization rules into the screen.

### 6. Store upload content type per outbox row

`apps/mobile/src/outbox/service.ts` currently uploads every row with `application/syncmind.capture-text+json`. Audio rows need `application/syncmind.capture-audio+json`. Add a content-type column or equivalent migration-safe metadata so each queued row uploads with the content type matching its encrypted bundle kind. Existing rows default to the text content type.

Alternative considered: infer content type from row id or decrypt before upload. Inference is brittle, and decrypting in the upload path violates the outbox boundary.

### 7. Keep Spine unchanged and verify desktop compatibility

Spine already accepts arbitrary `X-Syncmind-Content-Type` strings, stores encrypted bytes, and publishes bundle notifications. The spec delta only makes `application/syncmind.capture-audio+json` an expected mobile capture content type. Desktop handling remains a separate acceptance dependency: if this checkout lacks working `capture-audio` dispatch, implementation must add a separate desktop change or explicitly block US-044 acceptance until that work exists.

Alternative considered: use `/v1/media/upload`. That endpoint belongs to older media-ingestion work and would bypass the already integrated mobile encrypted outbox path.

## Risks / Trade-offs

- [Risk] The wrong Expo audio package could be selected -> Mitigation: make versioned-doc verification task 1.1 and do not code against a package until the SDK 56 API is confirmed.
- [Risk] Expo encoder options may be approximated differently on iOS and Android -> Mitigation: specify the target settings, assert the configured options in tests, and treat platform-specific output variance as acceptable if the file is `.m4a` AAC and remains desktop-decodable.
- [Risk] Metering may be unavailable on one platform or in test runtime -> Mitigation: keep waveform rendering driven by optional metering values with a deterministic idle fallback.
- [Risk] Base64 audio increases payload size by about one third -> Mitigation: enforce both raw and base64 caps before enqueue and surface a short "clip too long" message.
- [Risk] App backgrounding or calls can interrupt recording -> Mitigation: subscribe to app state changes, stop recording, retain the partial file temporarily, and require an explicit keep/discard choice.
- [Risk] Outbox migration can break existing text rows -> Mitigation: add the content-type metadata with a default text value and keep upload tests for pre-existing rows.
- [Risk] Desktop capture-audio dispatch may be missing or incompatible -> Mitigation: verify desktop dispatch before acceptance and either add a dependency change or stop before claiming US-044 is complete.
- [Risk] Raw temp-file deletion can fail -> Mitigation: delete best-effort after all terminal paths and never insert raw file paths or bytes into SQLite metadata.

## Migration Plan

1. Confirm the SDK 56 audio recording and file-system APIs, then add only the needed mobile dependencies.
2. Add the outbox content-type metadata migration with a default of `application/syncmind.capture-text+json`.
3. Add kind-aware bundle helpers and audio payload construction.
4. Add voice mode UI and recording lifecycle on the existing Capture screen.
5. Add tests for voice-mode entry, accessible mode toggle, permission denial, press-and-hold transitions, double-tap toggle recording, max duration, size caps, interruption keep/discard, content-type upload, and encrypted-only queue persistence.
6. Verify desktop `capture-audio` compatibility or open a separate desktop dependency before acceptance.

Rollback is limited to removing the audio UI and new dependencies. Existing text outbox rows remain compatible because their content type defaults to the current text bundle value.

## Open Questions

- Whether the final implementation should show the "keep / discard" interruption prompt as a native alert or as an in-screen review panel can be decided during UI implementation, as long as it is testable and blocks silent upload.
- The exact Expo SDK 56 recording package/API name must be resolved during task 1.1 from versioned Expo docs before code changes.
