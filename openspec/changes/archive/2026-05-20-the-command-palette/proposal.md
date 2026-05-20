## Why

SyncMind Phase 1 delivered a headless Rust core that indexes local files and exposes `search_knowledge` via MCP. However, non-technical users and even power users lack a fast, visual way to interact with their local knowledge base without opening a terminal or relying on an external MCP client. A native desktop command palette provides the missing interactive surface—global hotkey, millisecond search, instant preview—while keeping all data strictly on-device.

## What Changes

- Initialize `apps/desktop/` as a Tauri v2 + SolidJS application.
- Compile `syncmind-core` crates as library dependencies inside the Tauri backend, exposing capabilities through typed Tauri Commands.
- Implement a Raycast-style floating command palette window with system-wide global hotkey (`Cmd+Shift+Space` on macOS).
- Build semantic search UI with debounced input, keyboard navigation, file-type icons, and similarity scores.
- Add a preview pane with syntax highlighting and quick actions (copy, open file, reveal in Finder).
- Introduce a "RAG Lab" panel for tuning `top_k` and file-type filters with debug telemetry.
- Provide a visual settings editor and indexing status dashboard, eliminating the need to hand-edit `config.toml`.
- Add system tray integration with launch-at-login support.

## Capabilities

### New Capabilities
- `desktop-shell`: Tauri application scaffolding, Rust core library integration, global hotkey registration, floating window lifecycle, system tray menu, and auto-launch on login.
- `command-palette`: Search input, debounced semantic retrieval, results list with keyboard navigation, preview pane with syntax highlighting, and quick actions (copy, open, reveal).
- `rag-lab`: Parameter tuning panel (`top_k`, `filter_file_type`) with live debug telemetry (latency, result count, embedding model info).
- `settings-indexing`: Visual configuration editor for `config.toml`, registered file management, indexing status dashboard, and manual re-index triggers.

### Modified Capabilities
- *(None — this change introduces new UI capabilities without altering existing core RAG or MCP behavior.)*

## Impact

- **New code:** `apps/desktop/src/` (SolidJS frontend), `apps/desktop/src-tauri/` (Tauri backend/Rust wrappers).
- **Dependencies:** Tauri v2 crates (`tauri`, `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`), SolidJS, Vite, `shiki` or `prismjs` for syntax highlighting.
- **Core coupling:** `apps/desktop/src-tauri/Cargo.toml` will add `path` dependencies to `syncmind-core`, `syncmind-storage`, and `syncmind-rag-engine`.
- **Platform scope:** macOS only for this phase; Windows/Linux support explicitly deferred.
