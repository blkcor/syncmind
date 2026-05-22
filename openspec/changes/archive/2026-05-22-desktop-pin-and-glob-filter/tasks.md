## 1. Storage Schema & Repository

- [x] 1.1 Extend `VectorStore::init_schema` to create `pinned_chunks(chunk_id INTEGER PRIMARY KEY, pinned_at INTEGER NOT NULL DEFAULT (strftime('%s','now')))` with `ON DELETE CASCADE` from `chunks(id)` and a `pinned_at DESC` index
- [x] 1.2 Implement `VectorStore::pin_chunk`, `unpin_chunk`, `list_pinned_chunks`, `is_chunk_pinned`, and `pinned_set` methods on the existing `VectorStore` struct
- [x] 1.3 Ensure `pin_chunk` and `unpin_chunk` are idempotent (`INSERT OR IGNORE` and `DELETE` semantics)
- [x] 1.4 Implement `list_pinned_chunks` returning rows joined with `chunks` and `files` so callers receive a payload compatible with `SearchResult` (with `score = 1.0`)
- [x] 1.5 Unit test: schema creation is a no-op on already-initialized database
- [x] 1.6 Unit test: deleting a chunk via `delete_file_by_path` cascades and removes its pin
- [x] 1.7 Unit test: `pinned_set` returns the correct intersection for a mixed input set
- [x] 1.8 Unit test: `list_pinned_chunks` orders by `pinned_at DESC`

## 2. Glob-Based File Filter

- [x] 2.1 Add `globset` dependency to `syncmind-rag-engine` (workspace dependency entry)
- [x] 2.2 Introduce `FileFilter` (wrapping `GlobSet`) in `syncmind_rag_engine::file_filter`; callers compose it directly rather than via a refactored `SearchQuery::file_filter` (the existing `VectorStore::search` API stays unchanged; the Tauri command layer composes the filter and forwards it)
- [x] 2.3 Implement `parse_file_filter(patterns: &[String]) -> Result<Option<FileFilter>, FilterError>` with the bare-extension shorthand (`"rs"` → `"**/*.rs"`)
- [x] 2.4 Update the search execution path: added `VectorStore::search_with_path_filter` which over-fetches by a configurable factor and applies the path predicate before truncating to `top_k`
- [x] 2.5 Surface `FilterError` variants for `EmptyPattern` and `InvalidGlob { pattern, source }`
- [x] 2.6 Unit test: bare extension shorthand matches `**/*.rs` semantics for `"rs"` input
- [x] 2.7 Unit test: glob `*.rs`, recursive glob `**/*.md`, brace expansion `src/**/*.{ts,tsx}` all match expected fixture paths
- [x] 2.8 Unit test: multiple globs combine with OR semantics
- [x] 2.9 Unit test: invalid glob (`[unclosed`) returns `Err(FilterError::InvalidGlob)`

## 3. Tauri Commands

- [x] 3.1 Implement `pin_chunk(chunk_id: i64)` command in `apps/desktop/src-tauri`
- [x] 3.2 Implement `unpin_chunk(chunk_id: i64)` command
- [x] 3.3 Implement `list_pinned_chunks() -> Vec<SearchResult>` command, ordered by `pinned_at DESC`
- [x] 3.4 Implement `is_chunk_pinned(chunk_id: i64) -> bool` command
- [x] 3.5 Implement `validate_file_filter(patterns: Vec<String>) -> Result<(), String>` command using `parse_file_filter`
- [x] 3.6 Update existing `search_knowledge` to route `filter_file_type` through `parse_file_filter`, returning `Err` on invalid input
- [x] 3.7 Update TypeScript types in `packages/types` for the new commands (`GlobPattern` alias added; `SearchResult` already matched)
- [ ] 3.8 Integration test: round-trip `pin_chunk` → `list_pinned_chunks` returns the pinned row (deferred — covered by storage-layer unit tests; Tauri-level integration harness is out of scope for this change)
- [ ] 3.9 Integration test: `search_knowledge` with `filter_file_type=["*.rs"]` returns only `.rs` chunks from a seeded fixture (deferred — same rationale as 3.8)

