# mobile-pairing-state-management Specification

## Purpose
TBD - created by archiving change mobile-pairing-state-management. Update Purpose after archive.
## Requirements
### Requirement: Paired Desktop information display in Settings

The Settings screen SHALL display a "Paired Desktop" card when `isPaired === true`, showing the peer device's fingerprint (truncated), device type, pairing timestamp as a relative time string, Spine URL, and the timestamp of the last successful Spine contact.

#### Scenario: Paired desktop card shows all peer fields

- **WHEN** the app is paired (`isPaired === true`)
- **AND** `PersistedPairingState` contains `pairedPeerFingerprint`, `pairedPeerDeviceType`, `pairedAt`, `spineUrl`, and `lastSeenAt`
- **THEN** the Settings screen renders a "Paired Desktop" card
- **AND** the card displays:
  - Fingerprint truncated to `sha256:<first 8 hex chars>…<last 4 hex chars>` with full fingerprint selectable
  - Device type badge (e.g., "Desktop")
  - `Paired 2 hours ago` (relative time from `pairedAt`)
  - Spine URL (non-selectable, secondary text)
  - `Last seen 5 minutes ago` (relative time from `lastSeenAt`; "Never" if null)

#### Scenario: Paired desktop card hidden when unpaired

- **WHEN** `isPaired === false`
- **THEN** the "Paired Desktop" card is not rendered
- **AND** the Settings screen does not show any peer device information

#### Scenario: Fingerprint truncation format

- **WHEN** the peer fingerprint is `sha256:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2`
- **THEN** the displayed fingerprint reads `sha256:a1b2c3d4…a1b2`
- **AND** the full fingerprint string is accessible via text selection

### Requirement: Lightweight Unpair preserving device identity

The mobile app SHALL provide an `unpair()` function that revokes the current device on Spine, aborts in-flight Spine requests/uploads for the current pairing, clears pairing state from secure storage, flushes the outbox, and transitions the UI to unpaired — all without destroying the Ed25519 device identity key.

#### Scenario: Successful unpair from Settings

- **WHEN** the user taps "Unpair" on the Paired Desktop card
- **AND** confirms the destructive action in the confirmation dialog
- **THEN** the app calls `POST {spine_url}/v1/devices/{self_device_uuid}/revoke` with body `{ device_uuid: "<self_device_uuid>" }`
- **AND** on 2xx response, calls `clearPairingState()` removing all `syncmind.pairing.*` keys from `expo-secure-store`
- **AND** aborts in-flight Spine requests/uploads associated with the current pairing
- **AND** calls `clearOutbox()` to flush the pending outbox queue
- **AND** calls `useAppStore.getState().setUnpaired()`
- **AND** the Paired Desktop card disappears from Settings
- **AND** tabs are grayed out
- **AND** the Ed25519 identity key (`syncmind.device_identity_meta` in secure-store) is NOT removed

#### Scenario: Unpair when Spine revoke fails with network error

- **WHEN** the user confirms unpair
- **AND** the `POST /v1/devices/{self}/revoke` call fails with a network error (timeout, DNS failure, connection refused)
- **THEN** the app displays an alert: "Could not notify desktop — unpaired locally"
- **AND** still clears pairing state, outbox, and transitions to unpaired locally
- **AND** aborts in-flight Spine requests/uploads associated with the current pairing
- **AND** the Ed25519 identity key is preserved

#### Scenario: Unpair when Spine revoke returns 401 or 404

- **WHEN** the user confirms unpair
- **AND** the `POST /v1/devices/{self}/revoke` call returns 401 or 404
- **THEN** the app silently treats this as success (server already considers device revoked/unknown)
- **AND** clears pairing state, outbox, and transitions to unpaired
- **AND** aborts in-flight Spine requests/uploads associated with the current pairing
- **AND** does NOT show an error alert (the outcome is the same)

#### Scenario: Unpair returns remote revoke warning for UI

- **WHEN** the user confirms unpair
- **AND** the remote revoke call fails because of a network error or 5xx server error
- **THEN** `unpair()` still completes local cleanup
- **AND** returns a typed warning result that Settings can use to display "Could not notify desktop — unpaired locally"
- **AND** does NOT re-pair or restore local pairing state after the warning

#### Scenario: Unpair aborts in-flight Spine work

