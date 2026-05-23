## Context

PRD 002 (Command Palette) US-025 specifies that the RAG Lab glob chip input MUST provide "下拉建议：基于当前索引中实际出现的文件类型生成常用 pattern". The `desktop-pin-and-glob-filter` change delivered the chip input + `validate_file_filter` Tauri command but explicitly deferred this suggestion dropdown (task 5.6).

Current state:

- `RagLabTab.tsx` exposes a chip input that takes manual user input, runs it through `validate_file_filter`, and stores accepted patterns in `store.ragLab.fileTypeFilters` (SolidJS store). No suggestion source exists.
- `VectorStore` (in `core/storage/src/store.rs`) exposes `get_stats() -> (file_count, chunk_count)`, `delete_file_by_path`, pin / unpin methods, and search methods — but no API to enumerate file extensions currently in the index.
- The frontend has no list of "files I have indexed" surfaced anywhere (the existing `get_indexing_status` Tauri command returns counts + error log, not the path/extension set).

The suggestion source therefore has to come from a new storage API and a new Tauri command. The frontend work is straightforward chip-input enhancement.

## Goals / Non-Goals

**Goals:**

- Surface extensions currently present in the index as actionable `*.{ext}` suggestions in the RAG Lab chip input.
- Reuse the existing `validate_file_filter` + `addChip` flow so validation stays the single source of truth (no parallel validation path).
- Refresh suggestions whenever the user navigates to the RAG Lab tab, so they reflect the live index state.
- Fail closed-but-quiet: an error fetching suggestions degrades to "no suggestions", not a broken input.

**Non-Goals:**

- Keyboard navigation (`↑`/`↓` arrow keys) inside the suggestion dropdown — out of scope. Click + `Enter`-on-input is enough for the spec.
- Recursive / brace-expansion suggestions (e.g. `src/**/*.{ts,tsx}`) — out of scope. The dropdown only surfaces simple `*.{ext}` patterns.
- Real-time (push-based) refresh of the suggestion list when files are added / removed mid-session. The dropdown re-fetches on tab mount, which is the practical refresh trigger.
- Suggestions sourced from `registered_files` directly (i.e. unindexed paths). The source is exclusively the `files` table — only files that survived indexing contribute.

## Decisions

### D1. Suggestion source: storage scan, not config or stats

Three sources were considered:

1. `config.toml`'s `registered_files` — rejected because registered files may not have been indexed yet, and the list can include directories or globs that don't map cleanly to "extensions present".
2. Maintain an in-memory `HashSet<String>` updated by the indexing pipeline — rejected for scope creep: requires plumbing through `syncmind-indexing` and `syncmind-file-watcher` for a UI-only feature.
3. **Chosen:** Scan `files.path` in `VectorStore` on demand. The query is `SELECT DISTINCT path FROM files` (with extension extraction in Rust via `Path::extension`).

For a small index (<10k files), the scan is sub-millisecond and only runs on tab mount, so on-demand is fine. If this ever becomes a hot path, swapping to a denormalized column or in-memory cache is a localized change.

### D2. Extension extraction in Rust, not SQL

The naive SQL approach `SELECT DISTINCT lower(substr(path, instr(path, '.'))) FROM files` breaks on:

- Files without an extension (`README`, `Makefile`).
- Multi-dot paths where the leftmost dot is in a parent directory (`foo.bar/baz`).
- Path-component dotfiles like `.gitignore`.

Rust's `std::path::Path::extension` handles all three correctly. The cost is materializing one `String` per file row, which is fine at the scale we're targeting.

### D3. Return type: extensions only, no leading dot

The Tauri command returns `Vec<String>` of bare extensions (e.g. `["md", "rs", "ts"]`). The frontend prepends `*.` to form the suggestion label. Rationale: keeps the storage / command contract minimal and reusable for other consumers (e.g., a future "file types in your index" stat card on the Settings dashboard) that might want to display extensions without the glob wrapper.

### D4. Suggestion ordering: alphabetical ascending, case-insensitive dedup

Two ordering schemes were considered:

1. Frequency-based (most common extension first) — rejected because it requires `SELECT path, COUNT(*) GROUP BY` and stable display under re-mount, and frequency is unlikely to match what the user wants to click first.
2. **Chosen:** Alphabetical, lowercased, deduplicated. Stable, predictable, trivial to implement, easy for the user to scan.

