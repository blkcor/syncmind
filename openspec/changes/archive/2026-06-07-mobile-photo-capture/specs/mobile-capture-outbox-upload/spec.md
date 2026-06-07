## ADDED Requirements

### Requirement: Outbox rows support capture-image content type

The mobile outbox SHALL persist and upload `capture-image` rows with sync-bundle content type `application/syncmind.capture-image+json`. Existing rows without explicit metadata SHALL continue to upload as `application/syncmind.capture-text+json`.

#### Scenario: Image row uploads with image content type
- **WHEN** a `capture-image` row is flushed
- **THEN** the upload request includes `X-Syncmind-Content-Type: application/syncmind.capture-image+json`
- **AND** the request body is the encrypted outbox blob
- **AND** the upload path does not decrypt the row to discover its kind

#### Scenario: Existing text and audio content types are unchanged
- **WHEN** a `capture-text` or `capture-audio` row is flushed
- **THEN** the upload request uses the row content type already defined for that capture kind
- **AND** adding image support does not change existing text/audio upload metadata

### Requirement: Mobile bundle encryption supports capture-image

The mobile app SHALL encrypt `capture-image` payloads using the same v1 `BundleEnvelope`, `secureSerialize()`, AES-256-GCM key, nonce, AAD, and `nonce | ciphertext_and_tag` wire format used for `capture-text` and `capture-audio`.

#### Scenario: Valid image payload creates capture-image envelope
- **WHEN** the app encrypts a valid image capture payload
- **THEN** it constructs an outer `BundleEnvelope` with `schema_version = 1`
- **AND** `kind = "capture-image"`
- **AND** `filename = "capture-<id>.json"` or another deterministic JSON filename containing the capture id
- **AND** `content_utf8` is the serialized `capture-image` payload
- **AND** `sha256` is lower-hex SHA-256 of `content_utf8` UTF-8 bytes

#### Scenario: Image encryption uses paired sync key and peer AAD
- **WHEN** the app encrypts a valid image capture payload
- **AND** restored pairing state contains `syncKey` and `pairedPeerFingerprint`
- **THEN** encryption uses `syncKey` as the AES-256-GCM key
- **AND** encryption uses a fresh 12-byte random nonce
- **AND** encryption uses AAD equal to the bytes decoded from `pairedPeerFingerprint` after the `sha256:` prefix
- **AND** the output blob starts with the 12-byte nonce followed by ciphertext and the 16-byte GCM tag

#### Scenario: Direct stringify is rejected for image payloads
- **WHEN** development/test code attempts to serialize a guarded `capture-image` payload without `secureSerialize()`
- **THEN** the guard fails fast before the payload can be persisted or uploaded
- **AND** `secureSerialize()` remains the allowed path for producing plaintext JSON bytes

#### Scenario: Image encryption result is enqueued with image content type
- **WHEN** a `capture-image` payload is encrypted successfully
- **THEN** the encrypted outbox row is enqueued with content type `application/syncmind.capture-image+json`
- **AND** the row stores only encrypted bytes and bounded non-sensitive preview metadata
