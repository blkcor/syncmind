## 1. Storage Layer

- [x] 1.1 Add `VectorStore::list_distinct_extensions(&self) -> Result<Vec<String>, StorageError>` in `core/storage/src/store.rs`. Implementation: prepare `SELECT path FROM files`, iterate rows, extract extension via `Path::extension`, lowercase, collect into a `HashSet<String>`, then sort the resulting `Vec<String>` ascending.
- [x] 1.2 Skip rows whose path has no extension (`Path::extension` returns `None`) — these MUST NOT contribute to the result.
- [x] 1.3 Unit test: seed `files` with paths `a.rs`, `b.RS`, `c.md`, `D.PY`, `README`, `.gitignore`; assert `list_distinct_extensions()` returns `Ok(vec!["md", "py", "rs"])` exactly (case collapsed, no `README` / `.gitignore` artifacts, sorted).
- [x] 1.4 Unit test: empty `files` table returns `Ok(vec![])`.
- [x] 1.5 Unit test: registered-but-not-indexed path (i.e., in `Config::registered_files` but absent from `files`) does not contribute to the result.

## 2. Tauri Command

- [x] 2.1 Add `#[tauri::command] pub fn list_indexed_extensions(state: State<AppState>) -> Result<Vec<String>, String>` in `apps/desktop/src-tauri/src/commands.rs`, wrapping `state.store.list_distinct_extensions()` with the standard `map_err(|e| format!(...))` error formatting used elsewhere in this file.
- [x] 2.2 Register the new command in the `tauri::generate_handler![...]` macro invocation inside `apps/desktop/src-tauri/src/lib.rs`.
- [x] 2.3 Add a TypeScript signature for `list_indexed_extensions` in `packages/types/src/index.ts` (or wherever the existing Tauri command types live) returning `Promise<string[]>`. (Resolved inline at call site as `invoke<string[]>('list_indexed_extensions')` — matches existing convention; no commands carry typed signatures in `packages/types`.)
- [x] 2.4 Verify `cargo check -p syncmind-desktop` (or the desktop Tauri crate name in `apps/desktop/src-tauri/Cargo.toml`) passes after the command is registered.

## 3. Frontend — Suggestion Dropdown

- [x] 3.1 Add `createSignal<string[]>([])` for `suggestions` and `createSignal<boolean>(false)` for `focused` near the top of `RagLabTab.tsx`.
- [x] 3.2 In `onMount`, call `invoke<string[]>('list_indexed_extensions')`, map results to `*.{ext}` form, store via `setSuggestions`. Wrap in try/catch so any error sets `setSuggestions([])` (silent degradation per design D10).
- [x] 3.3 Add a derived signal `filteredSuggestions` that excludes patterns already present in `store.ragLab.fileTypeFilters` and applies a case-insensitive substring filter against `draftPattern()` (empty `draftPattern` returns all non-excluded suggestions).
- [x] 3.4 Render the dropdown as `<Show when={focused() && filteredSuggestions().length > 0}><ul class="glob-suggestions">…</ul></Show>` positioned beneath the `glob-input` element inside the `glob-field` label.
- [x] 3.5 Wire each suggestion entry's `onMouseDown` to (a) call `e.preventDefault()` to prevent input blur, (b) `setDraftPattern(suggestion)`, (c) `addChip()`. Do NOT use `onClick`.
- [x] 3.6 Add `onFocus={() => setFocused(true)}` and `onBlur={() => { setFocused(false); addChip(); }}` to the `glob-input` element. The blur handler order matters: hiding the dropdown must happen after the mouseDown side effect has completed (which it will, since `onMouseDown` fires before `blur`).
- [x] 3.7 Add CSS for `.glob-suggestions` (absolute-positioned list, max-height with overflow-y) and `.glob-suggestion` (single-line, hover background, cursor pointer) in `apps/desktop/src/styles.css`. Match the visual language of existing chip / list elements.

## 4. Verification

- [x] 4.1 `cargo test --workspace`: all suites green, including the three new `list_distinct_extensions` unit tests from §1.
- [x] 4.2 `cargo clippy --workspace --all-targets`: no new warnings introduced by this change. (One pre-existing `items after a test module` warning in `apps/desktop/src-tauri/src/lib.rs:86` is unrelated to this change.)
- [x] 4.3 `pnpm tsc --noEmit` (or the equivalent type-check) inside `apps/desktop`: clean.
- [x] 4.4 `pnpm lint` inside `apps/desktop`: clean.
- [ ] 4.5 Manual smoke test: with an index containing `.rs`, `.md`, `.py` files, navigate to RAG Lab → focus glob input → confirm `*.md`, `*.py`, `*.rs` appear in alphabetical order. Type `s` → confirm filter narrows to `*.rs`. Click `*.rs` → confirm it appears as a chip and the input is cleared. (Pending end-user verification.)
- [ ] 4.6 Manual smoke test: with an empty index, navigate to RAG Lab → focus glob input → confirm no dropdown items appear and the input is still usable for manual entry. (Pending end-user verification.)
- [ ] 4.7 Manual smoke test: simulate a fetch error (e.g., temporarily rename `list_indexed_extensions` in the handler registration) → confirm the RAG Lab tab still loads, the input is functional, and no error toast surfaces. Restore the handler before committing. (Pending end-user verification.)
- [x] 4.8 `openspec validate rag-lab-glob-suggestions --strict`: clean.

## 5. Documentation

- [x] 5.1 Update PRD `docs/prd/002-the-command-palette.md` US-025 to remove the strike-through-able status note about the suggestion dropdown (or to cross-reference this change name), so the PRD reflects current coverage. (No edit required: line 116 already describes the implemented behavior as an acceptance criterion; PRD does not track per-bullet status. Avoiding edits to `002` also prevents a merge conflict with the in-progress `daemon-control-channel` change which also touches this file.)
- [x] 5.2 If `desktop-pin-and-glob-filter`'s task 5.6 retroactively needs annotation, leave a one-line note in this change's `proposal.md` Impact section (already done) — no edit to the archived change is required.
