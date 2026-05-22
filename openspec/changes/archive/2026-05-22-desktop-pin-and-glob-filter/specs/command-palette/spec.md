## ADDED Requirements

### Requirement: Pin toggle on search results
The system SHALL render a pin toggle on every search result row and allow the user to pin or unpin via mouse or keyboard.

#### Scenario: Pin icon reflects state
- **WHEN** a search result is rendered
- **THEN** a pin icon SHALL appear at the trailing edge of the row
- **AND** the icon SHALL be filled when the chunk is pinned and empty when it is not

#### Scenario: Toggle via click
- **WHEN** the user clicks the pin icon on a result row
- **THEN** the chunk's pin state SHALL be toggled
- **AND** the icon SHALL update immediately to reflect the new state

#### Scenario: Toggle via keyboard
- **WHEN** the user presses `Cmd+P` while a result row is selected
- **THEN** the chunk's pin state SHALL be toggled
- **AND** the icon SHALL update immediately

#### Scenario: Optimistic update with rollback
- **WHEN** a pin toggle is initiated
- **THEN** the UI SHALL update optimistically before the backend confirms
- **AND** if the backend call fails, the UI SHALL revert and display a toast describing the error

### Requirement: Pinned tab in the palette
The system SHALL provide a top-level Pinned tab in the command palette that lists all pinned chunks using the same result row component as search results.

#### Scenario: Open Pinned tab via shortcut
- **WHEN** the user presses `Cmd+Shift+P` inside the palette
- **THEN** the palette SHALL switch to the Pinned tab

#### Scenario: Pinned tab content and ordering
- **WHEN** the Pinned tab is shown
- **THEN** it SHALL list every currently-pinned chunk ordered by `pinned_at` descending
- **AND** each row SHALL support the same `Enter`, `Cmd+Enter`, and `Cmd+P` interactions as search results

#### Scenario: Empty Pinned tab
- **WHEN** the Pinned tab is shown and no chunks are pinned
- **THEN** the tab SHALL display the empty-state message "No pinned items yet. Press Cmd+P on a search result to pin it."

## MODIFIED Requirements

### Requirement: Keyboard navigation
The system SHALL allow full keyboard control of the results list, including pin toggling and tab switching.

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

#### Scenario: Cmd+P toggles pin
- **WHEN** the user presses `Cmd+P` on a selected result
- **THEN** the result's pin state is toggled

#### Scenario: Cmd+Shift+P opens Pinned tab
- **WHEN** the user presses `Cmd+Shift+P` anywhere in the palette
- **THEN** the palette switches to the Pinned tab
