# mobile-device-identity Specification

## MODIFIED Requirements

### Requirement: Mobile device identity is stored and used through a native secure-store boundary
The system SHALL keep the Ed25519 device identity private key inside a native iOS Keychain / Android Keystore-backed implementation. The JS layer SHALL NOT generate, serialize, persist, or retain the raw private key bytes after migration completes.

#### Scenario: Identity generated on first launch through native module
- **WHEN** the app launches with no existing identity
- **THEN** the native `SyncMindDeviceIdentity` module generates the device identity
- **AND** the JS layer receives only non-sensitive metadata (`fingerprint`, `publicKeyHex`, `biometricEnabled`)
- **AND** no JS persistence entry contains the raw private key or seed

#### Scenario: Existing identity loaded on subsequent launches
- **WHEN** the app launches and the native identity already exists
- **THEN** `ensureIdentity()` returns the existing fingerprint
- **AND** `getDevicePubkey()` returns the existing public key
- **AND** `isAuthenticationRequired()` reflects the native biometric configuration

### Requirement: Identity operations occur through the native module only
The system SHALL route signing and key derivation through the native module. `apps/mobile/src/crypto/identity.ts` remains the public facade, but the underlying private key operations SHALL execute only in the native implementation.

#### Scenario: `sign()` returns signature bytes from native implementation
- **WHEN** a caller invokes `sign(message)`
- **THEN** the native module performs the signature operation
- **AND** the JS layer receives only the signature bytes
- **AND** the private key bytes are not exposed through the bridge

#### Scenario: `derive_x25519()` returns shared secret bytes from native implementation
- **WHEN** a caller invokes `derive_x25519(peer_pubkey)`
- **THEN** the native module performs the derivation
- **AND** the JS layer receives only the derived shared secret bytes
- **AND** the private key bytes are not exposed through the bridge

### Requirement: Legacy JS-stored identities are migrated and removed
The system SHALL support one-way migration from the legacy `device_identity` blob into the native identity store, then delete the legacy blob.

#### Scenario: Legacy blob migrated on first launch after upgrade
- **WHEN** the app finds a legacy `device_identity` entry containing private key material
- **THEN** the JS layer invokes the native legacy-import path once
- **AND** on success the app deletes `device_identity`
- **AND** subsequent launches use the native identity only

#### Scenario: Migration failure blocks continued legacy use
- **WHEN** legacy import fails
- **THEN** the app does not silently continue using the legacy JS-stored key material

### Requirement: JS persistence is limited to non-sensitive metadata
The system MAY persist `device_identity_meta` for UI and restart restoration, but that metadata SHALL contain only non-sensitive values.

#### Scenario: Metadata persisted without private key material
- **WHEN** the app stores device identity metadata on the JS side
- **THEN** the stored value may include `fingerprint`, `publicKeyHex`, and `biometricEnabled`
- **AND** it SHALL NOT include private key bytes, seed material, or any reversible private key encoding

### Requirement: Privacy verification covers serialization, logging, and errors
The system SHALL include tests proving that private key material does not leak through JS persistence, logging, JSON serialization, or error messages.

#### Scenario: No private key leak through persistence and logs
- **WHEN** the identity module is exercised by unit tests
- **THEN** no `expo-secure-store` write contains private key material
- **AND** no `console.log` capture includes private key material
- **AND** no `JSON.stringify(...)` result includes private key material
- **AND** no thrown `Error.message` includes private key material
