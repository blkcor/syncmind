## ADDED Requirements

### Requirement: Photo capture is available from the paired Capture screen

The mobile app SHALL expose US-045 photo capture from the existing paired Capture screen through a camera icon in the capture toolbar/action area. The control SHALL open an ActionSheet-style source picker with "Take Photo", "Pick from Library", and cancel choices.

#### Scenario: Paired user opens the photo source picker
- **WHEN** the app is paired
- **AND** the user activates the camera icon in the Capture screen toolbar
- **THEN** the app shows choices for "Take Photo", "Pick from Library", and cancel
- **AND** no camera or library permission is requested until the user chooses a source
- **AND** no capture payload is created by opening the picker alone

#### Scenario: User cancels the photo source picker
- **WHEN** the source picker is visible
- **AND** the user chooses cancel or dismisses the picker
- **THEN** the app returns to the Capture screen
- **AND** no image picker is launched
- **AND** no capture payload or outbox row is created

#### Scenario: Unpaired user cannot start photo capture
- **WHEN** the app is not paired
- **THEN** the Capture screen shows the pairing scanner or unpaired state
- **AND** it does not show an active photo capture control
- **AND** it does not request camera or library permissions

### Requirement: Camera and library permissions gate image selection

The mobile app SHALL use Expo SDK 56-compatible image picker APIs for both camera capture and library selection. Camera permission SHALL be requested only for "Take Photo". Photo-library permission SHALL be requested only for "Pick from Library".

#### Scenario: Native camera permission copy covers both camera uses
- **WHEN** the implementation configures native camera permission text
- **THEN** the camera permission copy covers both desktop QR scanning and photo capture
- **OR** the implementation documents that the image picker owns a separate camera permission prompt for photo capture

#### Scenario: Camera permission granted launches camera
- **WHEN** the paired user chooses "Take Photo"
- **AND** camera permission is granted
- **THEN** the app launches the platform camera picker for one image
- **AND** the selected asset is passed to the image preprocessing pipeline

#### Scenario: Camera permission denied creates no capture
- **WHEN** the paired user chooses "Take Photo"
- **AND** camera permission is denied
- **THEN** the app does not launch the camera picker
- **AND** shows "Enable camera access to take photos."
- **AND** no capture payload or outbox row is created

#### Scenario: Library permission granted launches picker
- **WHEN** the paired user chooses "Pick from Library"
- **AND** photo-library permission is granted
- **THEN** the app launches the platform library picker for one image
- **AND** the selected asset is passed to the image preprocessing pipeline

#### Scenario: Library permission denied creates no capture
- **WHEN** the paired user chooses "Pick from Library"
- **AND** photo-library permission is denied
- **THEN** the app does not launch the library picker
- **AND** shows "Enable photo library access to pick images."
- **AND** no capture payload or outbox row is created

### Requirement: Selected images are normalized for desktop OCR

The mobile app SHALL preprocess selected camera or library images before encryption so the encoded image is JPEG with MIME type `image/jpeg`. If either image edge is greater than 2048 px, the app SHALL resize proportionally so the long edge is 2048 px and the short edge preserves aspect ratio.

#### Scenario: Oversized image is resized proportionally
- **WHEN** the user selects an image with width or height greater than 2048 px
- **THEN** the preprocessing pipeline resizes the image so the long edge is 2048 px
- **AND** preserves the original aspect ratio within rounding tolerance
- **AND** returns the post-resize `width` and `height` for the payload

#### Scenario: Image within bounds keeps dimensions
- **WHEN** the user selects an image whose width and height are both 2048 px or smaller
- **THEN** the preprocessing pipeline does not upscale the image
- **AND** returns the selected image dimensions or orientation-corrected dimensions for the payload

#### Scenario: Output is JPEG regardless of source format
- **WHEN** the selected image is HEIC, PNG, RAW, or another picker-supported format
- **THEN** the preprocessing pipeline re-encodes the output as JPEG
- **AND** the capture payload uses `image_mime = "image/jpeg"`

#### Scenario: EXIF is preserved when supported
- **WHEN** the selected image asset includes EXIF metadata
- **AND** the SDK 56 preprocessing API supports metadata preservation for JPEG output
- **THEN** the app preserves the metadata in the encoded JPEG
- **AND** does not intentionally strip orientation, time, or location metadata

### Requirement: Image captures enforce encoded size limits

The mobile app SHALL encode image captures at JPEG quality 85 first. If the encoded JPEG exceeds 5 MB, the app SHALL retry encoding at quality 70. If the retry still exceeds 5 MB, the app SHALL reject the capture locally before encryption or enqueue. After base64 and JSON serialization, the app SHALL also reject any `capture-image` payload/envelope that would exceed the existing decoded bundle content cap.

#### Scenario: Quality 85 output within cap is accepted
- **WHEN** preprocessing at JPEG quality 85 produces an encoded image of 5 MB or less
- **THEN** the app uses that JPEG for the `capture-image` payload
- **AND** does not run the quality 70 retry

