## ADDED Requirements

### Requirement: Voice mode is available from the paired Capture screen

The mobile app SHALL expose US-044 voice capture from the existing paired Capture screen without changing the unpaired pair-first flow. Voice mode SHALL be reachable by an upward swipe of at least 48 px from the lower capture composer/action area and by an explicit microphone mode-toggle control.

#### Scenario: Paired user enters voice mode
- **WHEN** the app is paired
- **AND** the user swipes upward at least 48 px from the lower capture composer/action area before release
- **THEN** the Capture screen switches to voice mode
- **AND** the screen shows a circular press-and-hold recording control
- **AND** text capture draft content is not discarded by the mode switch

#### Scenario: Paired user enters voice mode by accessible toggle
- **WHEN** the app is paired
- **AND** the user activates the microphone mode-toggle control
- **THEN** the Capture screen switches to voice mode
- **AND** the control has an accessibility label that identifies voice capture mode
- **AND** text capture draft content is not discarded by the mode switch

#### Scenario: Unpaired user cannot enter recording
- **WHEN** the app is not paired
- **THEN** the Capture screen shows the pairing scanner or unpaired state
- **AND** it does not request microphone permission
- **AND** it does not show an active recording control

### Requirement: Microphone permission gates recording

The mobile app SHALL request microphone permission before starting the first recording and SHALL guide the user to system settings when permission is denied.

#### Scenario: Permission granted starts recording
- **WHEN** the paired user presses the voice recording control
- **AND** microphone permission has not already been granted
- **AND** the platform permission request succeeds
- **THEN** the app starts recording
- **AND** the recording state becomes visible on the Capture screen

#### Scenario: Permission denied shows settings guidance
- **WHEN** the paired user presses the voice recording control
- **AND** microphone permission is denied
- **THEN** the app does not start recording
- **AND** the app shows a path to open system settings
- **AND** no capture payload is created
- **AND** no outbox row is enqueued

### Requirement: Recording uses the US-044 audio profile

The mobile app SHALL configure recordings as `.m4a` AAC LC audio targeting 16000 Hz sample rate, mono channel count, and 32000 bps bit rate when supported by the platform encoder.

#### Scenario: Recording starts with configured audio options
- **WHEN** recording starts
- **THEN** the recorder is configured for an `.m4a` container
- **AND** the recorder is configured for AAC LC audio
- **AND** the target sample rate is 16000 Hz
- **AND** the target channel count is 1
- **AND** the target bit rate is 32000 bps

### Requirement: Press-and-hold sends on release

The mobile app SHALL start recording when the user holds the voice control and SHALL stop, validate, encrypt, enqueue, and trigger best-effort upload when the user releases it. The same recording action SHALL also be available through an accessibility-compatible double-tap-to-toggle interaction.

#### Scenario: Release sends valid audio capture
- **WHEN** the paired user holds the voice recording control
- **AND** recording has started successfully
- **AND** the user releases the control before the maximum duration
- **AND** the recorded clip is within size limits
- **THEN** the app stops recording
- **AND** reads the `.m4a` bytes
- **AND** creates a `capture-audio` payload
- **AND** encrypts and enqueues the bundle
- **AND** starts a best-effort outbox flush
- **AND** deletes the recorder temp file best-effort after enqueue

#### Scenario: Recording shows metering waveform
- **WHEN** recording is active
- **AND** recorder status includes metering values
- **THEN** the Capture screen updates a waveform or level display from those metering values
- **AND** the waveform is not persisted to the outbox

#### Scenario: Accessible toggle records without press-and-hold
- **WHEN** the app is paired
- **AND** the Capture screen is in voice mode
- **AND** the user activates the recording control through an accessibility action
- **THEN** the app starts recording without requiring a sustained press gesture
- **AND** a second activation stops, validates, encrypts, enqueues, and triggers best-effort upload for a valid clip

### Requirement: Audio captures enforce duration and size limits

The mobile app SHALL enforce a 60-second maximum duration and SHALL reject audio captures larger than 8 MB raw bytes or 11 MB base64 before encryption or enqueue.

#### Scenario: Sixty-second timeout stops recording
- **WHEN** recording reaches 60 seconds
- **THEN** the app stops recording automatically
- **AND** the app informs the user that the maximum recording length was reached
- **AND** the resulting clip follows the same validation and enqueue path as a released recording

#### Scenario: Oversized audio is rejected locally
- **WHEN** a stopped recording exceeds 8 MB raw bytes
- **OR** its base64 representation exceeds 11 MB
- **THEN** the app rejects the capture locally
- **AND** shows `Clip too long`
- **AND** no encrypted outbox row is created
- **AND** the recorder temp file is deleted best-effort

### Requirement: Audio payload matches the capture-audio schema

The mobile app SHALL build the plaintext `capture-audio` payload only transiently before bundle encryption. The payload SHALL include `v: 1`, `kind: "capture-audio"`, `id`, `audio_base64`, `audio_mime: "audio/mp4"`, `duration_ms`, `client_ts`, and `client_device_fingerprint`.

#### Scenario: Valid recording produces capture-audio plaintext before encryption
- **WHEN** a valid recording is stopped
- **THEN** the app creates a payload with `v = 1`
- **AND** `kind = "capture-audio"`
- **AND** `id` is a UUID v4
- **AND** `audio_base64` contains the recorded `.m4a` bytes encoded as base64
- **AND** `audio_mime = "audio/mp4"`
- **AND** `duration_ms` is the measured recording duration
- **AND** `client_ts` is the capture timestamp
- **AND** `client_device_fingerprint` is the local mobile device fingerprint

#### Scenario: Plaintext audio is not durably persisted
- **WHEN** a capture-audio payload is built
- **THEN** plaintext JSON and raw audio bytes are used only long enough to encrypt the bundle
- **AND** SQLite outbox rows store encrypted bytes and non-sensitive metadata only
- **AND** logs, retry metadata, and status previews do not include raw audio bytes or `audio_base64`

### Requirement: Recording interruptions preserve a user choice

The mobile app SHALL stop recording on call-like interruption or when the app remains backgrounded for more than 30 seconds, then present a keep/discard choice for the partial segment.

#### Scenario: Interruption prompts keep or discard
- **WHEN** recording is active
- **AND** a platform interruption occurs
- **THEN** the app stops recording
- **AND** preserves the partial temp file long enough for user choice
- **AND** asks whether to keep or discard the segment

#### Scenario: Keep enqueues the interrupted segment
- **WHEN** an interrupted segment is waiting for review
- **AND** the user chooses keep
- **THEN** the app validates the partial clip
- **AND** encrypts and enqueues it as `capture-audio` if valid
- **AND** deletes the temp file best-effort after enqueue

#### Scenario: Discard deletes the interrupted segment
- **WHEN** an interrupted segment is waiting for review
- **AND** the user chooses discard
- **THEN** no capture payload is created
- **AND** no outbox row is enqueued
- **AND** the temp file is deleted best-effort
