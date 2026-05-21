# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SyncMind is a privacy-first, fully offline proactive local context engine. It is NOT a note-taking app or chat UI. It runs as a local daemon that indexes the user's fragmented digital assets (code snippets, drafts, reading notes) into a semantic vector store, then injects relevant context into AI assistants via the Model Context Protocol (MCP).

## Architecture

SyncMind adopts a **"Headless-First"** architecture where core computation and UI are completely decoupled.

```
┌─────────────────────────────────────────────────────────────┐
│                    The Brain (Rust)                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  Watcher │→ │ Extractor│→ │ Chunker  │→ │ Embedder │   │
│  └──────────┘  └──────────┘  └──────────┘  └────┬─────┘   │
│                                                  ↓         │
│  ┌─────────────────────────────────────────────────────┐  │
│  │        Vector Store (SQLite + sqlite-vec)           │  │
│  └─────────────────────────────────────────────────────┘  │
│                           ↓                                │
│  ┌─────────────────────────────────────────────────────┐  │
│  │         MCP Server (Stdio / SSE transport)          │  │
│  └─────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ↓                     ↓                     ↓
   ┌─────────┐         ┌──────────┐          ┌──────────┐
   │Desktop  │         │ Web Dash │          │ Mobile   │
   │(Tauri)  │         │(Next.js) │          │(Expo)    │
   └─────────┘         └──────────┘          └──────────┘
```

### The Brain (`core/`)

- **Language:** Rust (Cargo workspace)
- **Responsibilities:**
  - Data pipeline: watch local file tree → extract text → semantic chunking → generate embeddings via local LLM (Ollama) or ONNX fallback.
  - Storage: relational metadata in SQLite, vectors in sqlite-vec.
  - MCP Provider: expose `search_knowledge` tool to external consumers (Claude Code, Cursor, etc.).
- **Constraints:** Idle memory < 100MB. No external network calls except local Ollama HTTP. All data stays on the user's physical machine.

### The Spine (`services/`)

- **Language:** Go (Go-Zero / Hertz)
- **Responsibilities:** Cross-device sync gateway, E2EE data relay, mobile media ingestion.
- **Status:** Phase 3 — not yet implemented.

### The Nerve Endings (`apps/`)

All UI clients are consumers of the Brain. No UI client is required for the core to function.

- **Desktop (`apps/desktop/`):** Tauri + SolidJS/React. Global command palette (Raycast-style), IPC to Rust core.
- **Web (`apps/web/`):** Next.js/Vue. Admin dashboard, 3D knowledge graph, RAG lab.
- **Mobile (`apps/mobile/`):** React Native / Expo. Minimal idea capture, voice/scan input.

## Monorepo Structure

SyncMind is a **polyglot monorepo** managed with two workspace systems:

- **Rust:** `core/Cargo.toml` defines a Cargo workspace with four member crates.
- **TypeScript / Frontend:** Root `package.json` defines a pnpm workspace covering `apps/*` and `packages/*`.

There is no single root build command that spans both languages. Build and test each stack independently.

## Repository Structure

| Path                      | Purpose                                                  |
| ------------------------- | -------------------------------------------------------- |
| `core/`                   | Rust headless engine (Cargo workspace)                   |
| `core/mcp-server/`        | MCP protocol server (Stdio + SSE transports)             |
| `core/file-watcher/`      | File change listener and re-indexing trigger             |
| `core/rag-engine/`        | Text extraction, semantic chunking, embedding generation |
| `core/storage/`           | SQLite + sqlite-vec persistence layer                    |
| `services/`               | Go sync gateway (future)                                 |
| `services/sync-gateway/`  | Go microservice for cross-device sync                    |
| `apps/`                   | UI clients: desktop, web, mobile                         |
| `apps/desktop/`           | Tauri desktop app                                        |
| `apps/web/`               | Next.js / Nuxt knowledge graph dashboard                 |
| `apps/mobile/`            | Mobile idea capture app                                  |
| `packages/`               | Shared frontend packages                                 |
| `packages/types/`         | Global TypeScript type definitions                       |
| `packages/ui-kit/`        | Cross-platform reusable component library                |
| `packages/eslint-config/` | Shared ESLint configurations                             |
| `packages/ts-config/`     | Shared TypeScript configurations                         |
| `docs/vision.md`          | Architecture blueprint and engineering directives        |
| `docs/prd`                | The Prd with the SFC Standard name convertion            |

