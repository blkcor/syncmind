# device-pairing Specification

## ADDED Requirements

### Requirement: Mobile pairing completion includes device identity
The system SHALL require the mobile device to include its persistent Ed25519 identity public key when completing a pairing session.

#### Scenario: Mobile pairing payload includes identity public key
- **WHEN** a mobile device sends a POST request to `/v1/pairing/complete`
- **THEN** the request body includes the mobile device's Ed25519 public key in addition to the ephemeral X25519 key
- **AND** the Spine stores the Ed25519 public key in the `devices.public_key` field
- **AND** the `device_a_fingerprint` (SHA-256 of the Ed25519 public key, prefixed with `sha256:`) is used as the mobile device's persistent identifier

#### Scenario: Pairing fails if mobile identity is missing
- **WHEN** a mobile device sends a POST request to `/v1/pairing/complete` without an Ed25519 public key in the body
- **THEN** the Spine returns HTTP 400 Bad Request with error code `MOBILE_IDENTITY_REQUIRED`
