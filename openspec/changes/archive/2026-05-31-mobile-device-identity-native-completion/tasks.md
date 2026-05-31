## 1. OpenSpec And PRD Alignment

- [x] 1.1 Update the replacement change artifacts for the native-backed US-040 completion path
- [x] 1.2 Update `docs/prd/005-mobile-capture.md` to point US-040 at the new completion change and explain that the archived 2026-05-27 change is historical only

## 2. Native Identity Module Completion

- [x] 2.1 Implement the iOS `SyncMindDeviceIdentity` module with real Keychain-backed identity creation, lookup, signing, biometric protection, reset, and legacy import
- [x] 2.2 Implement the Android `SyncMindDeviceIdentity` module with real Keystore-backed identity creation, lookup, signing, biometric protection, reset, and legacy import
- [x] 2.3 Ensure both platform implementations expose consistent metadata (`fingerprint`, `publicKeyHex`, `biometricEnabled`) without exposing private key bytes

## 3. JS Facade And Migration

- [x] 3.1 Keep `apps/mobile/src/crypto/identity.ts` as the public facade and remove any remaining private-key-in-JS behavior
- [x] 3.2 Keep only `device_identity_meta` on the JS side and ensure it never stores sensitive key material
- [x] 3.3 Finish one-way migration from legacy `device_identity` blobs into the native identity store
- [x] 3.4 Ensure biometric state and identity metadata restore correctly after restart

## 4. Verification

- [x] 4.1 Update and pass Jest coverage for privacy, migration, restart persistence, and biometric state restoration
- [x] 4.2 Run `pnpm --filter mobile typecheck`
- [x] 4.3 Run `pnpm --filter mobile lint`
- [x] 4.4 Run `pnpm --filter mobile test --runInBand`
- [ ] 4.5 Record iOS and Android manual verification for identity creation, restart persistence, biometric toggle, and reset

Verification evidence from 2026-05-30:
- `CI=true pnpm --config.registry=https://registry.npmjs.org --filter mobile typecheck`
- `CI=true pnpm --config.registry=https://registry.npmjs.org --filter mobile lint`
- `CI=true pnpm --config.registry=https://registry.npmjs.org --filter mobile test --runInBand` (3 suites, 35 tests)
