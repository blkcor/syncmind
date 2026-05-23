## Why

The RAG Lab glob chip input (delivered in `desktop-pin-and-glob-filter`) accepts arbitrary user-typed patterns but provides no discovery aid. Users must remember which file extensions are present in their index — typing `*.foo` against an empty index produces silent zero-result confusion, and discovering the right extension requires opening `config.toml` or running a search. PRD 002 US-025 explicitly calls for "下拉建议：基于当前索引中实际出现的文件类型生成常用 pattern (如检测到 `.rs` 文件就建议 `*.rs`)", but `desktop-pin-and-glob-filter` deferred task 5.6 as an incremental nice-to-have. This change closes that gap.

## What Changes

- Add `VectorStore::list_distinct_extensions() -> Result<Vec<String>, StorageError>` to surface the set of file extensions currently present in the index, lowercased, sorted ascending, deduplicated.
- Add a `list_indexed_extensions() -> Vec<String>` Tauri command in `apps/desktop/src-tauri` that wraps the storage method and returns plain extension strings (without the leading dot).
- Expose the new command to the frontend via `packages/types` and the existing Tauri command surface.
- Extend `RagLabTab.tsx` with an inline suggestion dropdown rendered beneath the glob chip input:
  - Suggestions are derived as `*.{ext}` from `list_indexed_extensions`, sorted alphabetically.
  - Suggestions that are already present as chips are excluded.
  - When the input is non-empty, suggestions are filtered by substring match (case-insensitive).
  - Clicking or pressing `Enter` on a focused suggestion adds it as a chip (reusing the existing `validate_file_filter` + `addChip` path, so validation remains the single source of truth).
- Refresh the suggestion source each time the RAG Lab tab mounts so it reflects the current index contents.
- Fail silently when the suggestion fetch errors (e.g., empty index, storage unavailable): the dropdown simply renders empty rather than breaking the input.

## Capabilities

### New Capabilities

*(None — this change extends existing capabilities without introducing a new one.)*

### Modified Capabilities

- `rag-lab`: Add a new requirement covering the indexed-extension suggestion dropdown attached to the glob chip input.
- `vector-storage`: Add a new requirement covering `list_distinct_extensions` so the suggestion source is a documented public API of the store rather than a desktop-side implementation detail.

## Impact

- **Code**:
  - `core/storage/src/store.rs` — new `list_distinct_extensions` method + unit tests.
  - `apps/desktop/src-tauri/src/commands.rs` — new `list_indexed_extensions` command.
  - `apps/desktop/src-tauri/src/lib.rs` — register the new command in `invoke_handler!`.
  - `apps/desktop/src/components/RagLabTab.tsx` — suggestion dropdown UI + focus/blur handling.
  - `apps/desktop/src/styles.css` — `.glob-suggestions` + `.glob-suggestion` styles.
  - `packages/types/src/index.ts` — TypeScript signature for the new command (if maintained manually).
- **Dependencies**: None.
- **Privacy**: The new command returns only file extensions (e.g., `["md", "rs"]`), never paths or contents. No new privacy surface.
- **Performance**: `list_distinct_extensions` scans `files.path` once per RAG Lab mount; for an index of <10k files the query completes in <5ms locally. No watcher / pipeline impact.
- **Compatibility**: Additive only. Existing `search_knowledge`, `validate_file_filter`, chip input, and storage APIs are unchanged. Frontend gracefully degrades when the command is unavailable (older daemon builds).
