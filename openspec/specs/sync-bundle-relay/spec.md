# sync-bundle-relay Specification

## Purpose
Defines the encrypted sync bundle relay contract including upload, idempotency, WebSocket notification, paginated listing, download, acknowledgment, and multi-instance broadcast. Mobile capture clients consume the same opaque upload contract as other sync bundle clients.
## Requirements
### Requirement: Encrypted sync bundle upload
The system SHALL accept encrypted sync bundles from authenticated devices and store them opaquely.

#### Scenario: Successful bundle upload
- **WHEN** an authenticated device sends a POST request to `/v1/sync/bundle` with an encrypted payload and `X-Syncmind-Content-Type` header
- **AND** the payload size is within `max_bundle_size_mb`
- **AND** the `from_device_id` inferred from the JWT matches the authenticated device
- **AND** the authenticated device has an active paired device
- **THEN** the system stores the encrypted payload in `sync_bundles` as BYTEA
- **AND** the system computes and stores `payload_hash` (SHA-256 of the encrypted blob)
- **AND** the system sets `expires_at = NOW() + bundle_retention_days`
- **AND** the system returns HTTP 201 Created with `bundle_id`
- **AND** the system publishes a `SyncNotification` to Redis channel `sync:notify:{to_device_id}`

#### Scenario: Payload too large
- **WHEN** an authenticated device uploads a bundle whose size exceeds `max_bundle_size_mb`
- **THEN** the system returns HTTP 413 Payload Too Large with error code `BUNDLE_TOO_LARGE`

#### Scenario: Unpaired device upload attempt
- **WHEN** an authenticated device with no active `paired_device_id` attempts to upload a bundle
- **THEN** the system returns HTTP 422 Unprocessable Entity with error code `DEVICE_NOT_PAIRED`

#### Scenario: Idempotent upload
- **WHEN** a device resubmits a bundle with the same `Idempotency-Key` header within 24 hours
- **THEN** the system returns the existing `bundle_id` without creating a duplicate record
- **AND** the system returns HTTP 201 Created

### Requirement: Real-time sync notification via WebSocket
The system SHALL push real-time notifications to connected devices when new sync bundles are available.

#### Scenario: Live notification on new bundle
- **WHEN** a sync bundle is successfully uploaded for device B
- **AND** device B has an active WebSocket connection to `/v1/sync/live`
- **THEN** the system delivers a JSON message to device B's WebSocket connection:
  ```json
  {"type":"new_bundle","bundle_id":"uuid","from_device":"uuid","payload_size":12345,"content_type":"text/markdown"}
  ```

#### Scenario: Offline device receives notification later
- **WHEN** a sync bundle is uploaded for device B
- **AND** device B has no active WebSocket connection
- **THEN** the Redis notification is dropped (no persistence)
- **AND** device B discovers the bundle upon its next poll of `/v1/sync/bundles`

### Requirement: WebSocket heartbeat
The system SHALL maintain connection health via bidirectional heartbeat.

#### Scenario: Heartbeat timeout closes connection
- **WHEN** the server sends a `{"type":"ping"}` message every 30 seconds
- **AND** the client fails to respond with `{"type":"pong"}` within 10 seconds
- **THEN** the server closes the WebSocket connection and removes it from the device connection map

### Requirement: Paginated bundle listing
The system SHALL allow devices to list pending sync bundles without downloading full payloads.

#### Scenario: List pending bundles
- **WHEN** an authenticated device sends a GET request to `/v1/sync/bundles?limit=20&before_id=<uuid>`
- **THEN** the system returns a JSON array of bundle metadata where `to_device_id` matches the authenticated device and `acked_at IS NULL`
- **AND** the response does NOT include the `encrypted_payload` field
- **AND** each entry includes `bundle_id`, `from_device`, `payload_size`, `content_type`, `created_at`, and `payload_hash`

### Requirement: Encrypted bundle download
The system SHALL allow a target device to download its full encrypted bundle payload.

#### Scenario: Download specific bundle
- **WHEN** an authenticated device sends a GET request to `/v1/sync/bundles/{bundle_id}`
- **AND** the bundle's `to_device_id` matches the authenticated device
- **THEN** the system returns the encrypted payload as `application/octet-stream`
- **AND** the response includes `X-Syncmind-Content-Type` (original content type)
- **AND** the response includes `X-Syncmind-Payload-Hash` for client-side integrity verification

#### Scenario: Download unauthorized bundle
- **WHEN** an authenticated device requests a bundle whose `to_device_id` does not match the authenticated device
- **THEN** the system returns HTTP 404 Not Found (to prevent information leakage about bundle existence)

### Requirement: Bundle acknowledgment and cleanup
The system SHALL allow devices to acknowledge successful processing of a bundle, triggering soft deletion.

#### Scenario: Acknowledge bundle
- **WHEN** an authenticated device sends a DELETE request to `/v1/sync/bundles/{bundle_id}`
- **AND** the bundle's `to_device_id` matches the authenticated device
- **THEN** the system sets `acked_at = NOW()` (soft delete)
- **AND** the system returns HTTP 204 No Content

#### Scenario: Physical cleanup of acknowledged bundles
- **WHEN** a scheduled cleanup job runs daily
- **THEN** the system hard-deletes all bundles where `acked_at IS NOT NULL` and `deleted_at < NOW() - INTERVAL '7 days'`
- **AND** the system hard-deletes all bundles where `expires_at < NOW()` regardless of `acked_at` status

### Requirement: Redis Pub/Sub broadcast for horizontal scaling
The system SHALL use Redis Pub/Sub to broadcast sync notifications across multiple Spine instances.

#### Scenario: Multi-instance notification delivery
- **WHEN** Spine instance A uploads a bundle for device B
- **AND** device B is connected via WebSocket to Spine instance B
- **THEN** instance A publishes to Redis channel `sync:notify:{device_b_id}`
- **AND** instance B receives the Redis message and forwards it to device B's WebSocket connections

### Requirement: Mobile capture clients use the existing sync bundle upload contract

Mobile capture clients SHALL upload encrypted capture bundles through the existing `POST /v1/sync/bundle` endpoint using the same opaque payload and idempotency semantics as other sync bundle clients. Spine SHALL continue to treat the request body as encrypted bytes and SHALL NOT inspect or decrypt mobile capture payloads.

#### Scenario: Mobile capture upload is accepted as an opaque sync bundle

- **WHEN** an authenticated mobile device sends `POST /v1/sync/bundle`
- **AND** the request body is an encrypted mobile capture bundle
- **AND** the request includes `Content-Type: application/octet-stream`
- **AND** the request includes `X-Syncmind-Content-Type: application/syncmind.capture-text+json` for `capture-text` bundles
- **AND** the request includes `Idempotency-Key`
- **AND** the mobile device has an active paired desktop device
- **THEN** Spine stores the encrypted payload opaquely in `sync_bundles`
- **AND** computes `payload_hash` over the encrypted blob
- **AND** returns HTTP 201 Created with `bundle_id`
- **AND** publishes the normal sync notification to the paired desktop

#### Scenario: Mobile retry reuses idempotency key

- **WHEN** the same mobile outbox row retries upload with the same `Idempotency-Key`
- **THEN** Spine returns the original `bundle_id`
- **AND** does not create a duplicate `sync_bundles` row

#### Scenario: Spine does not inspect capture plaintext

- **WHEN** Spine receives an encrypted mobile capture bundle
- **THEN** Spine does not parse, decrypt, transform, log, or index the capture payload plaintext
- **AND** any server-visible metadata is limited to headers and opaque bundle metadata already defined by the sync bundle relay contract