- **WHEN** one or more authenticated Spine requests or upload attempts are in flight for the current pairing
- **AND** the user confirms unpair
- **THEN** the app aborts those in-flight operations before completing the local unpaired transition
- **AND** no aborted operation may re-write pairing state, re-enqueue cleared outbox items, or transition the app back to paired
- **AND** the Ed25519 identity key remains available for future re-pairing

#### Scenario: device_reset unchanged (regression guard)

- **WHEN** the user triggers "Reset Device Identity" from Danger Zone
- **THEN** `device_reset()` is called
- **AND** it still calls `revokeCurrentDevice()`, `clearPairingState()`, `clearOutbox()`, `setUnpaired()`, AND `clearIdentity()`
- **AND** the Ed25519 identity key is destroyed (full nuclear reset)

### Requirement: Unpaired state tab bar locking

When the app is unpaired, the bottom tab bar SHALL disable navigation to tabs that require an active pairing, while keeping the Capture tab available as the primary pairing entry.

#### Scenario: Paired-only tabs disabled when unpaired

- **WHEN** `isPaired === false`
- **THEN** the search/list tab or any existing placeholder tab representing paired-only capability is visually dimmed (opacity 0.4)
- **AND** tapping the paired-only tab does not navigate (navigation is disabled)
- **AND** a lock icon is displayed next to each disabled tab label
- **AND** the Capture tab remains navigable so the user can scan a desktop pairing QR code or see the pairing call-to-action

#### Scenario: Tabs enabled when paired

- **WHEN** `isPaired === true`
- **THEN** all tabs are fully opaque and navigable
- **AND** no lock icons are shown

#### Scenario: Unpaired empty state in capture and search screens

- **WHEN** the user is on the captures screen and `isPaired === false`
- **THEN** the screen displays the QR pairing scanner or a centered message: "Pair with a desktop to start capturing"
- **AND** the user can start pairing from this screen without first visiting Settings

- **WHEN** the user is on the search screen and `isPaired === false`
- **THEN** the screen displays a centered message: "Pair with a desktop to search your knowledge"
- **AND** a "Go to Settings" button navigates to the Settings tab

### Requirement: Spine self-device status and revoke endpoints

The Spine server SHALL expose authenticated self-device endpoints for startup pairing validation and device-level unpair. These endpoints SHALL only allow a device to read or revoke itself.

#### Scenario: Self-device status succeeds

- **WHEN** a paired mobile device calls `GET /v1/devices/{self_device_uuid}`
- **AND** the Authorization Bearer JWT is valid
- **AND** the JWT `sub` equals `{self_device_uuid}`
- **THEN** Spine returns HTTP 200 with `device_uuid`, `device_type`, `paired_device_id`, `is_active`, and `last_seen_at`

#### Scenario: Self-device status rejects another device id

- **WHEN** a device calls `GET /v1/devices/{other_device_uuid}`
- **AND** the Authorization Bearer JWT `sub` does not equal `{other_device_uuid}`
- **THEN** Spine returns HTTP 404 or 403 without revealing whether the other device exists
- **AND** the caller MUST NOT use this response to clear local pairing state unless the requested id was its own `self_device_uuid`

#### Scenario: Self-device revoke deactivates current device

- **WHEN** a paired mobile device calls `POST /v1/devices/{self_device_uuid}/revoke` with body `{ "device_uuid": "<self_device_uuid>" }`
- **AND** the Authorization Bearer JWT is valid
- **AND** the JWT `sub` equals `{self_device_uuid}`
- **THEN** Spine sets the current device `is_active` to `false`
- **AND** clears any paired device row whose `paired_device_id` points at the revoked device
- **AND** returns HTTP 204

#### Scenario: Self-device revoke treats unknown self as stale pairing

- **WHEN** a mobile device calls `POST /v1/devices/{self_device_uuid}/revoke`
- **AND** the JWT is valid but Spine has no active device row for `{self_device_uuid}`
- **THEN** Spine returns HTTP 404 with error code `DEVICE_NOT_FOUND` or `DEVICE_REVOKED`
- **AND** the mobile app treats this as a successful local unpair outcome

### Requirement: Authenticated Spine requests use Ed25519 JWT

The mobile app SHALL authenticate protected Spine API calls with an Ed25519-signed Bearer JWT produced through the native device identity signer. The pairing `sync_key` SHALL NOT be used as an HTTP authentication credential.

#### Scenario: authenticatedFetch injects a valid JWT

