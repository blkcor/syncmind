## MODIFIED Requirements

### Requirement: Capture payloads are securely serialized before encryption

The mobile app SHALL convert capture payload objects into UTF-8 plaintext bytes only through a `secureSerialize()` helper used by the bundle encryption path. For text captures, the inner plaintext payload serialized into `BundleEnvelope.content_utf8` SHALL contain `v: 1`, `kind: "capture-text"`, `id`, `text`, `source: "typed"`, `client_ts`, and `client_device_fingerprint`. The app MUST NOT log, persist, or attach plaintext capture payloads to diagnostics, retry metadata, or outbox rows beyond the bounded mini-preview metadata explicitly used by the capture screen.

#### Scenario: Text capture serialization produces plaintext bytes only transiently

- **WHEN** the user sends a non-empty text capture
- **THEN** the app constructs an inner capture payload object with `v = 1`, `kind = "capture-text"`, `id`, `text`, `source = "typed"`, `client_ts`, and `client_device_fingerprint`
- **AND** `client_device_fingerprint` is the local mobile device fingerprint string
- **AND** constructs an outer `BundleEnvelope` with `schema_version = 1`, `kind = "capture-text"`, `filename`, `content_utf8`, `captured_at`, and `sha256`
- **AND** `content_utf8` contains the serialized inner capture payload
- **AND** `secureSerialize()` returns UTF-8 JSON bytes of the outer envelope for encryption
- **AND** the outbox row stores only encrypted bytes and non-sensitive metadata

#### Scenario: Plaintext is not logged during enqueue

- **WHEN** a capture is enqueued
- **THEN** no call in the enqueue/encrypt/outbox path writes the plaintext text, payload object, or plaintext JSON to `console.log`, `console.warn`, `console.error`, breadcrumbs, or local retry metadata

#### Scenario: Direct stringify is rejected in development guard

- **WHEN** development/test code attempts to serialize a bundle payload in the outbox path without `secureSerialize()`
- **THEN** the guard fails fast before the payload can be persisted or uploaded