### D5. Filter-while-typing: substring match, not prefix

When the user types `r` the dropdown shows `*.rs`, `*.tsx` (no — wait, `tsx` doesn't contain `r`). Right — substring match. When the user types `s` the dropdown shows `*.rs`, `*.ts`, `*.tsx`. Prefix match (only `*.rs` would match when typing `r`) would hide useful suggestions in long extensions. The match runs on the `*.{ext}` form so the `*` and `.` are part of the searchable surface, not the raw extension.

### D6. Exclude already-chipped patterns

Suggestions whose `*.{ext}` form is already in `store.ragLab.fileTypeFilters` are hidden from the dropdown. Without this, clicking a duplicate triggers `addChip` → "Pattern already added" error path, which is technically correct but creates a discoverability annoyance.

### D7. Click-to-add via `onMouseDown`, not `onClick`

The chip input has an `onBlur={() => addChip()}` to add the current draft when the user tabs/clicks away. If a suggestion uses `onClick`, the input's `blur` fires first, `addChip()` runs against whatever was typed (likely an incomplete pattern), and the suggestion click then runs against a now-cleared dropdown state. Using `onMouseDown` with `preventDefault()` fires before the blur, so the suggestion path runs cleanly. This is the canonical chip-input pattern.

### D8. Validation flows through the existing path

A clicked suggestion calls `setDraftPattern(suggestion)` then `addChip()`, which already goes through `validate_file_filter`. There's no shortcut. Two reasons:

1. If the storage method returns a corrupt extension somehow (e.g., a future bug writes `*` into the extension column), validation catches it.
2. Single validation path is easier to reason about than a "trusted" vs "untrusted" split.

### D9. Refresh policy: on tab mount

`onMount` in `RagLabTab.tsx` triggers a single `list_indexed_extensions` call. Alternatives considered:

- Refresh on every keystroke — wasteful, no benefit (the index isn't changing keystroke-to-keystroke).
- Subscribe to a Tauri event when indexing completes — out of scope; no such event exists yet and adding one belongs in the indexing capability, not here.
- Refresh on focus — slightly tighter than mount, but tab focus and mount coincide in the current shell (the tab is unmounted when navigated away from), so this would only matter if the shell architecture changes.

### D10. Silent failure on fetch error

If `invoke('list_indexed_extensions')` rejects (storage error, command not registered against an older daemon, etc.), the catch block sets `setSuggestions([])` and emits a `tracing::warn!` server-side. The chip input continues to function — users can still type patterns manually. No toast, no error banner: the dropdown's absence is itself the signal, and the feature is a "discoverability aid", not a critical path.

## Risks / Trade-offs

- **[Risk]** Large indices (>100k files) make `SELECT DISTINCT path` slow → **Mitigation**: at the scale targeted in PRD 002 (knowledge bases for individuals, not enterprises), the file count is far below this threshold. If it ever becomes a problem, swap to a `SELECT DISTINCT extension FROM files` with a denormalized column.
- **[Risk]** Suggestion list grows unwieldy (a user with 50+ extension types) → **Mitigation**: the substring filter while typing collapses this fast. No artificial cap is imposed — users with diverse indices are exactly the ones who benefit most from the dropdown.
- **[Risk]** Stale suggestions if the user re-indexes while the tab is open → **Mitigation**: acceptable — the user can switch tabs and back to refresh. Adding a push-based refresh requires an event-bus capability outside this change's scope.
- **[Risk]** Extension extraction conflates `tar.gz` and `gz` (both map to extension `gz`) → **Mitigation**: this matches the behavior of `Path::extension` and is consistent with the glob `*.gz`. Users who need finer-grained matching can type `*.tar.gz` manually; the dropdown isn't a substitute for the chip input.
- **[Trade-off]** No keyboard navigation in the dropdown (D5 / D-non-goal): the dropdown is mouse-driven only in v1. This is a deliberate scope cut — keyboard nav adds focus management complexity (the input has to share focus with list items) that doesn't materially advance the PRD goal. Can be added later without a spec change.
- **[Trade-off]** Suggestions exclude registered-but-unindexed files. A user who registered `foo.unusual` but indexing hasn't completed will not see `*.unusual` as a suggestion until the file is indexed. This is the correct behavior — filtering by an extension whose chunks don't yet exist would produce zero results, which is the same confusion the dropdown is supposed to prevent.
