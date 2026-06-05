## ADDED Requirements

### Requirement: Mobile audio capture bundles relay opaquely

Spine SHALL accept encrypted mobile audio capture bundles through the existing sync bundle relay contract and SHALL treat `application/syncmind.capture-audio+json` as opaque encrypted content metadata.

#### Scenario: Mobile audio capture upload is accepted as sync bundle
- **WHEN** an authenticated mobile device sends `POST /v1/sync/bundle`
- **AND** the request body is an encrypted `capture-audio` bundle
- **AND** the request includes `Content-Type: application/octet-stream`
- **AND** the request includes `X-Syncmind-Content-Type: application/syncmind.capture-audio+json`
- **AND** the request includes `Idempotency-Key`
- **AND** the mobile device has an active paired desktop device
- **THEN** Spine stores the encrypted payload opaquely in `sync_bundles`
- **AND** stores the content type as `application/syncmind.capture-audio+json`
- **AND** computes `payload_hash` over the encrypted blob
- **AND** returns HTTP 201 Created with `bundle_id`
- **AND** publishes the normal sync notification to the paired desktop

#### Scenario: Spine does not inspect audio plaintext
- **WHEN** Spine receives an encrypted mobile audio capture bundle
- **THEN** Spine does not parse, decrypt, transform, transcribe, log, or index the capture audio plaintext
