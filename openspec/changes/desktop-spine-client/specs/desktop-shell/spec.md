## ADDED Requirements

### Requirement: Devices tab is registered as the fifth top-level navigation entry
The desktop shell SHALL register a "Devices" tab in `apps/desktop/src/App.tsx`'s tab list as the fifth navigation entry (after Search, Pinned, RAG Lab, Settings) and SHALL load the `DevicesTab` SolidJS component when the tab is active.

#### Scenario: Tab appears in navigation
- **WHEN** the user opens the main palette window
- **THEN** the tab list contains exactly five entries in this order: Search, Pinned, RAG Lab, Settings, Devices

#### Scenario: Devices tab renders the Spine UI
- **WHEN** the user clicks the Devices tab
- **THEN** `apps/desktop/src/components/DevicesTab.tsx` mounts inside the tab content area
- **AND** the component immediately invokes `spine_get_config`, `spine_get_identity`, and `spine_pair_status` to populate the view

### Requirement: Tray menu includes "Sync devices…" entry that navigates to the Devices tab
The desktop shell SHALL add a "Sync devices…" item to the tray icon menu (defined around `apps/desktop/src-tauri/src/lib.rs:417-431`) that, when clicked, shows the main window and switches the active tab to Devices.

#### Scenario: Tray click opens Devices tab
- **WHEN** the user clicks the "Sync devices…" item in the tray menu
- **THEN** the main window becomes visible and focused
- **AND** the active tab in the SolidJS store is set to `"devices"`

### Requirement: Spine Tauri commands are registered with the application builder
The desktop shell SHALL register the following commands in the Tauri command registration block of `apps/desktop/src-tauri/src/lib.rs`: `spine_get_config`, `spine_set_url`, `spine_set_trust_ca`, `spine_get_identity`, `spine_start_pairing`, `spine_pair_status`, `spine_cancel_pairing`, `spine_send_note`, `spine_unpair`, `spine_reset_identity`, `spine_list_inbox`, `spine_clear_inbox`. Each command SHALL return strongly typed structures that omit any secret material.

#### Scenario: Command discovery
- **WHEN** the desktop application starts
- **THEN** every command listed above is invokable from the SolidJS frontend via `@tauri-apps/api/core`'s `invoke`

#### Scenario: Commands omit secret material
- **WHEN** any Spine command returns a value to the frontend
- **THEN** the returned struct SHALL NOT contain Ed25519 private key bytes, X25519 private key bytes, `sync_key` bytes, `shared_secret` bytes, raw JWT tokens, or the `Authorization` header
- **AND** the returned struct MAY contain public fingerprints, device UUIDs, status enums, and human-readable error codes

### Requirement: SpineState is initialized at application startup
The desktop shell SHALL construct a single `SpineState` instance during Tauri builder setup and SHALL store it as managed application state so all Spine commands share the same identity, pairing, and connection state machines.

#### Scenario: SpineState is shared across commands
- **WHEN** two Tauri commands run concurrently and each calls into the Spine subsystem
- **THEN** both observe the same `SpineState` singleton
- **AND** state transitions are serialized by the `tokio::sync::Mutex` owned by `SpineState`

#### Scenario: Background tasks shut down on app exit
- **WHEN** the desktop application receives a quit signal
- **THEN** the application invokes `SpineState::shutdown()` which aborts every task in the owned `JoinSet`
- **AND** in-flight HTTPS requests are dropped without blocking the exit
