## Context

The mobile app must establish a persistent cryptographic identity before any pairing or capture operations. This identity is an Ed25519 key pair that:

- Is generated **locally on the device** (not received from a server)
- Lives in the OS secure key store (iOS Keychain / Android Keystore)
- Survives app restarts and reinstall-on-top (but not explicit device reset)
- Is used to sign JWTs for Spine authentication and to derive X25519 shared keys for E2EE bundle encryption

The existing `device-auth` and `device-pairing` specs assume the Spine manages device identity registration. US-040 adds the mobile-side identity generation layer — the mobile device creates its own key pair, then presents its public key during pairing.

## Goals / Non-Goals

**Goals:**
- Generate an Ed25519 key pair on first app launch using `@noble/curves` (pure JS, Hermes-compatible)
- Persist the key pair in `expo-secure-store` (iOS Keychain / Android Keystore)
- Expose `sign(message)` and `derive_x25519(peer_pub)` via pure-function API that never leaks raw private key bytes
- Store the public key fingerprint (SHA-256 hex, `sha256:` prefix) alongside the key material for quick identity queries
- Provide `device_reset()` to clear identity, unpair, and flush the outbox queue
- Add biometric protection toggle (off by default; when enabled, `expo-secure-store` sets `requireAuthentication: true`)
- Achieve test coverage for the "no raw key leak" invariant

**Non-Goals:**
- No multi-device key management (single identity per app install)
- No Cloud Keychain / Google Backup sync of identity keys
- No hardware-backed secure enclave key generation (falls back to software Ed25519 via @noble/curves)
- No server-side key revocation for mobile identities (device_reset is a local-only operation; the paired desktop must be re-paired explicitly)

## Decisions

### Decision 1: `@noble/curves` over `react-native-crypto` for Ed25519

The `@noble/curves` library is a pure JS implementation proven to work under Hermes (React Native's JS engine). Alternatives like `react-native-crypto` would require native module linking and are not guaranteed to work in Hermes strict mode without prebuild.

**Trade-off:** Pure JS Ed25519 is ~50% slower than native bindings for key generation (~5ms vs ~2ms on a modern phone), but key generation happens only once (first launch). Signing operations are sub-millisecond. The Hermes compatibility guarantee outweighs the marginal performance difference.

### Decision 2: `expo-secure-store` over bare Keychain/Keystore native modules

The Expo managed workflow provides `expo-secure-store` as a cross-platform wrapper for iOS Keychain (kSecAttrAccessibleWhenUnlockedThisDeviceOnly) and Android Keystore (EncryptedSharedPreferences backed by AndroidKeystore). Using it avoids a prebuild/bare-workflow dependency for Phase 4 MVP.

**Trade-off:** `expo-secure-store` has a per-value size limit (~8KB on older iOS, much larger on Android). The entire Ed25519 key (32 bytes seed + 32 bytes public) + fingerprint (~70 bytes) is well within limits. If future requirements need to store larger blobs, a migration to a custom native module would be needed.

### Decision 3: Private key isolation via closure-based API

The private key bytes are held in a module-level `Uint8Array` inside `identity.ts`. The module exposes only `sign(message)` and `derive_x25519(peer_pub)` — neither function returns the raw key. The key is never passed into React component trees, stores, or serialization paths.

```typescript
// NOT exported — holds the raw seed
let _privateKey: Uint8Array | null = null;

export function getDeviceFingerprint(): string { /* returns SHA-256(publicKey) */ }
export function sign(message: Uint8Array): Uint8Array { /* uses _privateKey internally */ }
```

This mirrors the existing desktop pattern where `core/storage/src/spine/envelope.rs` never logs raw key material.

### Decision 4: `requireAuthentication: false` by default

Setting `requireAuthentication: false` means the private key is accessible without biometric prompt. This is deliberate for MVP because:
- Every capture upload and search query would otherwise trigger FaceID/TouchID, making the app unusable for rapid capture workflows
- The physical device lock screen (PIN/biometric) is the primary security boundary
- A settings toggle allows privacy-conscious users to opt in

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| `expo-secure-store` data loss on iOS Keychain | Keys are scoped to app install; OS backup/restore or full device restore will lose the key. The user must re-pair. Document this as expected behavior. |
| Ed25519 key generation failure on low-memory devices | `@noble/curves` is lightweight; even on entry-level Android devices with 2GB RAM, the ~5ms operation completes without OOM. Add a try/catch with a clear "Device identity generation failed — restart the app" message. |
| `device_reset()` accidentally called and losing identity | The reset requires a two-step confirmation UI dialog. The outbox queue flush writes a final snapshot before deleting, allowing forensic recovery. |
| Biometric toggle race: disabling after enabling doesn't remove `requireAuthentication` | `expo-secure-store` doesn't support changing a stored item's `requireAuthentication` flag. When the user disables biometric protection, we must delete and re-store the key with `requireAuthentication: false`. |
