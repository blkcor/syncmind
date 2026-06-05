## 1. Setup And Compatibility

- [x] 1.1 Re-check Expo SDK 56 audio and file-system docs before coding, identify the supported recording package/API, and record any API naming differences in the implementation notes.
- [x] 1.2 Add the verified SDK 56 audio recording dependency to `apps/mobile/package.json`.
- [x] 1.3 Add file-system support if needed for reading recorded `.m4a` bytes and deleting temp files.
- [x] 1.4 Update Jest mocks/setup for the verified Expo audio and file-system APIs.

## 2. Outbox Upload Metadata

- [x] 2.1 Add a failing mobile outbox test for `capture-audio` rows uploading with `X-Syncmind-Content-Type: application/syncmind.capture-audio+json`.
- [x] 2.2 Add migration-safe outbox content-type metadata with default `application/syncmind.capture-text+json` for existing rows.
- [x] 2.3 Update enqueue and flush APIs to carry per-row sync-bundle content type without decrypting queued blobs.
- [x] 2.4 Keep existing text upload tests passing for rows without explicit content-type metadata.

## 3. Capture-Audio Bundle Crypto

- [x] 3.1 Add failing bundle tests for the US-044 `capture-audio` plaintext schema.
- [x] 3.2 Add failing bundle tests for `capture-audio` envelope kind, content hash, AES-GCM wire shape, and secure serialization guard.
- [x] 3.3 Implement `createCaptureAudioPayload()` and `encryptCaptureAudio()` using the existing v1 bundle envelope and pairing keys.
- [x] 3.4 Ensure tests do not persist or snapshot raw `audio_base64` outside targeted serialization checks.

## 4. Recording Service

- [x] 4.1 Add a focused mobile recording helper or hook for permission, recorder lifecycle, metering, max-duration timeout, temp-file read, and cleanup.
- [x] 4.2 Configure `.m4a` AAC LC recording with target 16000 Hz, mono, and 32000 bps options where supported.
- [x] 4.3 Enforce 60-second stop behavior and local 8 MB raw / 11 MB base64 caps before encryption.
- [x] 4.4 Delete recorder temp files best-effort after enqueue, validation rejection, discard, and cancellation paths.
- [x] 4.5 Add tests for permission denial, timeout stop, oversize rejection, and cleanup calls.

## 5. Capture Screen Voice Mode

- [x] 5.1 Add failing Capture screen tests for entering voice mode by at least 48 px of upward swipe from the lower capture composer/action area while preserving draft text.
- [x] 5.2 Add failing Capture screen tests for entering voice mode through an accessibility-labelled microphone mode-toggle control.
- [x] 5.3 Add press-and-hold UI state tests for idle, recording, stopping/enqueuing, permission denied, and error states.
- [x] 5.4 Add accessibility action tests for double-tap-to-toggle recording start and stop/send.
- [x] 5.5 Render the circular recording control and metering-driven waveform without changing the unpaired pairing flow.
- [x] 5.6 Wire press-in to start recording and press-out to stop, encrypt, enqueue, and trigger best-effort `flushOutbox()`.
- [x] 5.7 Wire the accessible toggle action to start recording and stop/send without requiring sustained press.
- [x] 5.8 Show local user feedback for max duration and `Clip too long` rejection.

## 6. Interruption Handling

- [x] 6.1 Add tests for app background longer than 30 seconds stopping recording and preserving a partial segment.
- [x] 6.2 Add tests for keep/discard choices after interruption.
- [x] 6.3 Implement app-state/interruption handling so keep validates/enqueues and discard deletes without creating a payload.

## 7. Relay And Desktop Compatibility

- [x] 7.1 Add or update Spine relay tests proving `application/syncmind.capture-audio+json` is stored and returned opaquely through `/v1/sync/bundle`.
- [x] 7.2 Verify whether desktop `capture-audio` dispatch exists in the current checkout and accepts the mobile payload schema.
- [x] 7.3 If desktop dispatch is missing or incompatible, stop before claiming US-044 complete and create or link a desktop dependency change.
- [x] 7.4 Keep `/v1/media/upload` out of the mobile audio capture path.

## 8. Verification

- [x] 8.1 Run `openspec validate mobile-audio-capture --strict`.
- [x] 8.2 Run mobile tests relevant to bundle crypto, outbox upload, and capture screen behavior.
- [x] 8.3 Run mobile typecheck.
- [x] 8.4 Run sync-gateway tests if relay tests were touched.
- [x] 8.5 Run desktop Rust tests for spine dispatch if desktop compatibility code was touched.
- [x] 8.6 Update PRD 005 US-044 status after implementation is complete and accepted.
