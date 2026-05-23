## ADDED Requirements

### Requirement: Indexed-extension suggestion dropdown

The RAG Lab glob chip input SHALL surface a suggestion dropdown derived from file extensions currently present in the vector store, so users can discover and apply common patterns without having to recall the exact extensions they have indexed.

#### Scenario: Dropdown populated from indexed extensions

- **WHEN** the user navigates to the RAG Lab tab
- **AND** the vector store contains at least one indexed file with an extension
- **THEN** the suggestion dropdown SHALL contain one entry per distinct extension, formatted as `*.<ext>` (e.g. `*.md`, `*.rs`)
- **AND** entries SHALL be sorted alphabetically ascending
- **AND** entries SHALL be deduplicated case-insensitively (lowercased)

#### Scenario: Suggestion excludes already-applied chips

- **WHEN** the dropdown is visible
- **AND** a suggestion's `*.<ext>` form already exists as a chip in `store.ragLab.fileTypeFilters`
- **THEN** that suggestion SHALL NOT appear in the dropdown

#### Scenario: Substring filtering while typing

- **WHEN** the user types one or more characters in the glob input field
- **THEN** the dropdown SHALL display only suggestions whose `*.<ext>` form contains the typed text as a case-insensitive substring
- **AND** when the input is empty, the dropdown SHALL display all non-excluded suggestions

#### Scenario: Clicking a suggestion adds it as a chip

- **WHEN** the user clicks a suggestion
- **THEN** the suggestion's `*.<ext>` text SHALL be passed through `validate_file_filter` and added as a chip
- **AND** the input field SHALL be cleared
- **AND** input focus SHALL NOT be lost before the chip is added (the suggestion handler SHALL run before any blur-triggered side effects)

#### Scenario: Empty index produces no dropdown

- **WHEN** the user navigates to the RAG Lab tab
- **AND** the vector store contains zero indexed files
- **THEN** the dropdown SHALL render no entries
- **AND** the input field SHALL remain usable for manual entry

#### Scenario: Storage error degrades gracefully

- **WHEN** the suggestion fetch fails (storage unavailable, command not registered, RPC error)
- **THEN** the dropdown SHALL render no entries
- **AND** the chip input SHALL remain fully functional for manual entry
- **AND** no error banner or toast SHALL be surfaced in the UI

#### Scenario: Refresh on tab mount

- **WHEN** the user navigates away from the RAG Lab tab and returns
- **THEN** the dropdown SHALL re-fetch suggestions from the current state of the vector store
- **AND** newly-indexed extensions added during the absence SHALL appear in the dropdown after return
