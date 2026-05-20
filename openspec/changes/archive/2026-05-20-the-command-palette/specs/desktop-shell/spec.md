## ADDED Requirements

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
