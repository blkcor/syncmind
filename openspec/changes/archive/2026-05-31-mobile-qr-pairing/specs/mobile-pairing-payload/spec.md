## MODIFIED Requirements

### Requirement: Desktop emits versioned JSON QR payload for mobile scanners

The desktop client SHALL produce, in the `spine_pairing_initiate` Tauri command response, a QR payload encoded as a UTF-8 JSON object conforming to the corrected v1 schema defined below. The same JSON string SHALL be both rendered into the QR PNG image and returned to the frontend as a separate string field.

#### Scenario: Successful initiation produces corrected v1 JSON payload

- **WHEN** the desktop user clicks "Start pairing" in the Devices Tab while a valid `Config.spine.url` is configured
- **THEN** the Tauri command response includes a field `qr_payload_json` whose value is a JSON-serialized object with fields `v: 1`, `kind: "syncmind-pairing"`, `session_id`, `spine_url`, `ca_fingerprint` (string or null), `pairing_token`, `expires_at`, `device_a_pubkey`, and `device_a_fingerprint`
- **AND** `session_id` equals the Spine `pairing_initiate` response `session_id`
- **AND** the `qr_png_base64` field encodes a PNG that, when decoded by a QR scanner, yields exactly the same UTF-8 string as `qr_payload_json`
- **AND** all fields in `qr_payload_json` are non-empty strings except `ca_fingerprint`, which MAY be JSON `null` when no self-signed CA is configured

#### Scenario: `session_id` is the only pairing completion locator in the QR payload

- **WHEN** a mobile scanner consumes the corrected v1 JSON payload
- **THEN** it uses `session_id` as the `/v1/pairing/complete` `session_id`
- **AND** it treats `pairing_token` as opaque/legacy credential material that MUST NOT be submitted as `session_id`

#### Scenario: Public key encodings are base64url without padding

- **WHEN** the JSON payload is emitted
- **THEN** `device_a_pubkey` is base64url-no-pad encoded raw Ed25519 public key bytes
- **AND** any public keys echoed by Spine pairing responses use the same base64url-no-pad encoding
