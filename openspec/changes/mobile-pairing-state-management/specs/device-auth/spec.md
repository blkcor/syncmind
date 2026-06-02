## MODIFIED Requirements

### Requirement: JWT authentication for all endpoints
The system SHALL reject any request to protected endpoints that does not carry a valid Ed25519-signed JWT. The JWT `sub` claim SHALL be the client-supplied `device_uuid` recorded as `devices.id` during pairing. The canonical protected Spine request contract SHALL use Spine as the token issuer and the device as the intended audience.

#### Scenario: Valid JWT grants access
- **WHEN** a device sends an HTTP request with an `Authorization: Bearer <jwt>` header
- **AND** the JWT contains a `sub` claim matching a registered device ID (the UUID supplied by the client at pairing time)
- **AND** the JWT contains `iat`, `exp` (≤ 24h from issuance), and `jti` claims
- **AND** the JWT `iss` claim is `"syncmind-spine"` and `aud` is `"syncmind-device"`
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
