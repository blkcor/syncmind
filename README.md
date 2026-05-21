# SyncMind

> Privacy-first, fully-offline local context engine for AI assistants.

SyncMind is a headless local daemon that indexes your fragmented digital assets — code snippets, drafts, reading notes — into a semantic vector store, then injects relevant context into AI assistants (Claude Code, Cursor, etc.) via the **Model Context Protocol (MCP)**.

All raw text, embeddings, and metadata stay on your machine. The only network call the core makes is to your local Ollama instance (and a one-time download of the ONNX fallback model from Hugging Face).

## Status

| Phase | Component | State |
|-------|-----------|-------|
| 1 | Headless MCP Core (Rust) | ✅ Complete |
| 2 | Desktop Command Palette (Tauri) | ✅ Complete |
| 3 | The Spine — cross-device sync (Go) | ⏸️ Planned |
| 4 | Mobile capture (Expo) | ⏸️ Planned |

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                      The Brain (Rust)                          │
│  Watcher → Extractor → Chunker → Embedder → Vector Store       │
│                            │                                   │
│                     MCP Server (Stdio / SSE)                   │
└──────────────────────────────┬─────────────────────────────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        ↓                      ↓                      ↓
   ┌──────────┐         ┌────────────┐         ┌──────────┐
   │ Desktop  │         │ Claude Code│         │  Cursor  │
   │ (Tauri)  │         │   (MCP)    │         │  (MCP)   │
   └──────────┘         └────────────┘         └──────────┘
```

The core is decoupled from any UI. The Desktop app is one consumer; Claude Code via MCP is another. Both read from the same SQLite + sqlite-vec store.

See [`docs/vision.md`](docs/vision.md) for the full architecture blueprint.

## Quickstart (Phase 1)

For the full walkthrough see [`docs/examples/quickstart.md`](docs/examples/quickstart.md). 30-second version:

```bash
# Build the daemon
cd core && cargo build --release --bin syncmind
sudo cp target/release/syncmind /usr/local/bin/

# Register the files you want indexed
syncmind register /absolute/path/to/notes.md
syncmind register /absolute/path/to/project/src/main.rs

# Run the daemon (foreground for first-run diagnostics)
syncmind daemon --foreground
```

On first run with no Ollama instance, the ONNX fallback model (`bge-small-en-v1.5`, ~130 MB) is auto-downloaded from Hugging Face to `<data-dir>/syncmind/models/`. Subsequent runs reuse the cached files.

## Connecting Claude Code

Add SyncMind as an MCP server in Claude Code. See [`docs/examples/claude_code_mcp.json`](docs/examples/claude_code_mcp.json) for the exact JSON payload, or run:

```bash
claude mcp add-json syncmind '{"command":"syncmind","args":["daemon","--foreground"]}'
```

Then ask Claude Code something like *"based on my notes, what did I write about embedding dimensions?"* — it will invoke the `search_knowledge` tool against your local index.

## File Locations

Paths below use generic placeholders (`<config-dir>` and `<data-dir>`) because SyncMind resolves them using your OS standard directories. Run `syncmind status` to see the exact paths on your machine.

| Path | Purpose |
|------|---------|
| `<config-dir>/syncmind/config.toml` | User configuration |
| `<data-dir>/syncmind/syncmind.db` | SQLite + sqlite-vec database |
| `<data-dir>/syncmind/logs/syncmind.log.<date>` | Rolling daemon logs |
| `<data-dir>/syncmind/models/` | Cached ONNX model + tokenizer |

**Platform Paths**

| Platform | `<config-dir>` | `<data-dir>` |
|----------|----------------|--------------|
| Linux | `~/.config` | `~/.local/share` |
| macOS | `~/Library/Application Support` | `~/Library/Application Support` |
| Windows | `%APPDATA%` | `%LOCALAPPDATA%` |

## Configuration Highlights

| Field | Default | Description |
|-------|---------|-------------|
| `ollama_url` | `http://localhost:11434` | Local Ollama base URL |
| `ollama_model` | `bge-m3` | Embedding model used when Ollama is available |
| `embedding_dim` | `1024` | Must match the model (`bge-m3` = 1024, `bge-small` = 384) |
| `mcp_transport` | `stdio` | `stdio` for Claude Code; `sse` for HTTP clients |
| `log_level` | `info` | `trace` / `debug` / `info` / `warn` / `error` |
| `log_to_file` | `true` | Whether to write to `<data-dir>/syncmind/logs/` |
| `log_rotation` | `daily` | `daily` / `hourly` / `never` |
| `onnx_model_url` | *(default HF URL)* | Override to use an in-country mirror |

## Repository Layout

| Path | Stack | Status |
|------|-------|--------|
| `core/` | Rust workspace (7 crates) | ✅ Phase 1 |
| `apps/desktop/` | Tauri + SolidJS | ✅ Phase 2 |
| `apps/web/` | Next.js / Vue | ⏸️ Future |
| `apps/mobile/` | Expo / React Native | ⏸️ Future |
| `services/sync-gateway/` | Go (Hertz) | ⏸️ Phase 3 |
| `packages/` | Shared TS configs and types | ✅ |
| `docs/` | PRDs, architecture, examples | ✅ |
| `openspec/` | Spec-driven change tracking | ✅ |

## Engineering Directives

The full directives live in [`CLAUDE.md`](CLAUDE.md). Summary:

1. **Privacy is absolute** — no raw text or vectors leave the user's machine.
2. **Spec-driven** — features start with a PRD in `docs/prd/` and an OpenSpec change in `openspec/changes/`.
3. **Frugal** — idle memory < 100 MB. No Electron in core.
4. **Decouple data from UI** — MCP and API first; UIs are consumers.

## License

Dual-licensed under MIT or Apache 2.0.
