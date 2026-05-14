## 1. Scaffolding & Tooling

- [ ] 1.1 Initialize Tauri v2 project in `apps/desktop/` with `src-tauri/` (Rust) and `src/` (SolidJS + TypeScript + Vite)
- [ ] 1.2 Configure `apps/desktop/package.json` with SolidJS, Vite, and TypeScript dependencies
- [ ] 1.3 Add `apps/desktop` to the root pnpm workspace so it can reference `packages/types`
- [ ] 1.4 Add `syncmind-core`, `syncmind-storage`, and `syncmind-rag-engine` as `path` dependencies in `apps/desktop/src-tauri/Cargo.toml`
- [ ] 1.5 Configure Tauri v2 capability file with minimal required permissions (no filesystem write exposed to frontend)
- [ ] 1.6 Verify `pnpm dev` launches the Tauri development window and `cargo check` passes in `src-tauri/`

## 2. Rust Core Integration & Tauri Commands

- [ ] 2.1 Implement core runtime initialization in `src-tauri/src/lib.rs`: load `~/.config/syncmind/config.toml`, start file watcher and indexing pipeline on app launch
- [ ] 2.2 Implement Tauri Command `search_knowledge(query, top_k, filter_file_type) -> Vec<SearchResult>` calling `syncmind-rag-engine` / `syncmind-storage` directly
- [ ] 2.3 Implement Tauri Command `get_config() -> Config` returning the current runtime configuration
- [ ] 2.4 Implement Tauri Command `update_config(ConfigPatch) -> Result<()>` persisting changes to `config.toml` and triggering core hot-reload
- [ ] 2.5 Implement Tauri Command `get_indexing_status() -> IndexingStatus` returning file count, chunk count, last update, and recent errors
- [ ] 2.6 Implement Tauri Command `trigger_reindex(file_path: Option<String>) -> Result<()>` queuing background re-index work
- [ ] 2.7 Set up TypeScript type generation (via `specta` + `tauri-specta` or manual types in `packages/types`) for all command inputs/outputs
- [ ] 2.8 Configure `tracing-subscriber` to log exclusively to stderr and `~/.local/share/syncmind/logs/desktop.log` (never stdout)

## 3. Desktop Shell — Hotkey, Window, Tray, Auto-Launch

- [ ] 3.1 Register global shortcut `Cmd+Shift+Space` using `tauri-plugin-global-shortcut` to toggle palette visibility
- [ ] 3.2 Configure the command palette window: borderless, fixed 860x540 px, centered on active screen, non-resizable
- [ ] 3.3 Implement window hide-on-blur behavior with 150 ms fade animation; keep process alive
- [ ] 3.4 Bind `Esc` key to hide the palette window
- [ ] 3.5 Focus and select-all search input text every time the window is shown
- [ ] 3.6 Add system tray icon and menu to macOS menu bar with items: Open Palette, Settings..., Indexing Status, Quit
- [ ] 3.7 Implement tray status indicator (healthy vs. error) based on last indexing result
- [ ] 3.8 Integrate `tauri-plugin-autostart` for "Launch at login" toggle in Settings
- [ ] 3.9 Ensure `Cmd+Q` and tray Quit cleanly shut down the core runtime (close SQLite, stop file watcher)

## 4. Command Palette — Search & Results

- [ ] 4.1 Build the search input component at the top of the palette with placeholder text and autofocus
- [ ] 4.2 Implement 300 ms debounce on search input before invoking `search_knowledge`
- [ ] 4.3 Show a loading skeleton or spinner while a search is in flight
- [ ] 4.4 Render search results list with: truncated file path (hover tooltip for full path), 120-char content preview, file-type icon, similarity score
- [ ] 4.5 Implement file-type icon mapping for common extensions (`.rs`, `.md`, `.py`, `.ts`, `.go`, `.pdf`) with a generic fallback
- [ ] 4.6 Add keyboard navigation: `↑` / `↓` to move selection, scroll selected item into view
- [ ] 4.7 Bind `Enter` to copy the selected chunk content to clipboard with "Copied!" feedback
- [ ] 4.8 Bind `Cmd+Enter` to open the source file in the system default application
- [ ] 4.9 Display empty state messages for empty query and zero-result queries