## Engineering Directives

These are hard rules. Any code change must respect them.

1. **Privacy is Absolute** — All raw text, code, and generated vectors must remain 100% on the user's local machine. No core RAG logic may depend on public cloud APIs.
2. **Docs as Code & Spec-Driven** — API docs and requirements must live in the repo. Every feature must first be defined in `docs/` with input/output schema before implementation.
3. **Frugal Resource Usage** — The core daemon is a background resident process. Idle memory must stay under 100MB. Avoid bloated runtimes (no Electron in core). This is why the core is Rust.
4. **Decouple Data from UI** — Always implement the API or MCP interface first. If the system cannot run without a UI panel, the architecture is wrong.

## Development Workflow

### Spec-Driven Development

1. Write or update the PRD in `docs/<NNN>-<feature-name>.md`.
2. Define acceptance criteria, input/output schemas, and module boundaries.
3. Implement against the spec.
4. Update the spec if the implementation diverges.

### Phase 1 Focus: Headless MCP Core

The current milestone is building the Rust core workspace in `core/`.

Planned crates:

- `syncmind-file-watcher` — File change listener (`notify` crate)
- `syncmind-rag-engine` — Text extraction trait + Markdown (`pulldown-cmark`), code, PDF implementations; semantic chunking; `Embedder` trait (Ollama primary, ONNX fallback)
- `syncmind-storage` — SQLite + sqlite-vec persistence layer
- `syncmind-mcp-server` — MCP server implementation (Stdio + SSE transports)

A top-level `syncmind` binary crate (or `mcp-server` binary) will provide the CLI via `clap`.

### Common Commands

**Rust (core):**

```bash
# Check all crates
cd core && cargo check

# Run tests
cargo test

# Lint
cargo clippy
```

**Frontend (apps / packages):**

```bash
# Install dependencies
pnpm install

# Build all packages
pnpm build

# Run all tests
pnpm test

# Lint all packages
pnpm lint
```

### Key Configuration Fields (`config.toml`)

- `ollama_url`, `ollama_model`
- `mcp_transport`: `"stdio"` | `"sse"`
- `bind_addr`: used when `mcp_transport = "sse"`
- `registered_files`: list of explicit file paths to index
- `embedding_dim`: must match the active model (1024 for `bge-m3`, 384 for `bge-small`)

### Data & Log Locations

SyncMind resolves data and config directories using the `dirs` crate (see `core/syncmind-core/src/paths.rs` and `core/syncmind-core/src/config.rs`). Never hardcode Linux-specific paths like `~/.local/share/syncmind/` in documentation or comments.

| Variable | Resolution | Example (Linux) |
|----------|------------|-----------------|
| `<data-dir>` | `dirs::data_local_dir()` + `/syncmind` | `~/.local/share/syncmind` |
| `<config-dir>` | `dirs::config_dir()` + `/syncmind` | `~/.config/syncmind` |

Platform-specific defaults:
- **Linux**: `~/.local/share/syncmind/` (data), `~/.config/syncmind/` (config)
- **macOS**: `~/Library/Application Support/syncmind/` (both data and config)
- **Windows**: `%LOCALAPPDATA%\syncmind\` (data), `%APPDATA%\syncmind\` (config)

Both directories can be overridden via environment variables: `SYNCMIND_DATA_DIR` and `SYNCMIND_CONFIG_DIR`.

## MCP Integration Notes

When implementing MCP-related code:

- Stdio mode: stdout is reserved for JSON-RPC only. All logs must go to stderr or the log file.
- The exposed tool is `search_knowledge` with params: `query` (string), `top_k` (int, default 5), `filter_file_type` (string[] optional).
- Return format per result: `{ chunk_id, file_path, start_line, end_line, content, score }`.

## Git Workflow

### Commit Convention

Follow [Conventional Commits](https://www.conventionalcommits.org/) with a monorepo-aware scope.

**Format:** `type(scope): description`

**Types:** `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `perf`, `ci`

