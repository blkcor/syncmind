## 1. Protocol Alignment Prerequisite

- [x] 1.1 Update `apps/desktop/src-tauri/src/spine/pairing.rs` `MobilePairingPayload` to include required `session_id`
- [x] 1.2 Update `openspec/specs/mobile-pairing-payload/spec.md` via this change's `mobile-pairing-payload` delta so v1 JSON explicitly includes `session_id`
- [x] 1.3 Add/adjust desktop unit tests proving `qr_payload_json` contains the same `session_id` returned by Spine `pairing_initiate`

## 2. Dependencies & Module Scaffold

- [x] 2.1 Add `expo-camera`, `@noble/curves`, and `@noble/hashes` to `apps/mobile/package.json`
- [x] 2.2 Run `pnpm install` to resolve and lock new dependencies
- [x] 2.3 Create `apps/mobile/src/pairing/` directory with placeholder `index.ts`

## 3. QR Payload Validation (`pairing/payload.ts`)

- [x] 3.1 Define `PairingPayload` TypeScript interface matching corrected v1 schema: `v`, `kind`, `session_id`, `spine_url`, `ca_fingerprint`, `pairing_token`, `expires_at`, `device_a_pubkey`, `device_a_fingerprint`
- [x] 3.2 Implement `parsePairingPayload(input: string): PairingPayload` — trim input, parse raw JSON payload, and return typed errors for malformed JSON
- [x] 3.3 Implement `validatePairingPayload(payload: PairingPayload): ValidationError | null` — check `v==1`, `kind`, UUIDv4 `session_id`, `expires_at` (±60s clock skew), `spine_url` https (production only), base64url-no-pad `device_a_pubkey`, and SHA-256 fingerprint match
- [x] 3.4 Reject v1 payloads missing `session_id` with a desktop-upgrade error; do not fall back to `pairing_token` as a session locator
- [x] 3.5 Export user-readable error message mapping from `ValidationError` variants

## 4. QR Scanner UI (`pairing/scanner.tsx`)

- [x] 4.1 Implement `PairingScanner` component using `expo-camera` `CameraView` with `barcodeScannerSettings`
- [x] 4.2 Implement camera permission request flow with `useCameraPermissions()`
- [x] 4.3 Implement permission-denied fallback UI (multiline text input for raw JSON pairing payload)
- [x] 4.4 Implement "Open Settings" deep link for permanently blocked permission
- [x] 4.5 Wire scanned/typed payload through `parsePairingPayload` + `validatePairingPayload` and display errors
- [x] 4.6 Configure native iOS camera usage description through `expo-camera` config plugin without requesting microphone access

## 5. Device UUID (`pairing/device.ts`)

- [x] 5.1 Implement `ensureMobileDeviceUuid(): Promise<string>` generating UUIDv4 once and storing it in `expo-secure-store`
- [x] 5.2 Implement `getMobileDeviceUuid()` / `clearMobileDeviceUuid()` helpers for session restore and device reset
- [x] 5.3 Ensure pairing completion and future JWT minting use this same UUID as the Spine `devices.id` / JWT `sub`

## 6. Pairing Handshake (`pairing/handshake.ts`)

- [x] 6.1 Implement base64url-no-pad decode helpers for QR payload pubkeys and Spine response pubkeys
- [x] 6.2 Implement Ed25519 public key → X25519 public key conversion for `device_a_pubkey` using `@noble/curves` or a native helper
- [x] 6.3 Implement `completePairing(payload, selfDeviceUuid, identityPubkey)` — POST `{ session_id, device_uuid, responder_pubkey, device_type: "mobile" }` to `/v1/pairing/complete`
- [x] 6.4 Parse successful Spine response fields `status`, `session_id`, `initiator_id`, `responder_id`, and `initiator_pubkey`
- [x] 6.5 Implement `deriveSyncKey(peerX25519Pubkey, sessionId)` — call native `derive_x25519(peerX25519Pubkey)` for shared secret, then HKDF-SHA256 with `@noble/hashes/hkdf`
- [x] 6.6 Verify `initiator_pubkey` in the Spine response matches QR payload `device_a_pubkey` when present

## 7. Session Persistence (`spine/session.ts` upgrade)

- [x] 7.1 Define `PersistedPairingState` interface with all fields from spec §"Pairing state persistence"
- [x] 7.2 Implement `persistPairingState(state: PersistedPairingState)` writing each field to `expo-secure-store`
- [x] 7.3 Implement `restorePairingState(): Promise<PersistedPairingState | null>` reading all required fields on startup
- [x] 7.4 Implement `clearPairingState()` removing all pairing keys from `expo-secure-store`
- [x] 7.5 Remove in-memory-only `SpineSession` model; replace with restored persisted state

## 8. Pairing Flow Orchestrator (`pairing/index.ts`)

- [x] 8.1 Implement `startPairingFlow(payload: PairingPayload): Promise<void>` — full sequence: ensure identity + device UUID → complete handshake → derive sync_key → persist → update store
- [x] 8.2 Wire scanner → orchestrator: call `startPairingFlow` on successful validation
- [x] 8.3 Handle error states from each step with user-readable messages and retry pathways

## 9. Store & Navigation Integration

- [x] 9.1 Add `isFirstPairing: boolean` field to `AppState` in `store.ts`
- [x] 9.2 Implement `useAppStore().setPaired(fingerprint)` update after successful pairing persistence, including `connectionStatus: "connected"`
- [x] 9.3 Implement post-pairing navigation: first pairing → capture screen + intro overlay; re-pairing → capture screen directly
- [x] 9.4 Wire `restorePairingState()` call into app startup (root `_layout.tsx` or equivalent)

## 10. CA Fingerprint Metadata (`pairing/tls-check.ts`)

- [x] 10.1 Implement `validateCAFingerprintFormat(expected: string): boolean` for `sha256:<lower-or-upper-hex>` values
- [x] 10.2 Persist `ca_fingerprint` with pairing state when present
- [x] 10.3 If a platform certificate-chain API is available, compare the presented certificate SHA-256 fingerprint and fail closed on mismatch
- [x] 10.4 If certificate-chain access is unavailable, do not claim TLS pinning; proceed under system trust only and log a warning

## 11. Tests

- [x] 11.1 Create `apps/mobile/__tests__/pairing.test.ts`
- [x] 11.2 Test `parsePairingPayload` — valid payload, malformed JSON, missing `session_id`, missing fields, wrong `kind`
- [x] 11.3 Test `validatePairingPayload` — expired (boundary: +59s, -61s), wrong version, invalid UUID, fingerprint mismatch, http in production, http in dev
- [x] 11.4 Test `completePairing` request body uses `session_id`, `device_uuid`, `responder_pubkey`, and `device_type`, with no X25519 ephemeral field
- [x] 11.5 Test `deriveSyncKey` — produces 32-byte key, deterministic given same inputs, matches a desktop/Rust golden vector
- [x] 11.6 Test `persistPairingState` / `restorePairingState` round-trip using `expo-secure-store` mock
- [x] 11.7 Test `clearPairingState` removes all keys

## 12. Verification

- [x] 12.1 Run `pnpm --filter mobile typecheck` and fix any type errors
- [x] 12.2 Run `pnpm --filter mobile lint` and fix any lint violations
- [x] 12.3 Run `pnpm --filter mobile test --runInBand` and ensure all tests pass
- [x] 12.4 Run relevant desktop Rust tests for `MobilePairingPayload` session_id emission
- [x] 12.5 Manual smoke: build dev client, scan a QR from desktop Devices panel, verify paired state persists across app restart
