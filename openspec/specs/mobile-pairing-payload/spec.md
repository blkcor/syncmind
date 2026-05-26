# mobile-pairing-payload Specification

## Purpose
TBD - created by archiving change desktop-spine-pairing-payload. Update Purpose after archive.
## Requirements
### Requirement: Desktop emits versioned JSON QR payload for mobile scanners

The desktop client SHALL produce, in the `spine_pairing_initiate` Tauri command response, a QR payload encoded as a UTF-8 JSON object conforming to the v1 schema defined below. The same JSON string SHALL be both rendered into the QR PNG image and returned to the frontend as a separate string field.

#### Scenario: Successful initiation produces v1 JSON payload

- **WHEN** the desktop user clicks "Start pairing" in the Devices Tab while a valid `Config.spine.url` is configured
- **THEN** the Tauri command response includes a field `qr_payload_json` whose value is a JSON-serialized object with fields `v: 1`, `kind: "syncmind-pairing"`, `spine_url`, `ca_fingerprint` (string or null), `pairing_token`, `expires_at`, `device_a_pubkey`, and `device_a_fingerprint`
- **AND** the `qr_png_base64` field encodes a PNG that, when decoded by a QR scanner, yields exactly the same UTF-8 string as `qr_payload_json`
- **AND** all fields in `qr_payload_json` are non-empty strings except `ca_fingerprint`, which MAY be JSON `null` when no self-signed CA is configured

#### Scenario: `ca_fingerprint` is null when no self-signed CA is configured

- **WHEN** `Config.spine.ca_fingerprint` is `None`
- **THEN** the `ca_fingerprint` field in the emitted JSON payload is exactly the JSON literal `null`, not an empty string and not absent from the object

#### Scenario: `device_a_fingerprint` matches SHA-256 of `device_a_pubkey`

- **WHEN** the JSON payload is emitted
- **THEN** `device_a_fingerprint` equals the lowercase hex encoding of `SHA-256(base64_decode(device_a_pubkey))`, prefixed with `sha256:`

#### Scenario: `expires_at` matches Spine server response verbatim

- **WHEN** the Spine `pairing_initiate` endpoint returns an `expires_at` field
- **THEN** the desktop client copies this RFC 3339 UTC string into the JSON payload's `expires_at` field without timezone conversion or reformatting

### Requirement: Desktop parses and validates incoming pairing payloads

The desktop client SHALL provide a parser `parse_mobile_pairing_payload(input: &str)` that accepts both v1 JSON and the legacy `spine://pair/{session_id}?pk={base64url_pubkey}` URI format, and rejects any other input with a typed error.

#### Scenario: Parser accepts valid v1 JSON

- **WHEN** the input is a UTF-8 string that successfully deserializes into the v1 `MobilePairingPayload` struct
- **AND** the `v` field equals `1`
- **AND** the `kind` field equals `"syncmind-pairing"`
- **THEN** the parser returns the populated struct

#### Scenario: Parser rejects unknown schema versions

- **WHEN** the input is a JSON object with `v: 2` or any non-`1` integer
- **THEN** the parser returns `SpineErrorCode::UnsupportedVersion`
- **AND** the error message names the offending version number

#### Scenario: Parser rejects mismatched `kind`

- **WHEN** the input is a JSON object with `v: 1` but `kind != "syncmind-pairing"`
- **THEN** the parser returns `SpineErrorCode::BadRequest`

#### Scenario: Parser accepts legacy URI format

- **WHEN** the input starts with `spine://pair/`
- **AND** the URI path contains a non-empty session identifier
- **AND** the URI query contains exactly the key `pk` with a base64url-encoded value
- **THEN** the parser returns a normalized struct containing the session_id and initiator pubkey, with other fields populated from the desktop's local `Config.spine`

#### Scenario: Parser rejects URI with unknown query parameters

- **WHEN** the input starts with `spine://pair/` but the URI query contains any key other than `pk`
- **THEN** the parser returns `SpineErrorCode::BadRequest`

### Requirement: Pairing payload TTL matches Spine session lifetime

The JSON payload's `expires_at` field SHALL reflect the server-authoritative session expiry. A consumer of the payload SHALL treat the payload as invalid once `expires_at` is in the past, allowing a ±60 second clock skew tolerance.

#### Scenario: Expired payload is rejected by the desktop parser

- **WHEN** `parse_mobile_pairing_payload` is given a v1 JSON payload whose `expires_at` is more than 60 seconds in the past relative to the desktop's local wall clock
- **THEN** the parser returns `SpineErrorCode::PairingExpired`

### Requirement: Pairing token is opaque and bound to one session

The `pairing_token` field SHALL be treated by all consumers as opaque, single-use credential material. The desktop emitter SHALL never reuse the same `pairing_token` across two distinct `pairing_initiate` calls, and SHALL never display the token alongside human-readable labels (e.g. avoid prefixing with "token: " in any UI surface).

#### Scenario: Token is unique per initiation

- **WHEN** the desktop calls `spine_pairing_initiate` twice in succession
- **THEN** the two emitted `pairing_token` values are byte-for-byte different
- **AND** the desktop does not log either token value to the persistent log file

### Requirement: Tauri command exposes the JSON payload alongside the QR image

The Tauri command surface for pairing initiation SHALL preserve the existing `qr_png_base64` field for backward compatibility, while adding a new `qr_payload_json` field carrying the raw JSON string.

#### Scenario: Existing frontend continues to function

- **WHEN** a frontend build compiled against the pre-change Tauri type definitions invokes `spine_pairing_initiate`
- **THEN** the call succeeds and `qr_png_base64` contains a renderable PNG QR code
- **AND** the additional `qr_payload_json` field is present in the response but may be ignored by the frontend without error

#### Scenario: Updated frontend can copy the raw payload

- **WHEN** a frontend reads the `qr_payload_json` field
- **THEN** the value is a valid JSON string identical to the QR-encoded content, suitable for being placed on the system clipboard for manual transcription

