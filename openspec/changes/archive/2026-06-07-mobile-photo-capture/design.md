## Context

PRD 005 US-045 follows completed text capture, encrypted outbox upload, and voice capture work. The current mobile app already has a paired Capture tab, AES-GCM bundle encryption, SQLite-backed encrypted outbox rows with per-row content type, authenticated `POST /v1/sync/bundle` upload, and desktop-side `capture-image` dispatch plus OCR specs in the checkout.

The current Capture screen has text and voice modes but no photo entry point. `apps/mobile/package.json` does not yet include `expo-image-picker`, and image resizing/re-encoding support must be confirmed against Expo SDK 56 before implementation. The current desktop `CaptureImagePayload` also lacks a `caption` field and its markdown templates do not materialize caption text, so mobile captions would be silently ignored unless the desktop dispatcher is updated in the same change.

The implementation should keep mobile as a sensor: no OCR, no embeddings, and no durable plaintext media beyond native picker/manipulation temp files that are deleted best-effort after encryption or rejection.

## Goals / Non-Goals

**Goals:**

- Add a camera icon action on the existing paired Capture screen without changing the pair-first unpaired state.
- Let users choose "Take Photo" or "Pick from Library" through an ActionSheet-style source picker.
- Request camera and media-library permissions only when the chosen source requires them.
- Normalize selected images to JPEG with long edge <= 2048 px, quality 85 by default, and quality 70 retry when needed.
- Preserve available EXIF metadata when the selected Expo SDK 56 preprocessing path supports it.
- Reject images still larger than 5 MB after preprocessing and retry.
- Verify serialized `capture-image` payload/envelope size remains under the existing bundle-size guard after base64 and JSON overhead.
- Allow an optional caption to travel in the same encrypted `capture-image` payload.
- Preserve optional captions in desktop-generated image markdown before and after OCR post-processing.
- Reuse the existing bundle envelope, AES-GCM encryption, SQLite outbox, and `POST /v1/sync/bundle` upload path.
- Verify current desktop `capture-image` compatibility before acceptance.

**Non-Goals:**

- No on-device OCR, embedding, visual search, or image classification.
- No `/v1/media/upload` path and no new Spine endpoint.
- No full Recent tab image thumbnail retention; that belongs to US-049.
- No Share Extension or Android share target; those reuse this preprocessing pipeline later in US-046.
- No editing tools beyond resize/re-encode/caption needed for capture.
- No durable local original-image storage after enqueue or rejection.
- No broad desktop OCR redesign beyond preserving caption text while retaining the current OCR workflow.

## Decisions

### 1. Resolve Expo SDK 56 image APIs before implementation

The mobile app SHALL use the image picking and image manipulation APIs supported by the exact Expo SDK 56 docs. `expo-image-picker` is required by PRD 005 for camera and library access. If SDK 56 requires a separate manipulation package for resize/re-encode, implementation must add it explicitly and record the API choice in implementation notes. Native permission copy must be checked at the same time because the app already uses `expo-camera` for QR pairing; the final camera permission message must cover both QR scanning and photo capture, or the spec must document that `expo-image-picker` owns a separate camera permission prompt.

Alternative considered: rely only on image picker `quality` options. That can compress but does not reliably enforce the 2048 px long-edge requirement.

### 2. Keep photo capture inside the existing Capture route

The existing `apps/mobile/app/(tabs)/index.tsx` route remains the capture home. A camera icon action is added to the lower capture toolbar/action row so photo capture is reachable from the paired surface regardless of text or voice mode. The unpaired branch continues to render pairing controls only and must not request image permissions.

Alternative considered: add a separate photo tab or route. That adds navigation friction for a capture action whose value is speed.

### 3. Use an ActionSheet-style source picker

Activating the camera icon opens choices for "Take Photo", "Pick from Library", and cancel. The chosen source determines which permission is requested and which picker API is launched. Permission denial is terminal for that attempt and must not enqueue a payload.

Alternative considered: launch the camera directly. That hides the library use case required by US-045 and makes permission prompts less predictable.

### 4. Preprocess before building the payload

The preprocessing helper receives a selected image asset, reads only the local asset URI, applies orientation-aware resize if the long edge exceeds 2048 px, writes a JPEG at quality 85, and measures encoded bytes. If encoded bytes exceed 5 MB, it retries at quality 70. If still over 5 MB, it rejects the capture locally. The output to bundle code is `{ imageBase64, width, height, byteLength }`.

Because base64 expands binary data by roughly one third, implementation must also check the serialized `capture-image` payload/envelope size before enqueue. A 5 MB JPEG normally remains below the desktop 12 MB decoded-content cap after base64 and JSON overhead, but the guard should be tested rather than assumed.

Alternative considered: upload original camera/library bytes. That would preserve formats like HEIC/RAW and very large images, but it would push decode complexity to desktop and increase encrypted bundle size.

### 5. Preserve EXIF when supported, but do not block capture on missing EXIF

