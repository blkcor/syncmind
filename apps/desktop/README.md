# SyncMind Desktop

Tauri + SolidJS command palette for searching the local SyncMind index, pinning
useful chunks, and testing retrieval settings in RAG Lab.

## Tauri Commands

The desktop shell invokes these pin and filter commands from the Rust backend:

| Command | Purpose |
|---------|---------|
| `pin_chunk(chunk_id)` | Adds a chunk to the local pinned list. The operation is idempotent. |
| `unpin_chunk(chunk_id)` | Removes a chunk from the local pinned list. The operation is idempotent. |
| `is_chunk_pinned(chunk_id)` | Returns whether a chunk is currently pinned. |
| `list_pinned_chunks()` | Returns pinned chunks as search-result rows ordered by newest pin first. |
| `list_indexed_file_types()` | Returns distinct indexed file extensions for the RAG Lab file-filter autocomplete. |
| `validate_file_filter(patterns)` | Validates RAG Lab file-filter glob patterns without running a search. |

`search_knowledge` accepts `filter_file_type` as glob patterns. Bare extension
values such as `rs` are still supported and are expanded by the backend as
`**/*.rs`.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+P` / `Ctrl+P` | Toggle the selected search result's pinned state. |
| `Cmd+Shift+P` / `Ctrl+Shift+P` | Open the Pinned tab. |

The Pinned tab uses the same result-row interactions as Search, including
opening a selected result and unpinning with the pin shortcut.
