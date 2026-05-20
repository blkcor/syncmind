## Context

SyncMind Phase 1 (`001-headless-mcp-core`) produced a Rust core engine capable of watching files, chunking text, generating embeddings via Ollama, and exposing `search_knowledge` through MCP Stdio/SSE transports. The engine runs as a headless daemon with all data stored locally in SQLite + sqlite-vec. Phase 2 adds the first native UI consumer: a macOS desktop command palette built with Tauri and SolidJS.

The desktop app is not a separate service communicating over HTTP or MCP. Instead, it compiles the core crates (`syncmind-core`, `syncmind-storage`, `syncmind-rag-engine`) as in-process libraries and exposes their capabilities through Tauri Commands. This eliminates serialization overhead and reduces the deployment artifact to a single `.app` bundle.

## Goals / Non-Goals

**Goals:**
- Deliver a macOS desktop application that provides instant semantic search over the user's local knowledge base.
- Ensure the combined Tauri + Rust core process stays under 150 MB at idle.
- Guarantee sub-500 ms end-to-end latency from keystroke to rendered search results (local SSD, < 1,000 chunks).
- Provide visual settings and indexing dashboards so users never need to edit `config.toml` by hand.
- Maintain absolute privacy: all IPC is in-process; no network calls for RAG retrieval.

**Non-Goals:**
- Cross-platform support (Windows/Linux shortcuts, window behaviors, and installers are deferred).
- Web Dashboard, Mobile App, or Spine (cloud sync) integration.
- Running an MCP server inside the desktop app (MCP remains the responsibility of the standalone CLI daemon).
- Plugin or theme extension system.

## Decisions

### 1. SolidJS instead of React for the frontend
**Rationale:** SolidJS offers fine-grained reactivity without a virtual DOM, producing smaller bundles and faster list updates—critical for a command palette that re-renders search results on every keystroke. React's larger runtime and VDOM diffing add unnecessary overhead for a single-window utility.
**Alternatives considered:** React (larger ecosystem, more hiring pool) and Svelte (minimal boilerplate, but less mature Tauri community examples). SolidJS strikes the best balance for this performance-sensitive UI.

### 2. Direct library integration ("Library-in-Process") instead of Sidecar or HTTP
**Rationale:** The Tauri backend links `syncmind-core` crates directly via Cargo `path` dependencies. This avoids managing a separate daemon process, simplifies code signing and sandboxing on macOS, and eliminates HTTP/MCP serialization latency.
**Alternatives considered:** Sidecar mode (Tauri bundles and manages the core binary as a child process) and HTTP client mode (desktop app talks to a localhost API). Both add operational complexity for a phase where the UI and core are tightly coupled.
**Trade-off:** If the core needs to outlive the UI in the future (e.g., background indexing while palette is closed), we will need to migrate to Sidecar mode. The current architecture localizes all core initialization logic in `src-tauri/src/lib.rs`, making such a migration straightforward.

### 3. Tauri v2 instead of v1
**Rationale:** Tauri v2 provides the Global Shortcut plugin and Auto Launch plugin as first-class citizens, both required for this feature. v2 also improves mobile support (future-proofing) and simplifies permission capabilities.
**Alternatives considered:** v1 with custom global-hotkey crates (`rdev`, `global-hotkey`). These work but require more manual platform-specific code.

### 4. Shiki for syntax highlighting in the preview pane
**Rationale:** Shiki uses the same TextMate grammar engine as VS Code, providing accurate highlighting for a wide range of languages with minimal configuration. It supports on-demand language loading, keeping the initial bundle small.
**Alternatives considered:** PrismJS (lighter but less accurate for newer languages like Rust or Go) and highlight.js (good coverage, but larger all-in-one bundle). Shiki's accuracy outweighs its slightly larger WASM dependency for a developer-focused tool.

### 5. macOS-first launch
**Rationale:** macOS is the primary development platform for the initial target audience (engineers using Claude Code/Cursor). Menu bar tray integration and global shortcut APIs are mature and well-documented on macOS.
**Migration path:** Window management and tray logic will be abstracted behind a thin platform module. When expanding to Windows/Linux, only this module needs new implementations.

## Risks / Trade-offs

- **[Risk] Memory budget exceeded:** Tauri WebView (~60–80 MB) + Rust core (~50–70 MB) could push combined idle memory close to or above 150 MB if not carefully managed.
  → **Mitigation:** Lazy-load the RAG Lab panel and Settings panel (code-split via SolidJS dynamic imports). Unload large preview buffers when the palette hides. Monitor with `activity monitor` during QA.

- **[Risk] SQLite lock contention:** If the user also runs the standalone `syncmind` CLI daemon, both processes may attempt concurrent writes to `~/.local/share/syncmind/syncmind.db`.
  → **Mitigation:** Ensure all SQLite connections across the codebase use WAL (Write-Ahead Logging) mode and a `busy_timeout` of at least 5 seconds. Document that running the desktop app and the CLI daemon simultaneously is supported but not recommended for write-heavy operations.

- **[Risk] stdout pollution breaks Tauri frontend loading:** Tauri uses stdout internally to communicate with its WebView loader. If Rust `tracing` logs or print statements leak to stdout, the frontend may fail to load or behave erratically.
  → **Mitigation:** Configure `tracing-subscriber` at application startup to write exclusively to stderr and the rotating log file (`~/.local/share/syncmind/logs/desktop.log`). Add a CI check (lint rule or test) that fails on `println!` usage in `apps/desktop/src-tauri/`.

- **[Risk] Bundle size bloat from ONNX fallback model:** If the desktop app bundles the core crates, it may also pull in the ONNX runtime and fallback embedding model, inflating the `.app` bundle by hundreds of megabytes.
  → **Mitigation:** Gate the ONNX fallback behind a Cargo feature flag (`onnx-fallback`) that is disabled for the desktop build. The desktop app assumes Ollama is available locally (reasonable for the target audience) and gracefully degrades search functionality if Ollama is unreachable.
