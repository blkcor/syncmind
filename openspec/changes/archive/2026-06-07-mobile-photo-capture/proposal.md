## Why

PRD 005 US-045 is the next unchecked mobile capture story after text and voice capture. Users need a fast camera/library image capture path that keeps raw image content private until encrypted, then lets the paired desktop store the JPEG and run OCR for indexing.

## What Changes

- Add a camera toolbar action on the paired Capture screen that opens an ActionSheet with "Take Photo" and "Pick from Library".
- Use `expo-image-picker` for camera and photo-library selection with permission requests made only when the user chooses the corresponding source.
- Add image preprocessing that normalizes camera/library assets to JPEG, resizes any long edge above 2048 px, targets quality 85, retries quality 70 when needed, and rejects images still above the 5 MB encoded hard cap.
- Support an optional caption captured with the image in the same `capture-image` bundle.
- Encode JPEG bytes into the existing encrypted bundle envelope as `kind: "capture-image"` before durable queueing.
- Persist queued image captures through the existing encrypted SQLite outbox and upload them through `POST /v1/sync/bundle`.
- Add narrow desktop `capture-image` caption materialization so captions are not silently dropped before OCR/indexing.
- Keep broader desktop OCR implementation and Spine relay internals out of scope; current desktop `capture-image` OCR compatibility must be verified before acceptance.

## Capabilities

### New Capabilities

- `mobile-photo-capture`: Camera/library image selection, image preprocessing, optional caption entry, `capture-image` payload construction, privacy constraints, limits, and paired Capture screen integration for US-045.

### Modified Capabilities

- `mobile-capture-outbox-upload`: Kind-aware encrypted bundle construction and upload metadata for `capture-image` rows in the existing outbox.
- `ocr-text-extraction`: Desktop `capture-image` dispatch must retain optional captions in generated markdown before and after OCR post-processing.

## Impact

- Affected mobile code: `apps/mobile/app/(tabs)/index.tsx`, a new or existing `apps/mobile/src/capture/` image helper, `apps/mobile/src/crypto/bundle.ts`, `apps/mobile/src/outbox/service.ts`, mobile tests, `apps/mobile/package.json`, and `apps/mobile/app.json`.
- Affected desktop code: `apps/desktop/src-tauri/src/spine/dispatch.rs` and desktop dispatch tests for caption retention.
- Adds Expo SDK 56-compatible image picking/manipulation dependencies as needed after versioned docs are checked.
- Reuses existing pairing state, AES-GCM bundle encryption, SQLite outbox persistence, and authenticated Spine upload.
- No server endpoint changes are expected; Spine already relays opaque sync bundles for arbitrary `X-Syncmind-Content-Type` values, so this change does not modify the relay contract.