- **WHEN** `authenticatedFetch` sends a protected Spine request
- **AND** restored pairing state contains `selfDeviceUuid`
- **THEN** it creates a JWT with claims `sub`, `iat`, `exp`, `jti`, `iss`, and `aud`
- **AND** `sub` equals `selfDeviceUuid`
- **AND** `iss` equals `syncmind-device`
- **AND** `aud` equals `syncmind-spine`
- **AND** the JWT is signed by the native Ed25519 identity via `sign()`
- **AND** the request includes `Authorization: Bearer <jwt>`

### Requirement: Automatic unpaired transition on authentication or self-device revocation

Authenticated Spine API calls that prove the current pairing is invalid SHALL clear pairing state and transition the app to `Unpaired` state, preventing infinite retry loops. Generic resource 404 responses SHALL NOT clear pairing state.

#### Scenario: 401 triggers auto-unpair

- **WHEN** the app makes an authenticated request to Spine (health check, upload, search, etc.)
- **AND** Spine returns HTTP 401
- **THEN** the `authenticatedFetch` wrapper intercepts the response
- **AND** calls `clearPairingState()` to remove all `syncmind.pairing.*` keys
- **AND** calls `useAppStore.getState().setUnpaired()`
- **AND** throws `UnpairedError`
- **AND** the UI transitions to unpaired state (tabs grayed out, Paired Desktop card hidden)

#### Scenario: Self-device 404 triggers auto-unpair

- **WHEN** the app makes an authenticated request to `GET /v1/devices/{self_device_uuid}`
- **AND** Spine returns HTTP 404 with error code `DEVICE_NOT_FOUND` or `DEVICE_REVOKED`
- **THEN** the `authenticatedFetch` wrapper intercepts the response
- **AND** performs the same auto-unpair sequence as 401
- **AND** throws `UnpairedError`

#### Scenario: Generic 404 does not trigger auto-unpair

- **WHEN** the app makes an authenticated request to a non-device resource such as `GET /v1/sync/bundles/{bundle_id}`
- **AND** Spine returns HTTP 404 because the resource does not exist or is hidden
- **THEN** pairing state is NOT cleared
- **AND** `authenticatedFetch` returns the response or throws the caller's normal HTTP error type
- **AND** `isPaired` remains `true`

#### Scenario: Network errors do NOT trigger auto-unpair

- **WHEN** the app makes an authenticated request to Spine
- **AND** the request fails with a network error (timeout, DNS failure, connection refused)
- **THEN** pairing state is NOT cleared
- **AND** `isPaired` remains `true`
- **AND** `connectionStatus` may be set to `"error"` but the app remains in paired state for retry

#### Scenario: Health check on startup detects stale pairing

- **WHEN** the app starts up and `restorePairingState()` succeeds (pairing state exists in secure-store)
- **THEN** the app performs a startup health-check ping to Spine using `GET /v1/devices/{self_device_uuid}` through `authenticatedFetch`
- **AND** if the health check returns 401 or a qualifying self-device 404, the `authenticatedFetch` interceptor auto-clears state
- **AND** the app enters unpaired state without user action
- **AND** if the health check succeeds, `connectionStatus` becomes `"connected"`

### Requirement: Last successful Spine contact tracking

The mobile app SHALL track the timestamp of the last successful Spine API response and persist it as part of the pairing state.

#### Scenario: lastSeenAt updated on 2xx response

- **WHEN** any request through `authenticatedFetch` receives a 2xx response
- **THEN** `PersistedPairingState.lastSeenAt` is updated to the current UTC ISO 8601 timestamp in memory
- **AND** the value is persisted to `expo-secure-store` under `syncmind.pairing.last_seen_at`

#### Scenario: lastSeenAt throttle prevents excessive writes

- **WHEN** multiple 2xx responses arrive within a 30-second window
- **THEN** `expo-secure-store` is written at most once per 30 seconds
- **AND** the in-memory value always reflects the latest timestamp

#### Scenario: lastSeenAt is null when never contacted

- **WHEN** pairing is completed but no Spine API call has succeeded yet
- **THEN** `lastSeenAt` is `null`
- **AND** the Settings card displays "Never" for last seen time

#### Scenario: Existing US-041 pairing state restores without lastSeenAt

- **WHEN** the app launches with pairing state persisted by US-041
- **AND** `syncmind.pairing.last_seen_at` is missing from `expo-secure-store`
- **THEN** `restorePairingState()` succeeds
- **AND** `PersistedPairingState.lastSeenAt` is `null`
- **AND** the app remains paired unless the startup self-device health check returns 401 or a qualifying 404
