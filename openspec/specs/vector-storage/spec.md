# vector-storage Specification

## Purpose
SQLite + sqlite-vec backed local store for file metadata, semantic chunks, and embedding vectors. Provides transactional upsert, similarity search, statistics, and per-file deletion.

## Requirements

### Requirement: Delete file artifacts by absolute path
The store SHALL provide `VectorStore::delete_file_by_path(&self, path: &Path) -> Result<bool, StorageError>` that transactionally removes all storage artifacts associated with a file: vec_chunks rows, chunks rows, and the files row.

#### Scenario: Delete clears vec_chunks, chunks, and files
- **WHEN** `delete_file_by_path` is invoked with a path that has been indexed
- **THEN** the method returns `Ok(true)`
- **AND** `SELECT COUNT(*) FROM files WHERE absolute_path = ?` returns 0
- **AND** `SELECT COUNT(*) FROM chunks WHERE file_id = ?` returns 0 (via FK cascade)
- **AND** `SELECT COUNT(*) FROM vec_chunks WHERE chunk_id IN (…)` returns 0 (explicit deletion, since sqlite-vec virtual tables do not honor FK cascade)

#### Scenario: Delete is idempotent for unknown paths
- **WHEN** `delete_file_by_path` is invoked with a path that has never been indexed
- **THEN** the method returns `Ok(false)`
- **AND** no rows are modified

#### Scenario: Delete is transactional under failure
- **WHEN** an error occurs mid-deletion (e.g., simulated I/O failure between vec_chunks delete and files delete)
- **THEN** the transaction rolls back
- **AND** the partial deletion is not visible to subsequent searches

### Requirement: Index cleanup on file removal events
The indexing pipeline SHALL invoke `delete_file_by_path` when receiving a `FileEvent::Remove` from the file watcher, ensuring that deleted files no longer surface in `search_knowledge` results.

#### Scenario: Search excludes deleted files after debounce
- **WHEN** a registered file is deleted on disk
- **AND** the debounce window elapses
- **THEN** `search_knowledge` queries that previously matched the file's chunks no longer return them

### Requirement: Unregister cleans up the index
The `syncmind unregister <path>` CLI command SHALL invoke `delete_file_by_path` after removing the path from `registered_files`, so that unregistered files do not leave orphaned chunks in the database.

#### Scenario: Unregister removes both config and index entries
- **WHEN** the user runs `syncmind unregister /abs/path/to/file.md`
- **THEN** the path is removed from `registered_files` in `config.toml`
- **AND** `delete_file_by_path` is invoked
- **AND** `syncmind status` shows the file is no longer counted in `Indexed files`
