# Design: desktop-pin-and-glob-filter

## Context

PRD 003 §"Open Questions" decided that the desktop palette should:

1. Let users pin search results, persisted locally on a single device.
2. Let the RAG Lab `filter_file_type` accept glob patterns (not raw extensions or regex).

This design covers both because they touch overlapping surfaces (search result rendering, Tauri command layer, SQLite schema) and decompose poorly into separate changes without redundant scaffolding.

## Goals

- Add pin/unpin without any cross-device assumptions (Phase 3 sync stays out of scope).
- Replace the file-type multi-select with glob input without breaking existing callers of `search_knowledge`.
- Keep all new logic in the Rust core; the frontend only renders state and dispatches commands.

## Non-Goals

- Cross-device pin synchronization (Phase 3 / The Spine).
- Pin metadata richer than `(chunk_id, pinned_at)` — no labels, folders, or notes.
- Regex support in `filter_file_type` (explicitly out per PRD decision).
- Migrating other callers (e.g., MCP `search_knowledge`) to glob beyond the minimum needed for consistency.

## Architecture

### Data layer (`syncmind-storage`)

The existing storage layer initializes schema via `VectorStore::init_schema` using `CREATE TABLE IF NOT EXISTS`. We follow the same pattern (no separate migration framework, consistent with Phase 1) and append:

```sql
CREATE TABLE IF NOT EXISTS pinned_chunks (
    chunk_id   INTEGER PRIMARY KEY,
    pinned_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_pinned_chunks_pinned_at ON pinned_chunks(pinned_at DESC);
```

`chunk_id` is `INTEGER` to match the existing `chunks(id)` primary key type. `pinned_at` is a Unix epoch seconds integer (matching `last_modified` / `last_indexed` in `files`). `FOREIGN KEY` enforcement requires `PRAGMA foreign_keys = ON`, which `VectorStore::new` already sets.

Why `ON DELETE CASCADE`: re-indexing or file deletion in the existing pipeline already removes rows from `chunks`. Cascade keeps pin state consistent without bespoke cleanup logic in any other crate.

New methods on `VectorStore` (mirrors existing API style — no separate trait, consistent with `delete_file_by_path` / `search_*`):

```rust
impl VectorStore {
    pub fn pin_chunk(&self, chunk_id: i64) -> Result<(), StorageError>;          // idempotent
    pub fn unpin_chunk(&self, chunk_id: i64) -> Result<(), StorageError>;        // idempotent
    pub fn list_pinned_chunks(&self) -> Result<Vec<SearchResult>, StorageError>; // ordered by pinned_at DESC
    pub fn is_chunk_pinned(&self, chunk_id: i64) -> Result<bool, StorageError>;
    pub fn pinned_set(&self, chunk_ids: &[i64]) -> Result<HashSet<i64>, StorageError>;
}
```

`pinned_set` is the bulk lookup used when rendering search results, so the palette does not issue N `is_pinned` queries per render. `list_pinned_chunks` joins `pinned_chunks` with `chunks` and `files` to return the full `SearchResult` payload (matching `search` output). Pinned results carry `score = 1.0` as a synthetic value, since they bypass vector ranking.

### Filter evaluation (`syncmind-rag-engine`)

`SearchQuery` gains a unified filter field:

```rust
pub struct SearchQuery {
    pub query: String,
    pub top_k: usize,
    pub file_filter: Option<FileFilter>,
}

pub enum FileFilter {
    Globs(GlobSet),       // produced from validated user input
}
```

A helper `parse_file_filter(patterns: &[String]) -> Result<FileFilter, FilterError>` accepts the user-provided strings, applies a shorthand transformation, and returns either a compiled `GlobSet` or a structured error.

**Shorthand rule (compatibility):** patterns that contain none of `*`, `?`, `[`, `{` are treated as extensions — `"rs"` becomes `"**/*.rs"`, `"md"` becomes `"**/*.md"`. This keeps the existing MCP and Tauri callers (which pass bare extensions) working without breaking changes.

The actual match runs against the absolute file path of each candidate chunk.

