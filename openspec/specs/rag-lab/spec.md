# rag-lab Specification

## Purpose
TBD - created by archiving change the-command-palette. Update Purpose after archive.
## Requirements
### Requirement: Parameter tuning controls
The system SHALL provide a RAG Lab panel where users can adjust search parameters that affect the underlying vector retrieval.

#### Scenario: Top-K slider adjustment
- **WHEN** the user navigates to the RAG Lab panel
- **THEN** a `top_k` slider is visible with a range from 1 to 20
- **AND** the default value is 5
- **AND** adjusting the slider immediately updates subsequent search queries without requiring an app restart

#### Scenario: File type filter selection
- **WHEN** the user views the RAG Lab panel
- **THEN** a multi-select control lists all file types currently present in the index
- **AND** selecting one or more types restricts search results to those types
- **AND** deselecting all types removes the filter

#### Scenario: Parameter reset
- **WHEN** the user clicks the "Reset" button in the RAG Lab panel
- **THEN** `top_k` returns to 5
- **AND** all file type filters are cleared

### Requirement: Debug telemetry display
The system SHALL expose real-time telemetry for the last executed search query.

#### Scenario: Query latency shown
- **WHEN** a search completes
- **THEN** the panel displays the round-trip query latency in milliseconds

#### Scenario: Result count shown
- **WHEN** a search completes
- **THEN** the panel displays the number of results returned

#### Scenario: Embedding model info shown
- **WHEN** the RAG Lab panel is open
- **THEN** it displays the active embedding model name (e.g., `bge-m3`) and its vector dimension

### Requirement: Raw JSON inspection
The system SHALL allow advanced users to inspect the raw backend response.

#### Scenario: Toggle raw JSON view
- **WHEN** the user clicks "Show Raw JSON" in the RAG Lab panel
- **THEN** an expandable/collapsible code block appears containing the serialized JSON response from the last `search_knowledge` invocation
- **AND** the JSON is syntax-highlighted

