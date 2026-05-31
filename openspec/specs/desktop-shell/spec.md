# desktop-shell Specification

## Purpose
TBD - created by archiving change the-command-palette. Update Purpose after archive.
## Requirements
### Requirement: Application scaffolding initialized
The system SHALL provide a Tauri v2 project with a SolidJS frontend inside `apps/desktop/`.

#### Scenario: Development build succeeds
- **WHEN** a developer runs `pnpm install` followed by `pnpm dev`
- **THEN** the Tauri development window launches without errors
- **AND** the SolidJS frontend renders inside the window

#### Scenario: Rust backend compiles cleanly
- **WHEN** a developer runs `cargo check` inside `apps/desktop/src-tauri/`
- **THEN** the check completes with zero errors and zero warnings under `#![warn(clippy::all)]`

### Requirement: Rust core library integration
The system SHALL link `syncmind-core`, `syncmind-storage`, and `syncmind-rag-engine` as Cargo `path` dependencies and expose their capabilities through typed Tauri Commands.

#### Scenario: Core runtime starts on app launch
- **WHEN** the desktop application launches
- **THEN** the Tauri backend initializes the `syncmind-core` runtime
- **AND** the runtime loads `~/.config/syncmind/config.toml`
- **AND** the runtime starts the file watcher and indexing pipeline

#### Scenario: Type-safe command bridge
- **WHEN** a Tauri Command is invoked from the frontend
- **THEN** the command accepts and returns strongly typed structures
- **AND** corresponding TypeScript type definitions exist in `packages/types` or are auto-generated

### Requirement: Global hotkey toggles palette visibility
The system SHALL register a system-wide global hotkey that toggles the command palette window visibility on macOS.

#### Scenario: Hotkey shows hidden palette
- **WHEN** the user presses `Cmd+Shift+Space` while the palette is hidden
- **THEN** the palette window appears centered on the active screen within 300 ms
- **AND** the search input receives focus with its text selected

#### Scenario: Hotkey hides visible palette
- **WHEN** the user presses `Cmd+Shift+Space` while the palette is visible
- **THEN** the palette window hides within 150 ms

#### Scenario: Escape key hides palette
- **WHEN** the user presses `Esc` while the palette is visible
- **THEN** the palette window hides within 150 ms

### Requirement: Floating window lifecycle
The system SHALL present the command palette as a borderless, fixed-size floating panel that hides on blur.

#### Scenario: Window appears on activation
- **WHEN** the palette is activated
- **THEN** it renders as a borderless window centered on the current screen
- **AND** its dimensions are fixed at 860 px by 540 px
- **AND** it is not resizable by the user

#### Scenario: Window hides on focus loss
- **WHEN** the palette loses application focus (user clicks outside)
- **THEN** it hides automatically within 150 ms using a fade animation
- **AND** the application remains running

### Requirement: System tray integration
The system SHALL provide a macOS menu bar tray icon with a functional context menu.

#### Scenario: Tray menu shows on click
- **WHEN** the user clicks the SyncMind tray icon
- **THEN** a menu appears with items: "Open Palette", "Settings...", "Indexing Status", and "Quit"

#### Scenario: Tray reflects engine health
- **WHEN** the core engine is running normally
- **THEN** the tray icon or menu indicates a healthy status (e.g., green indicator)
- **WHEN** the last indexing operation failed
- **THEN** the tray indicates an error status (e.g., red indicator)

### Requirement: Auto-launch on login
The system SHALL support registering itself as a macOS login item.

#### Scenario: User enables auto-launch
- **WHEN** the user toggles "Launch at login" in Settings
- **THEN** the application registers itself as a login item
- **AND** it launches automatically on the next user login

#### Scenario: User disables auto-launch
- **WHEN** the user toggles "Launch at login" off
- **THEN** the application removes itself from login items

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

