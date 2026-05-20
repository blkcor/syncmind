## ADDED Requirements

### Requirement: Visual configuration editor
The system SHALL provide a settings panel that edits and persists `~/.config/syncmind/config.toml`.

#### Scenario: Ollama URL editing
- **WHEN** the user opens the Settings panel
- **THEN** an input field for `ollama_url` is visible
- **AND** the field validates that the value is a valid HTTP URL
- **AND** saving updates `config.toml` and triggers a core config reload

#### Scenario: Ollama model selection
- **WHEN** the user opens the Settings panel
- **THEN** a dropdown shows common models (`bge-m3`, `bge-small`)
- **AND** the user can enter a custom model name

#### Scenario: Transport mode display
- **WHEN** the user opens the Settings panel
- **THEN** the current `mcp_transport` value is displayed as read-only text
- **AND** a note explains that MCP transport is managed by the CLI daemon, not the desktop app

### Requirement: Registered file management
The system SHALL allow users to view, add, and remove registered file paths from the settings panel.

#### Scenario: List registered files
- **WHEN** the user opens the Settings panel
- **THEN** a list displays all paths from `registered_files`
- **AND** each entry shows the file path and a delete button

#### Scenario: Add files via dialog
- **WHEN** the user clicks "Add File"
- **THEN** a native file picker dialog opens supporting multiple selection
- **AND** selected files are appended to `registered_files`
- **AND** each new file is immediately queued for incremental indexing

#### Scenario: Remove registered file
- **WHEN** the user clicks the delete button next to a registered file
- **THEN** the path is removed from `registered_files`
- **AND** the config is saved

### Requirement: Indexing status dashboard
The system SHALL display a real-time dashboard of the indexing pipeline health.

#### Scenario: Summary cards visible
- **WHEN** the user opens the Settings panel
- **THEN** summary cards display: total registered files, total indexed chunks, and last indexing update timestamp

#### Scenario: Error log list
- **WHEN** indexing errors have occurred
- **THEN** a list shows the most recent 10 errors
- **AND** each entry includes: file path, error message, and timestamp

#### Scenario: Empty error state
- **WHEN** no indexing errors have occurred
- **THEN** the error log area displays: "No recent errors."

### Requirement: Manual re-index trigger
The system SHALL provide a control to rebuild the index on demand.

#### Scenario: Rebuild all with confirmation
- **WHEN** the user clicks "Rebuild All"
- **THEN** a confirmation dialog appears warning that the operation may take time
- **AND** confirming triggers a full re-index in a background thread
- **AND** the UI remains responsive during the operation

#### Scenario: Rebuild progress feedback
- **WHEN** a rebuild is in progress
- **THEN** the dashboard shows an in-progress indicator
- **AND** the summary cards update automatically when the rebuild completes
