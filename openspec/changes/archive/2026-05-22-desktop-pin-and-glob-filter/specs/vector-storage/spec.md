## ADDED Requirements

### Requirement: Pinned chunks table
The vector storage layer SHALL provide a `pinned_chunks` table that records which chunks have been pinned by the user, with a foreign-key cascade from the `chunks` table.

#### Scenario: Schema creation
- **WHEN** the storage layer initializes against a database lacking the table
- **THEN** the system SHALL create `pinned_chunks` with columns `chunk_id INTEGER PRIMARY KEY` and `pinned_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))`
- **AND** the system SHALL declare a `FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE` constraint
- **AND** the system SHALL create an index on `pinned_at DESC`

#### Scenario: Schema initialization is idempotent
- **WHEN** the storage layer initializes against a database that already contains the table
- **THEN** the system SHALL succeed without error and SHALL NOT mutate existing rows

#### Scenario: Cascade on chunk deletion
- **WHEN** a row is deleted from the `chunks` table (e.g. by re-indexing or file removal)
- **THEN** any matching row in `pinned_chunks` SHALL be removed by the same transaction

