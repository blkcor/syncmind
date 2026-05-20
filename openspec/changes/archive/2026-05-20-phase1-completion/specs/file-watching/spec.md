# file-watching Specification (Modified)

## Purpose
Watch registered files for create, modify, delete, and rename events; emit semantically classified events so downstream consumers (indexing pipeline, desktop app) can correctly route work between "re-index" and "remove from index."

## Requirements

### Requirement: Semantic event classification
The watcher SHALL emit `FileEvent` values that distinguish content updates from removals, replacing the previous behavior of forwarding raw paths.

#### Scenario: File modification emits Upsert
- **WHEN** a registered file's content is modified
- **THEN** the next debounced batch contains `FileEvent::Upsert(path)` for that file

#### Scenario: File deletion emits Remove
- **WHEN** a registered file is deleted from disk
- **THEN** the next debounced batch contains `FileEvent::Remove(path)` for that file
- **AND** the event is not silently dropped by an `is_file()` filter

#### Scenario: File rename emits Remove(old) + Upsert(new)
- **WHEN** a registered file is renamed from `old.md` to `new.md`
- **THEN** the next debounced batch contains both `FileEvent::Remove(old.md)` and `FileEvent::Upsert(new.md)`

#### Scenario: Debounce deduplication preserves final state
- **WHEN** a single file receives Modify → Remove → Create within the debounce window
- **THEN** the emitted event reflects the final on-disk state (Upsert if the file exists at debounce flush, Remove otherwise)
