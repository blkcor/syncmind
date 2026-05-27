## Why

Phase 4 (Mobile Capture) introduces the SyncMind mobile app as a lightweight capture client. Before any capture or pairing can happen, the mobile device needs a persistent cryptographic identity — an Ed25519 key pair stored in the OS secure key store. This is the foundational capability for all subsequent Phase 4 user stories (US-041 US-043~US-051): without a device identity, the mobile app cannot sign requests, establish pairing sessions, or encrypt bundles for the Spine relay.

## What Changes

- Add `apps/mobile/src/crypto/identity.ts` as the sole API surface for device identity operations
- Implement Ed25519 key pair generation via `@noble/curves` (pure JS, Hermes-compatible)
- Persist keys via `expo-secure-store` (iOS Keychain / Android Keystore)
- Add `sign(message)` and `derive_x25519(peer_pub)` operations via a pure-function API that never exposes raw private key bytes
- Add `device_reset()` to clear identity + unpair + clear queue
- Add biometric protection toggle in settings (off by default, `requireAuthentication: true` opt-in)
- Add unit tests for privacy guarantees (no raw key leak in logs, error messages, or JSON output)
- **No changes** to `core/`, `services/`, `apps/desktop/`, or existing `packages/`

## Capabilities

### New Capabilities
- `mobile-device-identity`: Persistent Ed25519 identity key pair generation, secure storage, signing, and derivation operations for the mobile app.

### Modified Capabilities

- `device-auth`: Mobile-side identity registration flow extends the existing Spine-level device-auth contract — the mobile device generates its own Ed25519 key pair locally (rather than receiving one from Spine), and registers its public key during the pairing handshake.
- `device-pairing`: The mobile pairing flow now depends on a pre-existing local identity; the pairing completion payload must include the mobile device's Ed25519 public key.

## Impact

- **New dependency**: `@noble/curves` and `@noble/hashes` in `apps/mobile/package.json`
- **New dependency**: `expo-secure-store` in `apps/mobile/package.json`
- **New dependency**: `expo-crypto` in `apps/mobile/package.json`
- **New module**: `apps/mobile/src/crypto/identity.ts` — pure-function identity API
- **New tests**: `apps/mobile/__tests__/crypto.test.ts` — privacy guarantee unit tests
- **No changes** to Rust core, Go service, or desktop app source code
