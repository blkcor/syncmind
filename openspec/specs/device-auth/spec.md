# device-auth Specification

## Purpose
TBD - created by archiving change the-spine. Update Purpose after archive.
## Requirements
### Requirement: Device identity key registration
The system SHALL associate each paired device with a persistent Ed25519 identity public key. Mobile devices generate their identity key locally prior to pairing initiation and submit the public key during the pairing completion handshake.

#### Scenario: Register mobile device identity key during pairing
- **WHEN** a mobile device completes a pairing session
- **THEN** the mobile device submits its locally-generated Ed25519 public key to the Spine
- **AND** the system stores the public key in `devices.public_key` indexed by `public_key_fingerprint` (SHA-256)
- **AND** the system marks the device as `is_active = TRUE`

### Requirement: JWT authentication for all endpoints
The system SHALL reject any request to protected endpoints that does not carry a valid Ed25519-signed JWT. The JWT `sub` claim SHALL be the client-supplied `device_uuid` recorded as `devices.id` during pairing.

#### Scenario: Valid JWT grants access
- **WHEN** a device sends an HTTP request with an `Authorization: Bearer <jwt>` header
- **AND** the JWT contains a `sub` claim matching a registered device ID (the UUID supplied by the client at pairing time)
- **AND** the JWT contains `iat`, `exp` (≤ 24h from issuance), and `jti` claims
- **AND** the JWT `iss` claim is `"syncmind-client"` and `aud` is `"syncmind-spine"`
- **AND** the JWT signature verifies against the device's registered Ed25519 public key
- **AND** the JWT has not expired and `jti` has not been used before
- **THEN** the system authenticates the request and sets the request context `device_id`

#### Scenario: Missing authorization header
- **WHEN** a request to a protected endpoint contains no `Authorization` header
- **THEN** the system returns HTTP 401 Unauthorized with error code `AUTH_MISSING`

#### Scenario: Invalid JWT signature
- **WHEN** a request presents a JWT whose Ed25519 signature does not verify against the stored public key for the claimed `sub`
- **THEN** the system returns HTTP 401 Unauthorized with error code `AUTH_INVALID_SIGNATURE`

#### Scenario: Expired JWT
- **WHEN** a request presents a JWT whose `exp` claim is in the past
- **THEN** the system returns HTTP 401 Unauthorized with error code `AUTH_EXPIRED`

#### Scenario: Replayed JWT
- **WHEN** a request presents a JWT whose `jti` has already been recorded in the token blacklist (Redis)
- **THEN** the system returns HTTP 401 Unauthorized with error code `AUTH_REPLAYED`

#### Scenario: sub does not match a registered device
- **WHEN** a request presents a JWT whose `sub` claim does not match any row in the `devices` table
- **THEN** the system returns HTTP 401 Unauthorized with error code `AUTH_INVALID`

### Requirement: WebSocket authentication
The system SHALL authenticate WebSocket upgrade requests using the same JWT mechanism.

#### Scenario: WebSocket connection with valid token
- **WHEN** a client initiates a WebSocket handshake to `/v1/sync/live` with a valid JWT in the `Sec-WebSocket-Protocol` subprotocol or query parameter
- **THEN** the system validates the JWT before completing the WebSocket upgrade
- **AND** upon successful validation, the system associates the WebSocket connection with the authenticated `device_id`

#### Scenario: WebSocket connection with invalid token
- **WHEN** a client initiates a WebSocket handshake with an invalid or missing JWT
- **THEN** the system rejects the handshake with HTTP 401 before upgrading the connection

### Requirement: Device deactivation
The system SHALL support immediate revocation of a device's authentication credentials.

#### Scenario: Deactivate compromised device
- **WHEN** an authenticated device sends a POST request to deactivate itself or its paired device
- **THEN** the system sets `devices.is_active = FALSE` for the target device
- **AND** the system invalidates all outstanding JWTs for that device by blacklisting active `jti` values
- **AND** subsequent requests using that device's JWTs receive HTTP 401 Unauthorized

### Requirement: Mobile generates identity key locally
The system SHALL generate the Ed25519 identity key pair on the mobile device, not on the Spine or desktop. The private key never leaves the mobile device.

#### Scenario: Identity key generated on mobile, not server
- **WHEN** a mobile device first launches
- **THEN** it generates its own Ed25519 key pair using `@noble/curves`
- **AND** the private key is stored only in `expo-secure-store` (iOS Keychain / Android Keystore)
- **AND** the Spine never receives or stores the private key

