# device-pairing Specification

## Purpose
TBD - created by archiving change the-spine. Update Purpose after archive.
## Requirements
### Requirement: Desktop initiates a pairing session
The system SHALL allow a desktop device to initiate a cryptographic pairing session by submitting its persistent Ed25519 public key together with a client-minted UUIDv4. The Spine SHALL persist the supplied UUID as `devices.id` upon successful pairing and SHALL reject conflicts.

#### Scenario: Successful pairing session initiation
- **WHEN** a desktop device sends a POST request to `/v1/pairing/initiate` containing its Ed25519 public key (`initiator_pubkey`), its client-minted UUIDv4 (`device_uuid`), and its `device_type`
- **THEN** the system creates a `pairing_sessions` record with status `pending` and a TTL of 5 minutes
- **AND** the system returns a JSON response containing `session_id`, `qr_payload`, `short_code`, and `expires_at`
- **AND** the `qr_payload` encodes `spine://pair/{session_id}?pk={base64url_initiator_pubkey}`
- **AND** the system generates a 6-digit short code as a manual fallback

#### Scenario: Missing or malformed device_uuid
- **WHEN** a POST request to `/v1/pairing/initiate` omits `device_uuid` or supplies a value that does not parse as a UUIDv4
- **THEN** the system returns HTTP 400 Bad Request with error code `INVALID_REQUEST`

#### Scenario: device_uuid conflicts with an active device
- **WHEN** a POST request to `/v1/pairing/initiate` supplies a `device_uuid` that already exists in the `devices` table with a DIFFERENT `public_key_fingerprint`
- **THEN** the system returns HTTP 409 Conflict with error code `UUID_CONFLICT`
- **AND** no pairing session is created

#### Scenario: device_uuid matches an existing recovery scenario
- **WHEN** a POST request to `/v1/pairing/initiate` supplies a `device_uuid` that already exists in the `devices` table with the SAME `public_key_fingerprint`
- **THEN** the system treats the request as device recovery and creates the pairing session normally

### Requirement: Mobile completes pairing via QR scan
The system SHALL allow a responder device (initially mobile; in this change also a second desktop for protocol verification) to complete a pairing session by submitting its Ed25519 public key and client-minted UUIDv4. The supplied UUID SHALL become `devices.id` for the responder.

#### Scenario: Responder completes pairing successfully
- **WHEN** a responder device sends a POST request to `/v1/pairing/complete` with `session_id`, its Ed25519 `responder_pubkey`, its client-minted `device_uuid` (UUIDv4), and its `device_type`
- **THEN** the system validates that the session exists, is not expired, and has status `pending`
- **AND** the system stores the responder public key in the pairing session and updates status to `completed`
- **AND** the system inserts a `devices` row for the responder using the supplied `device_uuid` as the primary key
- **AND** the system updates mutual `paired_device_id` references for the initiator and responder

#### Scenario: Responder device_uuid conflict
- **WHEN** the supplied `device_uuid` on `/v1/pairing/complete` already exists in `devices` with a different `public_key_fingerprint`
- **THEN** the system returns HTTP 409 Conflict with error code `UUID_CONFLICT`
- **AND** the pairing session is NOT marked completed

#### Scenario: Pairing session expired
- **WHEN** a responder attempts to complete a pairing session whose `expires_at` is in the past
- **THEN** the system returns HTTP 410 Gone with error code `PAIRING_EXPIRED`
- **AND** the system updates the session status to `expired`

#### Scenario: Pairing session already completed
- **WHEN** a responder attempts to complete a pairing session whose status is already `completed`
- **THEN** the system returns HTTP 409 Conflict with error code `PAIRING_ALREADY_COMPLETED`

### Requirement: Devices derive shared key locally
The system SHALL NOT derive, store, or have access to the shared symmetric key used for end-to-end encryption. Clients SHALL convert Ed25519 keys to Curve25519 using `ed25519_dalek::SigningKey::to_scalar_bytes()` (private side) and `curve25519_dalek::edwards::CompressedEdwardsY::decompress(...).to_montgomery()` (public side), perform X25519 ECDH locally, and derive `sync_key = HKDF-SHA256(shared_secret, salt=session_id, info="syncmind-v1")`.

#### Scenario: Key derivation happens client-side only
- **WHEN** both devices have exchanged Ed25519 public keys through the Spine and the session reaches `completed`
- **THEN** each device independently converts its Ed25519 private key to a Curve25519 scalar via `to_scalar_bytes()`
- **AND** each device independently converts the peer's Ed25519 public key to a Curve25519 point via `CompressedEdwardsY::decompress(...).to_montgomery()`
- **AND** each device independently computes `shared_secret = x25519(local_x25519_priv, peer_x25519_pub)`
- **AND** each device independently derives `sync_key = HKDF-SHA256(shared_secret, salt=session_id, info="syncmind-v1")`
- **AND** the Spine database contains no columns or logs storing `shared_secret` or `sync_key`

### Requirement: Pairing status polling
The system SHALL expose a polling endpoint for pairing status when WebSocket is unavailable.

#### Scenario: Poll pending pairing status
- **WHEN** a device sends a GET request to `/v1/pairing/{session_id}/status`
- **THEN** the system returns the current session status (`pending`, `completed`, `expired`, or `cancelled`)
- **AND** if the status is `completed`, the response includes the paired device identifier

### Requirement: Expired pairing session cleanup
The system SHALL automatically invalidate and clean up expired pairing sessions.

#### Scenario: Cleanup job removes expired sessions
- **WHEN** a scheduled cleanup job runs
- **THEN** the system updates all `pairing_sessions` records with `expires_at < NOW()` and status `pending` to status `expired`
- **AND** the system hard-deletes `pairing_sessions` records with status `expired` and `created_at < NOW() - INTERVAL '24 hours'`

