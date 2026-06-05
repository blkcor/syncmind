## ADDED Requirements

### Requirement: Outbox rows carry capture content type

The mobile outbox SHALL persist the sync-bundle content type needed to upload each encrypted row. Existing rows without explicit metadata SHALL upload as `application/syncmind.capture-text+json`.

#### Scenario: Text row keeps existing upload content type
- **WHEN** an existing text capture row is flushed
- **AND** the row has no explicit content-type metadata
- **THEN** the upload request includes `X-Syncmind-Content-Type: application/syncmind.capture-text+json`

#### Scenario: Audio row uploads with audio content type
- **WHEN** a `capture-audio` row is flushed
- **THEN** the upload request includes `X-Syncmind-Content-Type: application/syncmind.capture-audio+json`
- **AND** the request body is the encrypted outbox blob
- **AND** the upload path remains `POST /v1/sync/bundle`

### Requirement: Mobile bundle encryption supports capture-audio

The mobile app SHALL encrypt `capture-audio` payloads using the same v1 `BundleEnvelope`, `secureSerialize()`, AES-256-GCM key, nonce, AAD, and `nonce | ciphertext_and_tag` wire format used for `capture-text`.

#### Scenario: Capture-audio envelope uses recognized kind
- **WHEN** the app encrypts a valid audio capture payload
- **THEN** the outer bundle envelope has `schema_version = 1`
- **AND** `kind = "capture-audio"`
- **AND** `filename = "capture-<id>.json"` or another deterministic JSON filename containing the capture id
- **AND** `content_utf8` is the serialized `capture-audio` payload
- **AND** `sha256` is lower-hex SHA-256 of `content_utf8` UTF-8 bytes

#### Scenario: Capture-audio encryption uses existing pairing keys
- **WHEN** the app encrypts a valid audio capture payload
- **AND** restored pairing state contains `syncKey` and `pairedPeerFingerprint`
- **THEN** encryption uses the 32-byte `syncKey`
- **AND** encryption uses a fresh 12-byte random nonce
- **AND** encryption uses AAD decoded from `pairedPeerFingerprint`
- **AND** the persisted outbox blob is `nonce | ciphertext_and_tag`