#### Scenario: Oversized quality 85 output retries at quality 70
- **WHEN** preprocessing at JPEG quality 85 produces an encoded image larger than 5 MB
- **THEN** the app retries JPEG encoding at quality 70
- **AND** uses the quality 70 JPEG if it is 5 MB or less

#### Scenario: Oversized quality 70 output is rejected
- **WHEN** preprocessing at JPEG quality 70 still produces an encoded image larger than 5 MB
- **THEN** the app rejects the capture locally
- **AND** shows "Image too large - try a smaller photo."
- **AND** no capture payload or encrypted outbox row is created

#### Scenario: Serialized image payload over bundle cap is rejected
- **WHEN** a processed JPEG is within the 5 MB encoded image cap
- **AND** the resulting base64 JSON payload or envelope would exceed the existing decoded bundle content cap
- **THEN** the app rejects the capture locally before encryption or enqueue
- **AND** shows "Image too large - try a smaller photo."
- **AND** no encrypted outbox row is created

#### Scenario: Temporary image files are cleaned up
- **WHEN** preprocessing succeeds, is rejected, or is cancelled after a temporary file is created
- **THEN** the app deletes temporary processed image files best-effort
- **AND** does not persist the original image URI, original bytes, or processed JPEG bytes outside the encrypted outbox

### Requirement: Optional caption is bundled with the image

The mobile app SHALL allow the user to send an image capture with no caption or with a short optional caption. The caption SHALL be included in the same `capture-image` plaintext payload as `caption: string | null` before encryption.

#### Scenario: User sends an image without caption
- **WHEN** the user selects or takes an image
- **AND** skips the caption step
- **THEN** the app creates a `capture-image` payload with `caption = null`
- **AND** the image and caption field are encrypted into one bundle

#### Scenario: User sends an image with caption
- **WHEN** the user selects or takes an image
- **AND** enters a caption before sending
- **THEN** the app creates a `capture-image` payload with `caption` equal to the entered text
- **AND** the image and caption are encrypted into one bundle

#### Scenario: User cancels caption review
- **WHEN** the selected image is waiting for caption review
- **AND** the user cancels the capture
- **THEN** no capture payload or outbox row is created
- **AND** temporary processed image files are deleted best-effort

### Requirement: Image payload matches the capture-image schema

The mobile app SHALL build the plaintext `capture-image` payload only transiently before bundle encryption. The payload SHALL include `v: 1`, `kind: "capture-image"`, `id`, `image_base64`, `image_mime: "image/jpeg"`, `width`, `height`, `caption`, `client_ts`, and `client_device_fingerprint`.

#### Scenario: Valid image produces capture-image plaintext before encryption
- **WHEN** a selected image passes preprocessing and size validation
- **THEN** the app creates a payload with `v = 1`
- **AND** `kind = "capture-image"`
- **AND** `id` is a UUID v4
- **AND** `image_base64` contains the processed JPEG bytes encoded as base64
- **AND** `image_mime = "image/jpeg"`
- **AND** `width` and `height` match the processed JPEG dimensions
- **AND** `caption` is either the entered caption string or `null`
- **AND** `client_ts` is the capture timestamp
- **AND** `client_device_fingerprint` is the local mobile device fingerprint

#### Scenario: Plaintext image data is not durably persisted
- **WHEN** a `capture-image` payload is built
- **THEN** plaintext JSON and image bytes are used only long enough to encrypt the bundle
- **AND** SQLite outbox rows store encrypted bytes and non-sensitive metadata only
- **AND** logs, retry metadata, and status previews do not include `image_base64`, raw image bytes, sync keys, or full payload JSON

### Requirement: Valid image capture enqueues and flushes

The paired Capture screen SHALL encrypt and enqueue a valid `capture-image` bundle after source selection, preprocessing, optional caption review, and payload construction. The app SHALL trigger a best-effort outbox flush without blocking UI on network success.

#### Scenario: Valid photo capture enters encrypted outbox
- **WHEN** the app is paired
- **AND** the user selects or takes an image
- **AND** the image passes preprocessing and size validation
- **AND** the user confirms the caption review step
- **THEN** the app encrypts and enqueues a `capture-image` bundle
- **AND** the outbox row uses non-sensitive preview metadata only
- **AND** the app starts a best-effort `flushOutbox()`

#### Scenario: Queue full is surfaced without plaintext fallback
- **WHEN** a valid image capture is ready to enqueue
- **AND** the encrypted outbox rejects the row because the queue is full
- **THEN** the app shows "Capture queue is full - connect to upload or retry failed captures"
- **AND** no plaintext fallback row or image file is persisted

#### Scenario: Picker failure is surfaced without enqueue
- **WHEN** camera or library picker launch fails unexpectedly
- **THEN** the app shows "Could not select image."
- **AND** no capture payload or outbox row is created

#### Scenario: Preprocessing failure is surfaced without enqueue
- **WHEN** a selected image cannot be resized, re-encoded, measured, or read as base64
- **THEN** the app shows "Could not prepare image."
- **AND** no capture payload or outbox row is created