US-045 requires not stripping EXIF. Implementation should request EXIF from picker assets and use preprocessing APIs that preserve metadata when available. If a platform/API cannot preserve EXIF during JPEG re-encode, the app should continue capture rather than storing originals or adding native code, and record the limitation in implementation notes.

Alternative considered: avoid all re-encoding to preserve EXIF exactly. That conflicts with the JPEG normalization and size requirements.

### 6. Treat caption as payload metadata, not outbox plaintext

The optional caption is included as `caption: string | null` in the `capture-image` plaintext payload before encryption. It may be used as the outbox mini-preview, bounded by the existing preview length, but must not be stored as a durable plaintext payload or retry metadata.

Alternative considered: enqueue a separate `capture-text` row for the caption. That loses the semantic relationship between image and text and complicates desktop OCR composition.

### 7. Extend bundle helpers without duplicating crypto in UI

`apps/mobile/src/crypto/bundle.ts` should add `createCaptureImagePayload()`, `buildCaptureImageEnvelope()`, and `encryptCaptureImage()` using the same `secureSerialize()`, `BundleEnvelope`, content hash, sync key, nonce, and AAD path already used by text/audio. UI code should only call the helper and enqueue the encrypted result.

Alternative considered: construct image envelopes in the Capture screen. That would spread serialization and key handling across UI code.

### 8. Store `capture-image` content type per outbox row

The existing outbox content-type column should add `CAPTURE_IMAGE_CONTENT_TYPE = application/syncmind.capture-image+json`. Image rows upload with that value, while existing and default rows remain text-compatible.

Alternative considered: infer content type from row id or decrypt before upload. Inference is brittle, and decrypting in the upload path breaks the encrypted outbox boundary.

### 9. Keep Spine unchanged and add narrow desktop caption retention

Spine already accepts arbitrary `X-Syncmind-Content-Type` values, stores encrypted bytes opaquely, and publishes notifications. This change should not add a relay requirement or a relay test solely for the literal `application/syncmind.capture-image+json` header. Desktop `capture-image` dispatch currently exists in `apps/desktop/src-tauri/src/spine/dispatch.rs`, but it must be extended to parse optional `caption` and write non-empty captions into generated markdown as plain body text. OCR post-processing must preserve that caption when it rewrites successful OCR markdown or appends fallback markers.

Alternative considered: revive `/v1/media/upload`. That bypasses the encrypted outbox path now used by mobile captures.

## Risks / Trade-offs

- [Risk] Expo SDK 56 image manipulation APIs may not preserve EXIF during JPEG re-encode -> Mitigation: request/preserve EXIF where supported, document platform limitations, and keep the normalized JPEG path for size/desktop compatibility.
- [Risk] Base64 image payloads inflate size by about one third -> Mitigation: enforce 5 MB encoded JPEG before encryption and add an explicit serialized payload/envelope size test against the existing 12 MB decoded-content cap.
- [Risk] Desktop currently drops `caption` silently -> Mitigation: add desktop dispatch tests and a narrow dispatcher update that preserves caption text in markdown before and after OCR.
- [Risk] Camera permission copy can diverge between QR scanning and photo capture -> Mitigation: make permission-copy review a setup task and update shared camera wording or document a separate image-picker prompt.
- [Risk] Large image processing can briefly increase memory use -> Mitigation: process one selected image at a time, resize before base64 when possible, and avoid retaining original bytes after preprocessing.
- [Risk] Permission prompts can interrupt the fast capture flow -> Mitigation: request only after source selection and show concise denial feedback without changing pairing state.
- [Risk] Caption plaintext could leak through previews or logs -> Mitigation: bound preview text and keep bundle payload serialization guarded through `secureSerialize()`.
- [Risk] Desktop may ignore caption even if it accepts the payload -> Mitigation: verify desktop dispatch behavior before acceptance and add/link a separate desktop delta if caption indexing is missing.

## Migration Plan

1. Confirm Expo SDK 56 image picker, manipulation, file-system, base64, and EXIF behavior before coding.
2. Add only the verified mobile dependencies and native permission copy.
3. Add `capture-image` bundle helpers and tests.
4. Add image preprocessing helper and tests for resize, JPEG normalization, quality retry, serialized-size guard, oversize rejection, and temp cleanup.
5. Add Capture screen camera ActionSheet, caption prompt/form, enqueue, and best-effort flush.
6. Add desktop caption retention tests and update `capture-image` markdown generation/OCR post-processing.
7. Add outbox upload tests for `application/syncmind.capture-image+json`.
8. Verify desktop `capture-image` dispatch and OCR compatibility.

Rollback is limited to removing the photo UI and new image dependencies. Existing text/audio outbox rows remain compatible because their content types are unchanged.

## Open Questions

- The exact Expo SDK 56 manipulation package/API and EXIF preservation behavior must be resolved during implementation task 1.1.
- The final caption UI can be a modal/prompt or a compact in-screen review step, as long as it supports send-without-caption and no-capture cancel paths.
