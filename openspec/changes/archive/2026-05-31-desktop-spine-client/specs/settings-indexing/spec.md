## ADDED Requirements

### Requirement: Devices/Sync settings section is exposed in the desktop application
The desktop settings surface SHALL include a Devices/Sync section that lets the user configure the Spine URL, an optional self-signed CA path, view their device identity (fingerprint and UUID), initiate pairing, view paired-peer state and connection status, browse the sync-inbox, and trigger unpair or reset.

#### Scenario: Spine URL editor
- **WHEN** the user opens the Devices tab
- **THEN** an input field for `spine.url` is visible and pre-filled with the current value
- **AND** the field accepts `http://`, `https://`, and IP-based URLs
- **AND** saving updates `config.toml` and triggers a Spine subsystem reload

#### Scenario: Trust-CA PEM file picker
- **WHEN** the user opens the Devices tab and clicks "Trust self-signed CA"
- **THEN** a native file picker dialog opens accepting `.pem` and `.crt` files
- **AND** the chosen path is saved to `spine.trust_ca_path`
- **AND** the next HTTPS request to the Spine uses the supplied certificate as an additional root

#### Scenario: Plain HTTP warning banner
- **WHEN** the configured `spine.url` uses scheme `http://`
- **THEN** the Devices tab renders a yellow banner stating "Spine traffic is not transport-encrypted. End-to-end encryption still applies, but consider HTTPS."

#### Scenario: Local identity card
- **WHEN** the user opens the Devices tab and a device identity has been generated
- **THEN** the page displays the device fingerprint (first 16 characters with a copy button revealing all 64) and the device UUID
- **AND** a creation timestamp is shown

#### Scenario: Pair action when unpaired
- **WHEN** the Devices tab is open and `PairingState == Idle`
- **THEN** a "Start pairing" button is visible
- **AND** clicking it invokes `spine_start_pairing` and opens a modal containing the QR PNG, the 6-digit short code, and a `mm:ss` countdown

#### Scenario: Paired state display
- **WHEN** the Devices tab is open and `PairingState == Paired { peer_fingerprint, ... }`
- **THEN** the page displays the peer's fingerprint (truncated + copy-full), `device_type`, `paired_at`, last-seen time
- **AND** a connection-status badge reflects the current `ConnectionState` (green Connected / amber Reconnecting / grey Offline)
- **AND** an "Unpair" button is visible in the danger zone

### Requirement: sync-inbox is browsable and manually clearable from the settings surface
The Devices tab SHALL display the current size and last-modified timestamp of `<data-dir>/sync-inbox/` and SHALL expose a manual "Empty inbox" action gated by a two-step confirmation dialog. There SHALL be no automatic deletion of sync-inbox contents.

#### Scenario: Inbox size summary
- **WHEN** the user opens the Devices tab
- **THEN** an "Inbox" card shows the total size of `<data-dir>/sync-inbox/` (formatted human-readable, e.g., "12.4 MB") and the timestamp of the most-recently-modified file (or "empty")

#### Scenario: Manual clear with confirmation
- **WHEN** the user clicks "Empty inbox"
- **THEN** a confirmation dialog appears listing the number of files and total size that will be deleted
- **AND** confirming triggers `spine_clear_inbox`, which deletes every file under `<data-dir>/sync-inbox/` and recreates the directory with mode `0700`

#### Scenario: No automatic cleanup
- **WHEN** the desktop application is left running for arbitrary time periods
- **THEN** no background task deletes sync-inbox files based on age, size, or count

### Requirement: Trust-CA path is validated when set
The system SHALL validate that any value assigned to `spine.trust_ca_path` points to a readable file containing at least one PEM-encoded certificate. Invalid values SHALL be rejected without overwriting the previous configuration.

#### Scenario: Valid PEM accepted
- **WHEN** the user supplies a path to a file containing a `-----BEGIN CERTIFICATE-----` block parseable by `rustls-pemfile::certs`
- **THEN** the `spine_set_trust_ca` command persists the path to `config.toml`
- **AND** the next Spine HTTPS request adds the parsed certificate via `reqwest::ClientBuilder::add_root_certificate`

#### Scenario: Non-existent file rejected
- **WHEN** the user supplies a path that does not exist or is not readable
- **THEN** the `spine_set_trust_ca` command returns error code `TRUST_CA_NOT_READABLE`
- **AND** the previous `spine.trust_ca_path` value is preserved

#### Scenario: File without PEM certificate rejected
- **WHEN** the user supplies a path to a file that contains no valid PEM certificate block
- **THEN** the `spine_set_trust_ca` command returns error code `TRUST_CA_INVALID_PEM`
- **AND** the previous `spine.trust_ca_path` value is preserved
