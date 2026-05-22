## Why

PRD 002 (`docs/prd/002-the-command-palette.md`) closed two open questions whose decisions are not yet reflected in the OpenSpec capabilities:

1. **Pin / favorites for search results** — users want to retain frequently used chunks so the next palette invocation does not require re-searching.
2. **Glob-based `filter_file_type`** — the existing multi-select control only enumerates raw extensions present in the index, which cannot express "all Rust files under `src/`" or "any Markdown anywhere".

Both decisions are scoped to the desktop shell (Phase 2) and stay aligned with the privacy-first, local-only architecture: pin state lives in the local SQLite database, glob evaluation happens entirely in the Rust core.

## What Changes

- Add a `pinned_chunks` table to the local SQLite database, created via the existing `VectorStore::init_schema` using `CREATE TABLE IF NOT EXISTS`. Schema is intentionally minimal: `(chunk_id INTEGER PRIMARY KEY, pinned_at INTEGER)`, with `ON DELETE CASCADE` from `chunks(id)` so re-indexing automatically removes stale pins.
- Expose four new Tauri Commands: `pin_chunk`, `unpin_chunk`, `list_pinned_chunks`, `is_chunk_pinned`. All commands are idempotent and surface backend errors as typed results.
- Extend the command palette UI:
  - Show a Pin toggle icon on every search result; toggle via click or `Cmd+P`.
  - Add a "Pinned" tab (or `Cmd+Shift+P`) that lists pinned chunks in `pinned_at DESC` order using the same result row component.
- Replace the RAG Lab `filter_file_type` multi-select with a glob chip input: each chip is a glob pattern, validated against `globset` semantics in the Rust backend before being accepted.
- Update `search_knowledge` semantics so `filter_file_type` patterns are evaluated against absolute file paths using `globset::GlobSet`, with OR semantics across patterns.
- No MCP protocol changes; no cross-device sync changes; no embedding pipeline changes.

## Capabilities

### New Capabilities

- `pinned-chunks`: Local single-device persistence and management of user-pinned search results, including storage schema, Tauri command surface, and palette UI integration.

### Modified Capabilities

- `command-palette`: Search result rows now expose pin state and a pin toggle interaction; keyboard navigation gains `Cmd+P` and `Cmd+Shift+P`.
- `rag-lab`: The file-type filter becomes glob-based with validation, replacing the index-derived multi-select.
- `vector-storage`: Adds a migration that creates the `pinned_chunks` table with a foreign-key cascade from `chunks(id)`; no changes to vector tables or existing schemas.

## Impact

- **Crates affected**:
  - `syncmind-storage` — new schema fragment in `init_schema`, new pin/unpin/list methods on `VectorStore`, glob-aware result filtering when integrated with `syncmind-rag-engine`.
  - `apps/desktop/src-tauri` — four new Tauri commands, wiring to the storage layer.
  - `syncmind-rag-engine` — search query type now carries a compiled `GlobSet` instead of raw `Vec<String>` extensions (or accepts both during transition).
- **New Rust dependencies**: `globset` (BSD-2 / MIT, already commonly used by `ignore`, `ripgrep`); no new heavy dependencies.
- **Frontend**: Pin icon + tab in `apps/desktop/src/`; glob chip input component in the RAG Lab panel; new TypeScript types in `packages/types`.
- **Privacy**: Pin state and glob patterns never leave the device; no network calls are introduced.
- **Compatibility**:
  - Existing `search_knowledge` callers passing raw extensions (e.g., `["rs", "md"]`) must continue to work. The backend will treat any pattern without glob metacharacters as a shorthand for `**/*.{<ext>}`.
  - Databases without the new table get it created on next `VectorStore::new` via the existing `init_schema` flow; absence of the table never blocks startup.
