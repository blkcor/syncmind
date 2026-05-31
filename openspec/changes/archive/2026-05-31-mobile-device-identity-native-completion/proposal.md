## Why

The archived `mobile-device-identity` change from 2026-05-27 documented a pure-JS identity design that persisted private key material through `expo-secure-store`. The current US-040 acceptance bar is stricter: the private key must remain in the OS secure store and must never be serialized through the JS bridge. The repo also contains a duplicate active copy of the archived change, which no longer reflects the implementation direction.

US-040 is therefore still incomplete. The remaining work is to finish the native-backed identity path, align the docs/specs with that direction, and produce fresh verification evidence.

## What Changes

- Remove the duplicate active change `openspec/changes/mobile-device-identity/`; keep the archived 2026-05-27 record as historical context only
- Define the native-backed completion path for US-040 in a new change
- Replace the JS-held private key model with a native identity module model for iOS Keychain / Android Keystore
- Preserve the existing JS-facing identity facade while moving all private key operations behind the native module
- Define migration from legacy `device_identity` blobs to the native identity store
- Require fresh unit, typecheck, lint, and platform verification before US-040 can be accepted

## Capabilities

### Modified Capabilities

- `mobile-device-identity`: change the storage and execution model from JS-held key material to native-held key material, while preserving the mobile identity API surface

## Impact

- **New native boundary:** `apps/mobile/src/crypto/native-device-identity.ts`
- **New local module implementation:** `apps/mobile/modules/syncmind-device-identity/`
- **Updated mobile facade:** `apps/mobile/src/crypto/identity.ts`
- **Updated tests:** `apps/mobile/__tests__/crypto.test.ts`
- **Updated PRD status:** `docs/prd/005-mobile-capture.md`
