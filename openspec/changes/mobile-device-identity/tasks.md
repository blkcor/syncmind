## 1. Setup & Dependencies

- [x] 1.1 Add `@noble/curves`, `@noble/hashes`, `expo-secure-store`, `expo-crypto` to `apps/mobile/package.json`
- [x] 1.2 Create `apps/mobile/src/crypto/` directory structure

## 2. Identity Module Implementation

- [x] 2.1 Implement `apps/mobile/src/crypto/identity.ts` — Ed25519 key generation via `@noble/curves` on first launch
- [x] 2.2 Implement `expo-secure-store` persistence layer for `device_identity` (private key seed + public key + fingerprint)
- [x] 2.3 Implement `sign(message)` — Ed25519 signing using the in-memory private key
- [x] 2.4 Implement `derive_x25519(peer_pub)` — X25519 shared secret derivation from Ed25519 key pair
- [x] 2.5 Implement `getDeviceFingerprint()` and `getDevicePubkey()` — public information accessors
- [x] 2.6 Implement `ensureIdentity()` — idempotent init that returns fingerprint

## 3. Device Reset & Biometric Toggle

- [x] 3.1 Implement `device_reset()` — clear secure store, unpair via Spine, flush outbox queue
- [x] 3.2 Implement biometric protection toggle — re-store key with `requireAuthentication: true/false`
- [x] 3.3 Wire device settings UI: "Enable Biometric Protection" switch with confirmation dialog for disable

## 4. Privacy & Testing

- [x] 4.1 Write jest unit tests for `identity.ts` — verify `sign()` and `derive_x25519()` do not leak raw key bytes
- [x] 4.2 Write jest tests for identity persistence — key survives app restart (mocked secure store)
- [x] 4.3 Write jest tests for `device_reset()` — clears state, idempotent on re-call
- [x] 4.4 Run `pnpm --filter mobile typecheck` and `pnpm --filter mobile lint` to verify no regressions
