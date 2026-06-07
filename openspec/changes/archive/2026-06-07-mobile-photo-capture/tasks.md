## 1. Setup And API Verification

- [x] 1.1 Re-check Expo SDK 56 image picker, image manipulation, file-system/base64, and EXIF behavior before coding; record the selected APIs and any EXIF preservation limits in implementation notes.
- [x] 1.2 Add the verified SDK 56 image picker dependency to `apps/mobile/package.json`.
- [x] 1.3 Add the verified SDK 56 image manipulation dependency if resize/re-encode is not provided by the picker API.
- [x] 1.4 Update native camera permission copy in `apps/mobile/app.json` so it covers both QR scanning and photo capture, or record that `expo-image-picker` owns a separate camera prompt.
- [x] 1.5 Add native photo library permission copy in `apps/mobile/app.json` if required by the selected image picker package.
- [x] 1.6 Update Jest mocks/setup for the selected Expo image picker, manipulation, and file-system APIs.

## 2. Capture-Image Bundle Crypto

- [x] 2.1 Add failing bundle tests for the US-045 `capture-image` plaintext schema, including `caption: null` and non-empty caption.
- [x] 2.2 Add failing bundle tests for `capture-image` envelope kind, deterministic filename, content hash, AES-GCM wire shape, and secure serialization guard.
- [x] 2.3 Implement `CaptureImagePayload`, `createCaptureImagePayload()`, `buildCaptureImageEnvelope()`, and `encryptCaptureImage()` in `apps/mobile/src/crypto/bundle.ts`.
- [x] 2.4 Ensure tests do not snapshot or persist raw `image_base64` outside targeted serialization checks.

## 3. Image Preprocessing

- [x] 3.1 Add a focused image preprocessing helper under `apps/mobile/src/capture/` for picker asset input, resize decisions, JPEG output, size measurement, base64 output, and temp cleanup.
- [x] 3.2 Add tests for proportional resize when width or height exceeds 2048 px.
- [x] 3.3 Add tests that images within 2048 px are not upscaled.
- [x] 3.4 Add tests that non-JPEG inputs are normalized to `image/jpeg`.
- [x] 3.5 Add tests for JPEG quality 85 success, quality 70 retry, and rejection when quality 70 still exceeds 5 MB.
- [x] 3.6 Add tests for rejecting a processed image whose base64 JSON payload/envelope would exceed the existing decoded bundle content cap.
- [x] 3.7 Add tests or implementation notes for EXIF preservation when supported by the selected SDK 56 APIs.
- [x] 3.8 Delete processed temp files best-effort after successful enqueue, validation rejection, caption cancel, and picker cancel paths.

## 4. Outbox Upload Metadata

- [x] 4.1 Add a failing mobile outbox test for `capture-image` rows uploading with `X-Syncmind-Content-Type: application/syncmind.capture-image+json`.
- [x] 4.2 Add `CAPTURE_IMAGE_CONTENT_TYPE` without changing existing text/audio content type behavior.
- [x] 4.3 Update the image enqueue path to store per-row sync-bundle content type without decrypting queued blobs.
- [x] 4.4 Keep existing text/audio outbox upload tests passing for rows with and without explicit content-type metadata.

## 5. Capture Screen Photo UI

- [x] 5.1 Add Capture screen tests for a camera toolbar icon on the paired screen and no active photo control in the unpaired state.
- [x] 5.2 Add tests for the ActionSheet choices: "Take Photo", "Pick from Library", and cancel.
- [x] 5.3 Add tests that camera permission is requested only after "Take Photo" and library permission only after "Pick from Library".
- [x] 5.4 Add tests for permission denial creating no payload and no outbox row.
- [x] 5.5 Implement the camera icon action, source picker, permission gates, and single-image camera/library picker launch.
- [x] 5.6 Add caption review UI that allows send without caption, send with caption, and cancel without enqueue.
- [x] 5.7 Wire valid preprocessed images to `encryptCaptureImage()`, `enqueueOutboxItem()`, and best-effort `flushOutbox()`.
- [x] 5.8 Show the spec-defined local feedback strings for image-too-large, picker failure, preprocessing failure, queue full, and permission denial without logging image plaintext.

## 6. Desktop Caption Compatibility

- [x] 6.1 Add failing desktop dispatch tests for captioned `capture-image` payloads writing caption text into `<data-dir>/sync-inbox/captures/<id>.md`.
- [x] 6.2 Update desktop `CaptureImagePayload` with optional `caption` and write non-empty captions as plain markdown body text.
- [x] 6.3 Update image OCR post-processing so successful OCR rewrites preserve caption text while adding OCR output and `image_file`.
- [x] 6.4 Update image OCR fallback paths so no-text, decode-failed, or OCR-unavailable markers do not delete caption text.
- [x] 6.5 Verify desktop dispatch writes `<data-dir>/sync-inbox/images/<id>.jpg` plus capture markdown and triggers the OCR path.
- [x] 6.6 Keep `/v1/media/upload` out of the mobile photo capture path.

## 7. Verification

- [x] 7.1 Run `openspec validate mobile-photo-capture --strict`.
- [x] 7.2 Run mobile tests relevant to bundle crypto, image preprocessing, outbox upload, and capture screen behavior.
- [x] 7.3 Run mobile typecheck.
- [x] 7.4 Run mobile lint.
- [x] 7.5 Run desktop Rust tests for spine dispatch/OCR after desktop caption handling is touched.
- [x] 7.6 Update PRD 005 US-045 status after implementation is complete and accepted.
