## ADDED Requirements

### Requirement: Device identity key registration
The system SHALL associate each paired device with a persistent Ed25519 identity public key.

#### Scenario: Register identity key during pairing
- **WHEN** a pairing session transitions to `completed`
- **THEN** each device submits its Ed25519 public key to the Spine
- **AND** the system stores the public key in `devices.public_key` indexed by `public_key_fingerprint` (SHA-256)
- **AND** the system marks the device as `is_active = TRUE`

### Requirement: JWT authentication for all endpoints
The system SHALL reject any request to protected endpoints that does not carry a valid Ed25519-signed JWT.

#### Scenario: Valid JWT grants access
- **WHEN** a device sends an HTTP request with an `Authorization: Bearer <jwt>` header
- **AND** the JWT contains a `sub` claim matching a registered device ID
- **AND** the JWT contains `iat`, `exp` (≤ 24h from issuance), and `jti` claims
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
