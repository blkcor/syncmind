# device-pairing Specification

## Purpose
TBD - created by archiving change the-spine. Update Purpose after archive.
## Requirements
### Requirement: Desktop initiates a pairing session
The system SHALL allow a desktop device to initiate a cryptographic pairing session and receive a session identifier together with a QR-code payload.

#### Scenario: Successful pairing session initiation
- **WHEN** a registered desktop device sends a POST request to `/v1/pairing/initiate` containing its ephemeral X25519 public key
- **THEN** the system creates a `pairing_sessions` record with status `pending` and a TTL of 5 minutes
- **AND** the system returns a JSON response containing `session_id` and `qr_payload`
- **AND** the `qr_payload` encodes `spine://pair/{session_id}?pk={base64url_initiator_pubkey}`
- **AND** the system generates a 6-digit short code as a manual fallback

### Requirement: Mobile completes pairing via QR scan
The system SHALL allow a mobile device to complete a pairing session by submitting its ephemeral X25519 public key.

#### Scenario: Mobile completes pairing successfully
- **WHEN** a mobile device scans the QR payload and sends a POST request to `/v1/pairing/complete` with `session_id` and its X25519 public key
- **THEN** the system validates that the session exists, is not expired, and has status `pending`
- **AND** the system stores the mobile public key in the pairing session and updates status to `completed`
- **AND** the system registers both devices in the `devices` table with mutual `paired_device_id` references

#### Scenario: Pairing session expired
- **WHEN** a mobile device attempts to complete a pairing session whose `expires_at` is in the past
- **THEN** the system returns HTTP 410 Gone with error code `PAIRING_EXPIRED`
- **AND** the system updates the session status to `expired`

#### Scenario: Pairing session already completed
- **WHEN** a mobile device attempts to complete a pairing session whose status is already `completed`
- **THEN** the system returns HTTP 409 Conflict with error code `PAIRING_ALREADY_COMPLETED`

### Requirement: Devices derive shared key locally
The system SHALL NOT derive, store, or have access to the shared symmetric key used for end-to-end encryption.

#### Scenario: Key derivation happens client-side only
- **WHEN** both devices have exchanged ephemeral X25519 public keys through the Spine
- **THEN** each device independently computes `shared_secret = X25519(my_private_key, peer_public_key)`
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

