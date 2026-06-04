## 1. Session Schema Extension

- [x] 1.1 Add `lastSeenAt: string | null` field to `PersistedPairingState` interface in `spine/session.ts`
- [x] 1.2 Add `syncmind.pairing.last_seen_at` key to `PAIRING_KEYS` constant
- [x] 1.3 Update `persistPairingState()` to write `lastSeenAt` (or `"null"` string)
- [x] 1.4 Update `restorePairingState()` to read and restore `lastSeenAt`
- [x] 1.5 Update `clearPairingState()` to remove `last_seen_at` key
- [x] 1.6 Implement `updateLastSeenAt()` helper — updates in-memory + throttled secure-store write (max every 30s)
- [x] 1.7 Implement `getLastSeenAt()` accessor returning `string | null`

## 2. Spine Client: Endpoint Alignment & 401/404 Interception

- [x] 2.1 Implement mobile Ed25519 JWT creation in `spine/client.ts` using native `sign()`:
  - Claims: `sub=selfDeviceUuid`, `iat`, `exp=iat+24h`, `jti`, `iss="syncmind-spine"`, `aud="syncmind-device"`
  - Sign with Ed25519 identity; do NOT use `sync_key` as an HTTP auth credential
- [x] 2.2 Change `revokeCurrentDevice()` endpoint from `POST /v1/auth/revoke` to `POST /v1/devices/{self_device_uuid}/revoke` with body `{ device_uuid }`
- [x] 2.3 Implement `authenticatedFetch(url, options)` wrapper in `spine/client.ts`:
  - Inject `Authorization: Bearer <ed25519_jwt>`
  - On 2xx: call `updateLastSeenAt()` before returning response
  - On 401: call `clearPairingState()` + `setUnpaired()` + throw `UnpairedError`
  - On 404: only auto-unpair for `GET /v1/devices/{self}` / `POST /v1/devices/{self}/revoke` or explicit `DEVICE_REVOKED` / `DEVICE_NOT_FOUND` error codes
  - On generic resource 404: do NOT clear pairing state; return/throw the caller's normal HTTP error path
  - On network error: pass through unchanged (caller handles retry)
- [x] 2.4 Export `UnpairedError` class for callers to distinguish from other fetch errors
- [x] 2.5 Wire `revokeCurrentDevice()` to use `authenticatedFetch` in a mode where revoke 401/qualifying 404 are treated as acceptable stale-pairing outcomes

## 3. Spine Server: Self-Device Status & Revoke

- [x] 3.1 Add `services/sync-gateway/internal/handler/device.go` with authenticated self-device handlers:
  - `GET /v1/devices/{self_device_uuid}` returns self status only when path UUID equals JWT `sub`
  - `POST /v1/devices/{self_device_uuid}/revoke` deactivates self and clears peer `paired_device_id`
  - Unknown/inactive self returns 404 with `DEVICE_NOT_FOUND` or `DEVICE_REVOKED`
- [x] 3.2 Register device routes in `services/sync-gateway/cmd/spine/main.go` behind `authMW`
- [x] 3.3 Add `DeviceStore` helper for clearing paired links that point at a revoked device

## 4. Lightweight Unpair Flow

- [x] 4.1 Implement in-flight Spine request/upload abort support for the current pairing
- [x] 4.2 Implement `unpair()` in `crypto/identity.ts`:
  - Call `revokeCurrentDevice()` and capture non-401/non-qualifying-404 network/server failures as a typed warning result
  - Abort in-flight Spine requests/uploads for the current pairing before completing local cleanup
  - Call `clearPairingState()` to remove all pairing keys from secure-store
  - Call `clearOutbox()` to flush pending queue
  - Call `useAppStore.getState().setUnpaired()`
  - Do NOT call `clearIdentity()` — Ed25519 identity is preserved
- [x] 4.3 Ensure `device_reset()` is NOT modified (still calls revoke + clearPairing + clearOutbox + setUnpaired + clearIdentity)

## 5. Settings UI: Paired Desktop Card

- [x] 5.1 Add `fingerprintShort()` display helper (first 8 + `…` + last 4 of hex after `sha256:`)
- [x] 5.2 Add `relativeTime()` display helper (e.g., "2 hours ago", "3 days ago") for `pairedAt`
- [x] 5.3 Add "Paired Desktop" card to `settings.tsx`:
  - Show when `isPaired === true` (read from Zustand + secure-store restored state)
  - Display: peer fingerprint (truncated, selectable), device_type, paired_at relative time, Spine URL, last successful contact time (`lastSeenAt`)
  - "Unpair" button (destructive style) with confirmation AlertDialog
