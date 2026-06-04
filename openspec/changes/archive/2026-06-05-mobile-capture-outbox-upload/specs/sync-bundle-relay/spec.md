## ADDED Requirements

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
