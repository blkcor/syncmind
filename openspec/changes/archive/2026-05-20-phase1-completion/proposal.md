## Why

SyncMind Phase 1 (`001-headless-mcp-core`) was archived after delivering the Rust core engine, but a verification pass against `docs/prd/001-headless-mcp-core.md` revealed four acceptance-criteria gaps. Each gap touches a different layer of the core stack, but all share the same risk: Phase 1 was marked "code-complete" while shipping a daemon that cannot log to disk, cannot clean up indexed entries for deleted files, requires manual ONNX model placement, and offers no real-world end-to-end guide for new users. Closing these gaps is the prerequisite for opening Phase 2 (`The Spine`) and for the Desktop Command Palette (already shipping) to depend on a fully-spec-compliant core.

## What Changes

- **Embedding generation**: implement first-run auto-download of the ONNX fallback model and tokenizer from Hugging Face (configurable mirror) into `~/.local/share/syncmind/models/`, with atomic write and concurrent-launch protection.
- **Daemon observability**: replace the foreground-only `tracing_subscriber::fmt::init()` with a layered subscriber that always writes to a rolling file (`~/.local/share/syncmind/logs/syncmind.log.YYYY-MM-DD`) and additionally to stderr when `--foreground` is set; expose `log_level`, `log_to_file`, and `log_rotation` config fields.
- **File watching**: replace the watcher's `mpsc::Sender<Vec<PathBuf>>` channel with `mpsc::Sender<Vec<FileEvent>>` where `FileEvent` classifies events as `Upsert` or `Remove`, removing the existing filter that silently drops delete events.
- **Vector storage**: add `VectorStore::delete_file_by_path` that transactionally clears the `vec_chunks` virtual-table rows (sqlite-vec does not support FK cascade), the linked `chunks`, and the `files` row.
- **CLI**: route `FileEvent::Remove` through the indexing pipeline to the new storage API; extend `syncmind unregister <path>` to also delete the file's chunks from the index (currently leaves orphans).
- **Documentation**: create the missing root `README.md`, add a `docs/examples/claude_code_mcp.json` example for Claude Code CLI users, write a `docs/examples/quickstart.md`, enrich the existing `claude_desktop_config.json`, and update the PRD Open Questions to record the resolved decisions.
- **End-to-end verification**: add `scripts/e2e-phase1-realworld.sh` extending the existing `e2e-mcp-test.sh` with re-index-after-edit and delete-cleanup scenarios.

## Capabilities

### Modified Capabilities
- `embedding-generation`: Auto-download of ONNX fallback assets (was deferred as a non-goal in the original Phase 1 design; the PRD US-005 acceptance criterion requires it).
- `file-watching`: Channel emits semantic `FileEvent::Upsert | Remove` instead of opaque `PathBuf` batches; delete events are no longer filtered.
- `vector-storage`: New `delete_file_by_path` API plus integration with `FileEvent::Remove` and `syncmind unregister` for index cleanup.

### New Capabilities
- `daemon-observability`: Layered tracing subscriber with rolling-file appender, configurable level and rotation; safe under Stdio MCP transport (stdout reserved for JSON-RPC).

## Impact

- **Affected crates**: `core/rag-engine`, `core/syncmind-core`, `core/storage`, `core/file-watcher`, `core/syncmind-indexing`, `core/syncmind`.
- **Affected apps**: `apps/desktop/src-tauri` (watcher channel type change is a breaking API; same-PR migration required).
- **New dependencies**: `tracing-appender = "0.2"`, `fs2 = "0.4"` (lock file for download race protection).
- **Config schema additions** (backward-compatible defaults): `log_level`, `log_to_file`, `log_rotation`, `onnx_model_url`, `onnx_tokenizer_url`.
- **No breaking changes to MCP protocol** — `search_knowledge` schema and JSON-RPC handshake remain identical.
- **First-run network call**: ONNX auto-download contacts Hugging Face on initial daemon launch when Ollama is unavailable; documented in README and respects the privacy directive (single configurable endpoint, no telemetry).
