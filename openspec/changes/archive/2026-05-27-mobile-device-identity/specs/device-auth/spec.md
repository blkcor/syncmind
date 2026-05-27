# device-auth Specification

## MODIFIED Requirements

### Requirement: Device identity key registration
The system SHALL associate each paired device with a persistent Ed25519 identity public key. Mobile devices generate their identity key locally prior to pairing initiation and submit the public key during the pairing completion handshake.

#### Scenario: Register mobile device identity key during pairing
- **WHEN** a mobile device completes a pairing session
- **THEN** the mobile device submits its locally-generated Ed25519 public key to the Spine
- **AND** the system stores the public key in `devices.public_key` indexed by `public_key_fingerprint` (SHA-256)
- **AND** the system marks the device as `is_active = TRUE`

## ADDED Requirements

### Requirement: Mobile generates identity key locally
The system SHALL generate the Ed25519 identity key pair on the mobile device, not on the Spine or desktop. The private key never leaves the mobile device.

#### Scenario: Identity key generated on mobile, not server
- **WHEN** a mobile device first launches
- **THEN** it generates its own Ed25519 key pair using `@noble/curves`
- **AND** the private key is stored only in `expo-secure-store` (iOS Keychain / Android Keystore)
- **AND** the Spine never receives or stores the private key