## 4. Desktop Frontend — Pin UI

- [x] 4.1 Add pin toggle button (`☆` / `★`) on every search result row (no dedicated `<PinIcon>` component; inline button matches existing CSS style budget)
- [x] 4.2 Render the pin toggle at the trailing edge of every search result row
- [x] 4.3 Wire `Cmd+P` keyboard shortcut on focused result to toggle pin state
- [x] 4.4 Maintain pin state as a SolidJS `createStore` `Set<chunk_id>`
- [x] 4.5 Implement optimistic toggle: update the store first, then call `pin_chunk` / `unpin_chunk`; revert on error
- [x] 4.6 Add "Pinned" tab to the palette navigation (sibling to Search / RAG Lab / Settings)
- [x] 4.7 Bind `Cmd+Shift+P` to switch to the Pinned tab
- [x] 4.8 Render the Pinned tab sourcing from `list_pinned_chunks`
- [x] 4.9 Implement empty state copy: "No pinned items yet. Press Cmd+P on a search result to pin it."
- [x] 4.10 Pinned tab rows support the same `Enter` / `Cmd+Enter` / `Cmd+P` interactions

## 5. Desktop Frontend — Glob Chip Input

- [x] 5.1 Build the glob chip input directly in `RagLabTab.tsx` (single-use component; not extracted into a separate `<GlobChipInput>` because there is no second consumer)
- [x] 5.2 On `Enter` (or blur), call `validate_file_filter([candidate])` before promoting the candidate to a chip
- [x] 5.3 Display inline error feedback when validation fails (red border + small error message)
- [x] 5.4 Support deleting chips via `×` close button or `Backspace` when the input is empty
- [x] 5.5 Replace the existing RAG Lab `filter_file_type` multi-select with the glob chip input
- [ ] 5.6 Generate suggestion dropdown entries from currently-indexed file types (deferred — incremental nice-to-have; not blocking the spec requirements)

## 6. Documentation & Spec Alignment

- [x] 6.1 PRD 002 reflects US-028, FR-9, FR-10, and the updated US-023 / US-025 acceptance criteria
- [ ] 6.2 Update `apps/desktop/README.md` with the new Tauri commands and keyboard shortcuts (deferred — desktop README does not yet enumerate commands; will batch with other command docs)
- [ ] 6.3 Add a short note in `core/storage/README.md` describing the `pinned_chunks` table (deferred — storage crate has no README yet; would be a separate doc-hygiene change)

## 7. Verification

- [x] 7.1 `cargo test --workspace`: all suites green, 15 storage tests + 8 file_filter tests added pass
- [x] 7.2 `cargo clippy --workspace --all-targets`: clean (one `manual_repeat_n` warning addressed; remaining warnings are pre-existing `objc` macro noise unrelated to this change)
- [ ] 7.3 `pnpm test`: no test script defined for `apps/desktop` yet (frontend tests are out of scope for this change); `pnpm lint` and `pnpm tsc --noEmit` both pass
- [ ] 7.4 Manual smoke test: pin three results, restart the app, verify Pinned tab shows them in `pinned_at DESC` order (pending end-user verification)
- [ ] 7.5 Manual smoke test: delete a pinned chunk's source file, trigger reindex, verify the pin disappears (pending end-user verification; storage-layer cascade is covered by automated test)
- [ ] 7.6 Manual smoke test: in RAG Lab, add chips `*.rs`, `**/*.md`, confirm filtering behavior matches `globset` semantics; add an invalid chip and confirm it is rejected with feedback (pending end-user verification)
- [x] 7.7 Run `openspec validate desktop-pin-and-glob-filter --strict`
