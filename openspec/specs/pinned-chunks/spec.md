# pinned-chunks Specification

## Purpose
TBD - created by archiving change desktop-pin-and-glob-filter. Update Purpose after archive.
## Requirements
### Requirement: Local persistence of pinned chunks
The system SHALL persist pinned chunks in the local SQLite database such that pin state survives application restarts and is never transmitted off-device.

#### Scenario: Pin survives restart
- **WHEN** a user pins a chunk and the application is restarted
- **THEN** the chunk remains pinned and is visible in the Pinned tab on next launch

#### Scenario: Pin state is local only
- **WHEN** a chunk is pinned
- **THEN** no HTTP, MCP, or sync transport sends the pin operation off the device

### Requirement: Pin and unpin operations are idempotent
The system SHALL allow `pin_chunk` and `unpin_chunk` operations to be invoked repeatedly without error.

#### Scenario: Repeated pin
- **WHEN** `pin_chunk(chunk_id)` is invoked on a chunk that is already pinned
- **THEN** the operation SHALL succeed
- **AND** the `pinned_at` timestamp SHALL NOT be modified

#### Scenario: Repeated unpin
- **WHEN** `unpin_chunk(chunk_id)` is invoked on a chunk that is not pinned
- **THEN** the operation SHALL succeed without raising an error

### Requirement: Cascade cleanup of stale pins
The system SHALL automatically remove pin rows whose underlying chunk has been deleted from the index.

#### Scenario: Re-indexing removes obsolete pins
- **WHEN** a chunk that had been pinned is removed from the `chunks` table by re-indexing or file deletion
- **THEN** the corresponding `pinned_chunks` row SHALL be removed atomically by the database
- **AND** the Pinned tab SHALL not display the stale entry on next render

### Requirement: Pinned chunks listing
The system SHALL expose a query that returns pinned chunks in `pinned_at DESC` order with full result metadata.

#### Scenario: List pinned chunks
- **WHEN** `list_pinned_chunks()` is invoked
- **THEN** the response SHALL contain every currently-pinned chunk
- **AND** the items SHALL be ordered by `pinned_at` descending (most recently pinned first)
- **AND** each item SHALL include the same fields as a search result (`chunk_id`, `file_path`, `start_line`, `end_line`, `content`)

### Requirement: Bulk pin-state lookup
The system SHALL expose a bulk lookup that returns the subset of given chunk identifiers that are currently pinned, so search-result rendering can decorate pin state without N round-trips.

#### Scenario: Lookup pin state for a result page
- **WHEN** the palette renders a page of search results
- **THEN** the system SHALL be able to determine the pin state of all displayed chunks in a single backend call

### Requirement: Pin Tauri command surface
The desktop application SHALL expose Tauri commands `pin_chunk`, `unpin_chunk`, `list_pinned_chunks`, and `is_chunk_pinned` that the frontend uses to manage pin state.

#### Scenario: Pin command available to frontend
- **WHEN** the SolidJS frontend invokes `pin_chunk(chunk_id)` via the Tauri bridge
- **THEN** the command SHALL persist the pin and return `Ok(())`

#### Scenario: List command returns ordered payload
- **WHEN** the frontend invokes `list_pinned_chunks()`
- **THEN** the command SHALL return a `Vec<SearchResult>` ordered by `pinned_at` descending

### Requirement: Pin metadata schema is minimal and forward-compatible
The pinned-chunks table SHALL store only `chunk_id` (matching the type of `chunks.id`, currently `INTEGER`) and `pinned_at`. Cross-device synchronization fields SHALL NOT be added in this change.

#### Scenario: Schema scope
- **WHEN** the pin schema is initialized
- **THEN** the resulting table SHALL contain exactly two user-defined columns (`chunk_id`, `pinned_at`)
- **AND** SHALL NOT introduce columns for user identity, sync state, labels, or notes