## 5. Preview Pane & Quick Actions

- [ ] 5.1 Implement left/right split layout: results list (40%) and preview pane (60%)
- [ ] 5.2 Show file path and line range (e.g., `src/main.rs:42-58`) in the preview pane header
- [ ] 5.3 Render the full chunk content in the preview pane with a monospace font and preserved indentation
- [ ] 5.4 Integrate `shiki` for syntax highlighting based on file extension; lazy-load language grammars
- [ ] 5.5 Ensure independent vertical scrolling for preview pane and results list
- [ ] 5.6 Add quick action bar in preview pane: Copy, Open File, Reveal in Finder
- [ ] 5.7 Implement Copy action (clipboard write) with visual feedback
- [ ] 5.8 Implement Open File action via Tauri Command using `open::that` in Rust backend
- [ ] 5.9 Implement Reveal in Finder action via Tauri Command using macOS `open -R` equivalent

## 6. RAG Lab Panel

- [ ] 6.1 Add a bottom tab or sidebar icon to switch between "Search" and "RAG Lab" views
- [ ] 6.2 Build `top_k` slider control (range 1–20, default 5) wired to the search command parameter
- [ ] 6.3 Build dynamic file-type filter multi-select populated from current index contents
- [ ] 6.4 Add Reset button that restores `top_k` to 5 and clears all file-type filters
- [ ] 6.5 Display debug telemetry after each search: query latency (ms), result count, active embedding model name and dimension
- [ ] 6.6 Add collapsible "Raw JSON" view showing the serialized `search_knowledge` response with syntax highlighting

## 7. Settings & Indexing Dashboard

- [ ] 7.1 Add Settings panel accessible via tab or sidebar icon
- [ ] 7.2 Build `ollama_url` input with URL validation and auto-save on change
- [ ] 7.3 Build `ollama_model` dropdown with presets (`bge-m3`, `bge-small`) and custom text entry
- [ ] 7.4 Display read-only `mcp_transport` value with explanatory note
- [ ] 7.5 List current `registered_files` with per-item delete button; reflect changes immediately in config
- [ ] 7.6 Implement "Add File" button invoking Tauri file dialog (`dialog::open`) with multi-select; append selections to config and trigger incremental indexing
- [ ] 7.7 Build indexing dashboard summary cards: total files, total chunks, last update timestamp
- [ ] 7.8 Build error log list showing the 10 most recent indexing errors (path, message, timestamp)
- [ ] 7.9 Implement "Rebuild All" button with confirmation dialog; trigger via `trigger_reindex(None)` and show in-progress state
- [ ] 7.10 Ensure settings changes propagate to the Rust core runtime via config hot-reload

## 8. Polish, QA, and Compliance

- [ ] 8.1 Verify combined idle memory (Tauri + core) stays below 150 MB for 5 minutes using Activity Monitor
- [ ] 8.2 Run `cargo clippy` in `apps/desktop/src-tauri/` and resolve all warnings
- [ ] 8.3 Run `pnpm lint` (or equivalent) in `apps/desktop/` and resolve all errors
- [ ] 8.4 Confirm no `println!` or stdout-tracing exists in the Tauri backend (only stderr / log file)
- [ ] 8.5 Test global hotkey responsiveness when focus is in external applications (VS Code, Terminal, Browser)
- [ ] 8.6 Test palette hide-on-blur behavior: click outside, switch Spaces, activate another app
- [ ] 8.7 Verify syntax highlighting works for Rust, TypeScript, Markdown, Python, and Go in the preview pane
- [ ] 8.8 Confirm clipboard copy, file open, and Finder reveal actions work from both keyboard shortcuts and mouse clicks
