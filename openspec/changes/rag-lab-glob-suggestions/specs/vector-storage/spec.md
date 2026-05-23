## ADDED Requirements

### Requirement: Enumerate distinct file extensions in the index

The vector storage layer SHALL provide `VectorStore::list_distinct_extensions(&self) -> Result<Vec<String>, StorageError>` returning the set of file extensions currently present in the `files` table, so that consumer surfaces (such as the RAG Lab suggestion dropdown) can populate discovery aids without exposing absolute file paths.

#### Scenario: Returns distinct lowercase extensions

- **WHEN** the `files` table contains paths whose extensions are `rs`, `RS`, `md`, and `Py`
- **AND** `list_distinct_extensions` is invoked
- **THEN** the method SHALL return `Ok(vec!["md".to_string(), "py".to_string(), "rs".to_string()])`
- **AND** entries SHALL be lowercased
- **AND** entries SHALL be sorted in ascending lexicographic order
- **AND** duplicates introduced by case differences SHALL be collapsed into a single entry

#### Scenario: Skips files without an extension

- **WHEN** the `files` table contains paths such as `/abs/path/README`, `/abs/path/Makefile`, and `/abs/path/.gitignore`
- **AND** `list_distinct_extensions` is invoked
- **THEN** the returned vector SHALL NOT contain entries derived from these paths
- **AND** the method SHALL NOT return an error

#### Scenario: Empty index returns empty vector

- **WHEN** the `files` table contains zero rows
- **AND** `list_distinct_extensions` is invoked
- **THEN** the method SHALL return `Ok(vec![])`

#### Scenario: Returns extensions exclusively from the `files` table

- **WHEN** a path is present in the configuration's `registered_files` list but has not yet been written to the `files` table (e.g., indexing has not completed)
- **AND** `list_distinct_extensions` is invoked
- **THEN** the extension of that path SHALL NOT appear in the returned vector