- [x] 5.4 On "Unpair" confirmation → call `unpair()` → refresh UI state
- [x] 5.5 Handle `unpair()` revoke warning: if remote revoke fails with network/server error, show alert but keep local cleanup completed
- [x] 5.6 Keep existing "Reset Device Identity" in Danger Zone card; update its description to clarify it also destroys identity (nuclear option)

## 6. Tab Bar Gray-out When Unpaired

- [x] 6.1 In `(tabs)/_layout.tsx`, read `useAppStore().isPaired`
- [x] 6.2 When `isPaired === false`:
  - Keep Capture tab navigable as the pairing entry
  - Disable search/list or the current paired-only placeholder tab navigation (href → null or onPress override)
  - Visual: reduce opacity to 0.4, show lock icon next to label
- [x] 6.3 When `isPaired === true`: normal tab bar appearance and navigation

## 7. Unpaired Empty State in Capture/Search Screens

- [x] 7.1 In captures screen, when `isPaired === false`, keep the QR pairing scanner or show centered message: "Pair with a desktop to start capturing" with an in-screen pairing action
- [x] 7.2 In search/list screen, when `isPaired === false`, show centered message: "Pair with a desktop to search your knowledge" with a "Go to Settings" button

## 8. App Startup: Session Restoration with 401/404 Awareness

- [x] 8.1 Ensure `restorePairingState()` is called on app startup (already wired in US-041)
- [x] 8.2 Add a startup self-device health check via `GET /v1/devices/{selfDeviceUuid}` using `authenticatedFetch`
- [x] 8.3 If health-check returns 401 or qualifying self-device 404, `authenticatedFetch` auto-clears state → app enters unpaired without user action
- [x] 8.4 If health-check succeeds, update `lastSeenAt` and set `connectionStatus: "connected"`

## 9. Tests

- [x] 9.1 Create `apps/mobile/__tests__/pairing-state.test.ts`
- [x] 9.2 Test `fingerprintShort()` — `sha256:abcdefghijklmnop…wxyz` → `sha256:abcdefgh…wxyz`
- [x] 9.3 Test `relativeTime()` — known timestamps produce correct human-readable strings
- [x] 9.4 Test in-flight Spine request/upload abort during unpair
- [x] 9.5 Test `unpair()` flow: calls revoke endpoint, clears secure-store pairing keys, clears outbox, updates Zustand store, does NOT clear identity keys
- [x] 9.6 Test `authenticatedFetch`:
  - 2xx: response passes through, `lastSeenAt` updated
  - 401: pairing state cleared, `setUnpaired()` called, `UnpairedError` thrown
  - self-device 404 / `DEVICE_REVOKED`: pairing state cleared, `setUnpaired()` called, `UnpairedError` thrown
  - generic resource 404: pairing state NOT cleared
  - Network error: passes through, pairing state NOT cleared
- [x] 9.7 Test Ed25519 JWT injection: `Authorization: Bearer <jwt>` contains `sub`, `iss`, `aud`, `exp`, and is signed via native `sign()`
- [x] 9.8 Test `updateLastSeenAt` throttle (rapid calls within 30s only write once to secure-store)
- [x] 9.9 Test `PersistedPairingState` round-trip with `lastSeenAt` field
- [x] 9.10 Test legacy US-041 persisted pairing state without `last_seen_at` restores with `lastSeenAt: null`
- [x] 9.11 Test `device_reset()` still clears identity (regression guard)
- [x] 9.12 Add Go tests for `GET /v1/devices/{self}` and `POST /v1/devices/{self}/revoke`:
  - self status returns 200 for JWT `sub`
  - path/JWT mismatch returns 403 or 404
  - revoke deactivates self and clears peer link
  - unknown/inactive self returns `DEVICE_NOT_FOUND` or `DEVICE_REVOKED`

## 10. Verification

- [x] 10.1 Run `pnpm --filter mobile typecheck` — fix any type errors
- [x] 10.2 Run `pnpm --filter mobile lint` — fix any lint violations
- [x] 10.3 Run `pnpm --filter mobile test --runInBand` — all tests pass
- [x] 10.4 Run `cd services/sync-gateway && go test ./...` — all Spine tests pass
- [x] 10.5 Manual smoke: pair with desktop → verify Settings shows peer info → tap Unpair → confirm → verify back to unpaired state → verify identity still exists (re-pair without re-generating identity)
- [x] 10.6 Manual smoke: pair → revoke device on Spine → mobile self-device health check returns qualifying 404 → verify mobile auto-enters unpaired state
