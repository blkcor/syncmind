# mobile-device-identity Specification

## Purpose
Defines how the mobile app generates, stores, and uses a persistent Ed25519 identity key pair for cryptographic operations throughout the SyncMind ecosystem.

## ADDED Requirements

### Requirement: Mobile device generates Ed25519 key pair on first launch
The system SHALL generate an Ed25519 key pair on the mobile device's first app launch using the `@noble/curves` library, before any pairing or network operations.

#### Scenario: Key pair generated on cold start
- **WHEN** the app launches for the first time (no existing identity in `expo-secure-store`)
- **THEN** the system generates a new Ed25519 key pair using `ed25519.utils.randomPrivateKey()` from `@noble/curves`
- **AND** the system derives the corresponding public key
- **AND** the system computes the SHA-256 fingerprint of the public key, formatted as `sha256:<hex>`
- **AND** the system stores both the private key seed and public key in `expo-secure-store` under the key `device_identity`

#### Scenario: Existing identity loaded on subsequent launches
- **WHEN** the app launches and an identity already exists in `expo-secure-store`
- **THEN** the system loads the stored key material into memory
- **AND** the system exposes the fingerprint and public key through the identity API

### Requirement: Private key MUST NOT be exposed outside the identity module
The system SHALL contain the raw Ed25519 private key seed within the lexical scope of `apps/mobile/src/crypto/identity.ts`. No React component, Zustand store, serialization path, log statement, or error message SHALL ever receive the raw private key bytes.

#### Scenario: `sign()` returns a signature, not the key
- **WHEN** a caller invokes `sign(message)`
- **THEN** the function returns the Ed25519 signature bytes
- **AND** the return value does not contain, encode, or reveal the private key bytes under any condition

#### Scenario: `derive_x25519()` returns a shared secret, not the private key
- **WHEN** a caller invokes `derive_x25519(peer_pubkey)`
- **THEN** the function returns the X25519 shared secret bytes
- **AND** the return value does not contain, encode, or reveal the private key bytes

#### Scenario: console.log does not leak private key bytes
- **WHEN** any code path attempts to `console.log(JSON.stringify(identityModule))` or similar serialization
- **THEN** no `Uint8Array` representing the private key seed is enumerable or stringifiable
- **AND** the private key variable is declared outside the module's exported object

### Requirement: Identity module exposes a pure-function API
The system SHALL expose the following functions from `apps/mobile/src/crypto/identity.ts`:

| Function | Signature | Description |
|---|---|---|
| `ensureIdentity()` | `() => Promise<string>` | Ensure identity exists; return device fingerprint string |
| `getDeviceFingerprint()` | `() => string` | Return cached fingerprint `sha256:<hex>` |
| `getDevicePubkey()` | `() => Uint8Array` | Return cached public key bytes |
| `sign(message)` | `(message: Uint8Array) => Uint8Array` | Return Ed25519 signature |
| `derive_x25519(peer_pub)` | `(peer_pub: Uint8Array) => Uint8Array` | Return X25519 shared secret |
| `device_reset()` | `() => Promise<void>` | Clear identity, unpair, flush queue |

#### Scenario: `ensureIdentity` returns existing fingerprint
- **WHEN** `ensureIdentity()` is called and identity already exists
- **THEN** it returns the cached `sha256:<hex>` fingerprint string without regenerating the key

#### Scenario: `ensureIdentity` generates new key on first call
- **WHEN** `ensureIdentity()` is called and no identity exists
- **THEN** it generates a new Ed25519 key pair
- **AND** persists it to `expo-secure-store`
- **AND** returns the new fingerprint

### Requirement: Biometric protection toggle
The system SHALL support two modes for private key access: `requireAuthentication: false` (default) and `requireAuthentication: true` (biometric prompt required). When the user toggles the setting, the key is re-stored with the new option.

#### Scenario: Default mode does not require biometric
- **WHEN** the identity is first created
- **THEN** `expo-secure-store` stores the key with `requireAuthentication: false`
- **AND** `sign()` and `derive_x25519()` succeed without any biometric prompt

#### Scenario: Enabling biometric protection re-stores the key
- **WHEN** the user enables "Enable Biometric Protection" in settings
- **THEN** the system reads the existing key from `expo-secure-store` (requires biometric for this read)
- **AND** deletes the old entry
- **AND** re-stores the key with `requireAuthentication: true`
- **AND** subsequent reads for signing operations trigger a biometric prompt

#### Scenario: Disabling biometric protection re-stores the key
- **WHEN** the user disables "Enable Biometric Protection" in settings
- **THEN** the system reads the existing key from `expo-secure-store` (requires biometric for this read)
- **AND** deletes the old entry
- **AND** re-stores the key with `requireAuthentication: false`

### Requirement: device_reset clears all sensitive state
The system SHALL provide a `device_reset()` operation that destroys the local identity, initiates unpairing, and clears the outbox queue.

#### Scenario: device_reset clears identity and outbox
- **WHEN** `device_reset()` is called
- **THEN** the system deletes `device_identity` from `expo-secure-store`
- **AND** the system calls the Spine `POST /v1/auth/revoke` endpoint to revoke the current device session
- **AND** the system deletes all records from the local `outbox` table
- **AND** the in-memory identity state (`_privateKey`, `_publicKey`, `_fingerprint`) is nullified

#### Scenario: device_reset is idempotent on second call
- **WHEN** `device_reset()` is called a second time (identity already cleared)
- **THEN** the system does not throw or error
- **AND** the system is in the same "no identity" state as after the first call

### Requirement: Identity persists across app restarts
The system SHALL preserve the device identity across app restarts, including after OS-level background eviction and full app termination.

#### Scenario: Identity survives app restart
- **WHEN** the app is fully terminated and relaunched
- **THEN** `ensureIdentity()` returns the same fingerprint as before the restart
- **AND** `sign(message)` produces signatures verifiable with the same public key

### Requirement: Privacy check unit tests pass
The system SHALL include jest unit tests that verify the "no raw key leak" invariant.

#### Scenario: sign return value does not match private key regex
- **WHEN** `sign(message)` returns a signature
- **THEN** `JSON.stringify(signature)` does not match any regex matching a 32-byte hex-encoded sequence
- **AND** `console.log` interception of `sign()` output does not capture any substring matching the private key hex encoding

#### Scenario: derive_x25519 return value does not match private key regex
- **WHEN** `derive_x25519(peer_pub)` returns a shared secret
- **THEN** the test verifies the return value does not match the private key pattern
