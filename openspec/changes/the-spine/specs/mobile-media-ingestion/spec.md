## ADDED Requirements

### Requirement: Encrypted media blob upload
The system SHALL accept encrypted image and audio blobs from mobile devices and route them to the paired desktop as opaque data.

#### Scenario: Successful media upload
- **WHEN** an authenticated mobile device sends a POST request to `/v1/media/upload` with `multipart/form-data`
- **AND** the form contains an encrypted `file` blob and a `metadata` JSON field with `content_type`, `file_name`, and `file_size`
- **AND** the declared `content_type` is one of: `image/jpeg`, `image/png`, `image/heic`, `audio/m4a`, `audio/wav`
- **AND** the file size is within `max_bundle_size_mb`
- **AND** the mobile device has an active paired desktop device
- **THEN** the system stores the encrypted blob in `sync_bundles` with `content_type` set to the client's declared original type
- **AND** the system sets `to_device_id` to the paired desktop's ID
- **AND** the system returns HTTP 201 Created with `{media_id, bundle_id, expires_at}`
- **AND** the system publishes a `SyncNotification` to the desktop's Redis channel

#### Scenario: Unsupported content type
- **WHEN** a mobile device uploads a file with a `content_type` not in the allowed whitelist
- **THEN** the system returns HTTP 415 Unsupported Media Type with error code `MEDIA_TYPE_UNSUPPORTED`

#### Scenario: File size exceeds limit
- **WHEN** a mobile device uploads a media file whose size exceeds `max_bundle_size_mb`
- **THEN** the system returns HTTP 413 Payload Too Large with error code `MEDIA_TOO_LARGE`

### Requirement: Media remains opaque to Spine
The system SHALL NOT inspect, parse, transform, or generate metadata from uploaded media content.

#### Scenario: Spine stores encrypted media without inspection
- **WHEN** a mobile device uploads an encrypted image blob
- **THEN** the system writes the raw bytes to `sync_bundles.encrypted_payload` without calling any image parsing, EXIF extraction, thumbnail generation, or OCR libraries
- **AND** the system writes the raw bytes without calling any audio transcription or speech-to-text libraries
- **AND** the system includes no code paths that decrypt the payload

### Requirement: Media delivery to desktop
The system SHALL treat media bundles identically to standard sync bundles for delivery and acknowledgment.

#### Scenario: Desktop downloads media bundle
- **WHEN** a desktop device lists pending bundles via `/v1/sync/bundles`
- **AND** a media bundle is among the results with `content_type: image/jpeg`
- **THEN** the desktop device downloads the bundle via `/v1/sync/bundles/{id}` as opaque encrypted bytes
- **AND** after local decryption and processing (by Rust Core), the desktop acknowledges the bundle via DELETE
- **AND** the Spine marks the bundle as acknowledged

### Requirement: Media upload rate limiting
The system SHALL enforce rate limits on media uploads to prevent abuse.

#### Scenario: Rate limit exceeded
- **WHEN** a mobile device exceeds 100 uploads per minute
- **THEN** the system returns HTTP 429 Too Many Requests with a `Retry-After` header
- **AND** the system logs the rate-limit event with the device ID