### Tauri command surface

```rust
#[tauri::command]
async fn pin_chunk(chunk_id: i64, state: State<AppState>) -> Result<(), String>;

#[tauri::command]
async fn unpin_chunk(chunk_id: i64, state: State<AppState>) -> Result<(), String>;

#[tauri::command]
async fn list_pinned_chunks(state: State<AppState>) -> Result<Vec<SearchResult>, String>;

#[tauri::command]
async fn is_chunk_pinned(chunk_id: i64, state: State<AppState>) -> Result<bool, String>;

#[tauri::command]
async fn validate_file_filter(patterns: Vec<String>) -> Result<(), String>;
```

`validate_file_filter` is exposed so the frontend can render per-chip validation feedback without round-tripping through a full `search_knowledge` invocation.

The existing `search_knowledge` command keeps its signature; internally it now routes `filter_file_type` through `parse_file_filter`. If parsing fails, the command returns `Err` rather than silently falling back, since RAG Lab UI is responsible for filtering invalid chips before invocation.

### Frontend (apps/desktop)

- **Pin icon on results**: new `<PinIcon pinned={bool} onClick={...} />` rendered at the trailing edge of every result row in both the search list and the Pinned tab.
- **Pinned tab**: new top-level navigation entry sibling to "Search" / "RAG Lab" / "Settings"; hotkey `Cmd+Shift+P` toggles to it; empty state copy is "No pinned items yet. Press Cmd+P on a search result to pin it."
- **Glob chip input**: replaces the multi-select in RAG Lab. On `Enter`, the candidate pattern is validated via `validate_file_filter([candidate])` before being added as a chip. Invalid chips are rejected with an inline error message.

### State management

Pin status enters SolidJS `createStore` as a `Set<chunk_id>` that the palette refreshes whenever results render. Toggling a pin issues the Tauri command and optimistically updates the set; on error, the optimistic update is reverted and a toast is shown.

## Trade-offs and Alternatives Considered

| Decision | Alternative | Reason for choice |
|---|---|---|
| Single `pinned_chunks` table | Add `is_pinned` column to `chunks` | Pin is sparse and orthogonal to chunk lifecycle; a separate table avoids touching the hottest table in the system. |
| `ON DELETE CASCADE` from chunks | Periodic cleanup job | Cascade is atomic, free, and runs at the right granularity (per chunk delete). A cleanup job adds latency between re-index and UI consistency. |
| `globset` only | `regex` + `globset` dual mode | PRD explicitly chose glob-only. Regex would double UI complexity and invites footgun patterns. |
| Bare extensions → `**/*.<ext>` shorthand | Break existing callers, force a migration | Existing MCP callers send `["rs", "md"]`; preserving the contract avoids a flag day. |
| `validate_file_filter` as separate command | Re-use `search_knowledge` with empty query | A no-op search still spins up the vector path; validation should be cheap and free of side effects. |
| Optimistic pin toggle in UI | Always wait for backend ack | Pin is a cheap local SQLite insert (<5ms); optimistic UX matches palette responsiveness budget; backend error case is rare and recoverable. |

## Open Questions for Implementation

- Should the Pinned tab show a per-row "pinned X ago" timestamp? **Defer**: not a blocker; can be added without spec change.
- Should `Cmd+P` collision with browser-style "print" be configurable via Settings? **Defer**: keymap customization is its own future change.

## Verification Plan

- Unit tests in `syncmind-storage` covering migration application, idempotent pin/unpin, cascade behavior on chunk deletion, and `pinned_set` correctness.
- Unit tests in `syncmind-rag-engine` for `parse_file_filter` covering: bare extension shorthand, plain glob (`*.rs`), recursive glob (`**/*.md`), brace expansion (`src/**/*.{ts,tsx}`), invalid patterns, and OR semantics with multiple globs.
- Integration test: end-to-end `search_knowledge` invocation with mixed extension and glob inputs against a seeded fixture index.
- Manual smoke test in the Tauri dev window: pin a result, restart the app, verify it persists; delete the underlying file and confirm the pin disappears on re-index.
