## ADDED Requirements

### Requirement: Paired app opens to the text capture screen

The mobile app SHALL use the existing first tab as the paired text capture home screen. When pairing has been restored, the screen SHALL render a multiline text input with automatic focus so the user can begin typing without opening another view.

#### Scenario: Paired launch focuses text input

- **WHEN** the app finishes startup pairing restoration
- **AND** restored pairing state is present
- **THEN** the first tab is the Capture screen
- **AND** the Capture screen renders a multiline text input
- **AND** the text input requests focus automatically

#### Scenario: Unpaired launch remains pair-first

- **WHEN** the app finishes startup pairing restoration
- **AND** no restored pairing state is present
- **THEN** the first tab shows the unpaired capture empty state
- **AND** the pairing scanner remains available
- **AND** Send controls are not available

### Requirement: Capture screen shows peer and queue status

The paired Capture screen SHALL show a thin status row above the input. The row SHALL indicate connected, queued/offline, or pairing-invalid state without reading or decrypting queued capture blobs.

#### Scenario: Connected status is green

- **WHEN** the app is paired
- **AND** `connectionStatus` is `connected`
- **THEN** the status row shows a green status dot
- **AND** the row identifies the paired peer using non-sensitive peer metadata such as a shortened fingerprint

#### Scenario: Queued or offline status is gray

- **WHEN** the app is paired
- **AND** `connectionStatus` is `disconnected` or `connecting`
- **THEN** the status row shows a gray status dot
- **AND** the row communicates that captures can be queued locally

#### Scenario: Invalid pairing status is red

- **WHEN** the app is paired
- **AND** `connectionStatus` is `error`
- **THEN** the status row shows a red status dot
- **AND** the row communicates that pairing should be checked before upload can succeed

### Requirement: Text capture input enforces send eligibility

The paired Capture screen SHALL only allow Send for non-empty text at or below 50,000 characters. The character limit SHALL be enforced before creating, encrypting, or enqueuing a capture payload.

#### Scenario: Empty text cannot be sent

- **WHEN** the app is paired
- **AND** the text input is empty or contains only whitespace
- **THEN** Send is disabled or behaves as a local no-op
- **AND** no capture payload is created
- **AND** no outbox row is enqueued

#### Scenario: Text above envelope limit is rejected locally

- **WHEN** the app is paired
- **AND** the text input contains more than 50,000 characters
- **THEN** Send is disabled
- **AND** the screen shows `Too long - try splitting`
- **AND** no capture payload is created
- **AND** no outbox row is enqueued

#### Scenario: Eligible text is optimistically cleared after enqueue

- **WHEN** the app is paired
- **AND** the text input contains non-whitespace text at or below 50,000 characters
- **AND** the user taps Send
- **THEN** the app creates and encrypts a `capture-text` payload
- **AND** enqueues the encrypted bundle in the existing outbox
- **AND** clears the text input after enqueue succeeds
- **AND** starts a best-effort outbox flush without waiting for network success

### Requirement: Capture screen shows recent local capture metadata

The paired Capture screen SHALL show a mini preview of the latest 3 local capture statuses using outbox metadata. The preview SHALL NOT decrypt queued blobs or read plaintext from encrypted payloads.

#### Scenario: Latest three rows are displayed

- **WHEN** the app is paired
- **AND** the outbox contains one or more rows
- **THEN** the Capture screen shows up to 3 rows ordered by `created_at DESC`
- **AND** each row displays local metadata such as bounded preview text, relative time, state, attempts, or whitelisted error code
- **AND** the screen does not read `encrypted_blob`

#### Scenario: Recent status updates after queue changes

- **WHEN** enqueue or flush changes an outbox row
- **THEN** the Capture screen refreshes the mini preview through the existing outbox change subscription
- **AND** a polling fallback refreshes the same metadata while the screen remains paired

#### Scenario: Full recent list remains deferred

- **WHEN** the user interacts with the mini preview
- **THEN** the app MUST NOT expose a partial decrypted recent-capture list in this change
- **AND** full recent-list navigation and controls remain owned by US-049

### Requirement: Capture screen supports keyboard dismissal without audio capture

The paired Capture screen SHALL let the user dismiss the keyboard from the capture surface. The screen MUST NOT start audio recording or request microphone permission as part of US-043.

#### Scenario: User dismisses keyboard from capture surface

- **WHEN** the app is paired
- **AND** the keyboard is open on the Capture screen
- **AND** the user taps or drags outside the active text input
- **THEN** the keyboard is dismissed
- **AND** the current draft text remains in the input

#### Scenario: Voice mode is not implemented by text capture

- **WHEN** the user performs an upward voice-mode gesture on the Capture screen
- **THEN** the app does not start recording audio
- **AND** the app does not request microphone permission
- **AND** audio capture remains deferred to US-044
