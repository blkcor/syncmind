# command-palette Specification

## Purpose
TBD - created by archiving change the-command-palette. Update Purpose after archive.
## Requirements
### Requirement: Semantic search input
The system SHALL provide a debounced search input field at the top of the command palette.

#### Scenario: Input focused on show
- **WHEN** the palette window becomes visible
- **THEN** the search input immediately receives keyboard focus
- **AND** any existing text is selected

#### Scenario: Debounced search execution
- **WHEN** the user types into the search input
- **THEN** the system waits 300 ms after the last keystroke before invoking the Rust search command
- **AND** a loading indicator is displayed during the debounce and search period

#### Scenario: Empty query handling
- **WHEN** the search input is empty or cleared
- **THEN** the results list shows an empty state message: "Start typing to search your knowledge..."

### Requirement: Search results list
The system SHALL display semantic search results in a scrollable list with metadata.

#### Scenario: Results render with metadata
- **WHEN** search results are returned from the backend
- **THEN** each item displays: file path (truncated), content preview (first 120 characters), file-type icon, and similarity score (two decimal places)

#### Scenario: No results state
- **WHEN** a query returns zero results
- **THEN** the list displays: "No matches found. Try a broader query."

#### Scenario: File type icons mapped
- **WHEN** a result's source file has an extension (e.g., `.rs`, `.md`, `.py`)
- **THEN** the list shows a corresponding file-type icon
- **WHEN** the extension is unrecognized
- **THEN** a generic document icon is shown

### Requirement: Keyboard navigation
The system SHALL allow full keyboard control of the results list.

#### Scenario: Arrow key navigation
- **WHEN** the user presses `↑` or `↓`
- **THEN** the selection moves up or down one result
- **AND** the newly selected result is scrolled into view if necessary

#### Scenario: Enter copies content
- **WHEN** the user presses `Enter` on a selected result
- **THEN** the chunk's full content is copied to the system clipboard
- **AND** a "Copied!" toast or inline feedback appears

#### Scenario: Cmd+Enter opens file
- **WHEN** the user presses `Cmd+Enter` on a selected result
- **THEN** the source file opens in the system's default application for that file type

### Requirement: Preview pane
The system SHALL show a detailed preview of the selected result in a side pane.

#### Scenario: Preview shows file metadata and content
- **WHEN** a result is selected
- **THEN** the preview pane displays: full file path, line number range (e.g., `src/main.rs:42-58`), and the complete chunk content

#### Scenario: Code syntax highlighting
- **WHEN** the previewed file is a code file
- **THEN** the content is rendered with syntax highlighting appropriate to the file extension
- **AND** the highlighting preserves original indentation and formatting

#### Scenario: Scroll independence
- **WHEN** the preview content exceeds the pane height
- **THEN** the pane supports independent vertical scrolling
- **AND** scrolling the preview does not affect the results list scroll position

### Requirement: Quick actions
The system SHALL provide action buttons for common operations on the selected result.

#### Scenario: Copy action
- **WHEN** the user clicks the "Copy" action button
- **THEN** the chunk content is copied to the clipboard
- **AND** visual feedback confirms the action

#### Scenario: Open file action
- **WHEN** the user clicks the "Open File" action button
- **THEN** the system default application opens the source file

#### Scenario: Reveal in Finder action
- **WHEN** the user clicks the "Reveal in Finder" action button
- **THEN** macOS Finder opens to the containing folder with the file selected