**Scopes** — use the workspace path or affected crate:

- `core` — Rust workspace-wide changes
- `core:mcp-server`, `core:rag-engine`, `core:storage`, `core:file-watcher` — specific crate
- `apps:web`, `apps:desktop`, `apps:mobile` — frontend apps
- `packages:types`, `packages:ui-kit` — shared packages
- `services:sync-gateway` — Go service
- `docs` — documentation and PRDs

**Examples:**

```
feat(core:rag-engine): add markdown text extractor
fix(apps:web): handle empty search results in graph view
chore(core): update workspace dependencies
docs: clarify embedding dim configuration in 001-headless-mcp-core.md
```

**Rules:**

- Write descriptions in imperative mood (`add`, not `added` or `adds`).
- Keep the first line under 72 characters.
- Use the commit body for breaking change notes and rationale.
- Breaking changes must append `!` to the type: `feat(core)!: change embedding schema`.

### Branch Strategy

Use short-lived feature branches off `main`.

**Branch naming:** `type/short-description` or `type/scope/short-description`

Examples:

- `feat/core/mcp-stdio-transport`
- `fix/apps-web/graph-render-loop`
- `docs/update-prd-chunking`

### Pull Request Workflow

1. **Open a PR only when:** the feature is complete, tests pass, and `cargo clippy` (or frontend lint) is clean.
2. **PR title:** mirrors the commit convention — `type(scope): description`.
3. **PR description:** link to the relevant PRD section or issue. Summarize what changed and why.
4. **Required checks before merge:**
   - CI passes (tests, lint, typecheck).
   - PRD updated if the implementation diverged from the spec.
5. **Merge method:** squash and merge for feature branches. Rebase and merge for stacked PRs or multi-commit refactor branches where individual commits are meaningful.

## Agent Workflow

These rules guide how Agent(Claude Code) should operate inside this repository.

### 1. Spec-First Planning (OpenSpec Standard)

For any multi-step feature or refactor, use the OpenSpec workflow **before writing implementation code**.

**Location:** `openspec/changes/<change-name>/` (managed by the `openspec` CLI).

**Artifacts per change:**

- `proposal.md` — Why we're doing this, what's changing
- `design.md` — Technical approach
- `tasks.md` — Implementation checklist
- `.openspec.yaml` — Change metadata (managed by CLI)

**Process:**

1. **Propose:** Run `/opsx:propose <change-name>` to scaffold the change and generate artifacts.
2. **Get user approval** on the generated proposal and design.
3. **Apply:** Run `/opsx:apply <change-name>` to implement against `tasks.md`.
4. **Archive:** After merge, run `/opsx:archive <change-name>` to move the change to `openspec/changes/archive/YYYY-MM-DD-<change-name>/`.

**Note:** The legacy `docs/spec/` directory contains pre-OpenSpec specs (e.g. `001-headless-mcp-core`). New work must use the OpenSpec workflow in `openspec/changes/`.

### 2. Read the PRD Before Coding

Always read the relevant PRD in `docs/` before implementing. If the implementation must diverge from the spec, flag it and update the PRD.

### 3. Test-Driven Development

Use `superpowers:test-driven-development` when implementing features or bugfixes. Write tests before implementation code.

### 4. Systematic Debugging

When a test fails or behavior is unexpected, use `superpowers:systematic-debugging` before proposing fixes.

### 5. Verify Before Completion

Use `superpowers:verification-before-completion` before claiming any task is done. Actually run `cargo test`, `cargo clippy`, or `pnpm test` / `pnpm lint` and confirm they pass.

### 6. Proactive Code Review

Use `superpowers:requesting-code-review` before finishing a task or opening a PR. This is especially important for MCP protocol changes where stdout discipline and JSON-RPC correctness are critical.
