## ADDED Requirements

### Requirement: Glob validation Tauri command
The system SHALL expose a `validate_file_filter(patterns)` Tauri command that the frontend can call to validate user input without executing a search.

#### Scenario: Valid pattern accepted
- **WHEN** `validate_file_filter(["*.rs"])` is invoked
- **THEN** the command SHALL return `Ok(())`

#### Scenario: Invalid pattern rejected
- **WHEN** `validate_file_filter(["[unclosed"])` is invoked
- **THEN** the command SHALL return `Err` with a human-readable message describing the invalid pattern

## MODIFIED Requirements

### Requirement: Parameter tuning controls
The system SHALL provide a RAG Lab panel where users can adjust search parameters that affect the underlying vector retrieval. The file-type filter SHALL accept glob patterns.

#### Scenario: Top-K slider adjustment
- **WHEN** the user navigates to the RAG Lab panel
- **THEN** a `top_k` slider is visible with a range from 1 to 20
- **AND** the default value is 5
- **AND** adjusting the slider immediately updates subsequent search queries without requiring an app restart

#### Scenario: Glob chip input for file filter
- **WHEN** the user views the RAG Lab panel
- **THEN** the file-type filter SHALL be rendered as a chip input where each chip represents one glob pattern
- **AND** patterns SHALL be evaluated against the absolute file path of each candidate chunk
- **AND** multiple chips SHALL combine with OR semantics (a chunk matches if any chip's glob matches)
- **AND** removing all chips SHALL remove the filter entirely

#### Scenario: Glob validation before chip creation
- **WHEN** the user enters a candidate pattern in the glob input field and confirms with `Enter`
- **THEN** the candidate SHALL be validated via `validate_file_filter` before being promoted to a chip
- **AND** invalid candidates SHALL NOT be added as chips
- **AND** invalid candidates SHALL display an inline error message describing why the pattern is invalid

#### Scenario: Bare extension shorthand
- **WHEN** a chip's pattern contains no glob metacharacters (`*`, `?`, `[`, `{`) and is interpreted as a bare extension
- **THEN** the system SHALL treat it as `**/*.<pattern>` for matching purposes
- **AND** existing callers that pass bare extensions SHALL continue to function

#### Scenario: Parameter reset
- **WHEN** the user clicks the "Reset" button in the RAG Lab panel
- **THEN** `top_k` returns to 5
- **AND** all file-filter chips are cleared
