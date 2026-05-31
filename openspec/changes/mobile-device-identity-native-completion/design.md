## Context

US-040 currently sits between two designs:

- the archived OpenSpec change, which stores private key material in `expo-secure-store` and keeps the signing key in JS memory
- the in-progress code, which has already introduced a native module boundary (`SyncMindDeviceIdentity`) but does not yet implement it on iOS or Android

The remaining work is not a new product feature. It is a completion change that makes the code, PRD, and spec all agree on one stricter rule: private key bytes must never be exposed to JS serialization paths.

## Goals / Non-Goals

**Goals**
- Keep the Ed25519 identity key in iOS Keychain / Android Keystore-backed native storage
- Make the native module the only place where private key operations occur
- Preserve the JS facade contract used by the mobile app
- Migrate legacy `device_identity` blobs into the native identity store, then delete the legacy blob
- Persist only non-sensitive identity metadata on the JS side
- Prove the privacy invariant with fresh verification evidence

**Non-Goals**
- No protocol changes for pairing, JWT schema, or sync bundle formats
- No expansion into US-041 or broader mobile auth infrastructure
- No attempt to retrofit the archived change; it remains historical only

## Decisions

### Decision 1: Native module owns all private key operations

`apps/mobile/src/crypto/identity.ts` remains the public JS facade, but it must not generate, hold, or serialize private key bytes. The native module owns:

- identity creation
- identity lookup
- signing
- X25519 derivation
- biometric protection changes
- identity reset
- legacy import

### Decision 2: JS persists metadata only

The JS layer may persist `device_identity_meta` for non-sensitive UI/state restoration. That metadata may contain:

- `fingerprint`
- `publicKeyHex`
- `biometricEnabled`

It must not contain:

- private key bytes
- seed material
- any reversible encoding of the private key

### Decision 3: Legacy migration is one-way and destructive

If the app finds a legacy `device_identity` blob:

1. parse it in JS only for migration
2. call the native `importLegacyIdentity(privateKeyHex)` entrypoint once
3. on success, delete `device_identity`
4. persist only non-sensitive metadata afterward

If migration fails, the app must not silently continue using the legacy blob.

### Decision 4: Biometric state must survive restart

`isAuthenticationRequired()` must reflect the real native configuration after app restart. The implementation must not rely on a JS default or a transient in-memory flag alone.

## Verification Requirements

US-040 cannot be accepted until all of the following are fresh and passing:

- `pnpm --filter mobile test --runInBand`
- `pnpm --filter mobile typecheck`
- `pnpm --filter mobile lint`
- iOS manual verification of create/restart/biometric/reset flows
- Android manual verification of create/restart/biometric/reset flows
