# AGENTS.md

This file provides repository-level instructions for Codex and other coding agents working in this repo. It is the Codex-facing counterpart to `CLAUDE.md`; keep both files aligned when project rules change.

## Command And Tooling Preferences

- Prefer `rg` over `grep`, `fd` over `find`, `eza` over `ls`, `sd` over `sed`, `just` over `make`, `uv` over `pip`, `uv run` over `python3`, and `pnpm` over `npm`.
- Useful tools already available: `sg`, `duckdb`, `mlr`, `jc`, `gron`, `gh`, `sqlite3`, `hyperfine`, `gitleaks`.
- For Python work, use `uv`, `ruff`, and `basedpyright`.
- For background Python tasks, use unbuffered output: `PYTHONUNBUFFERED=1` or `python -u`.

## Project Overview

SyncMind is a privacy-first, fully offline proactive local context engine. It is not a note-taking app or chat UI. The core product is a local daemon that indexes fragmented personal knowledge into a semantic vector store, then injects relevant context into AI assistants through MCP.

### Architecture

SyncMind is headless-first. Core computation and UI are intentionally decoupled.

- `core/`: Rust "brain" for file watching, extraction, chunking, embeddings, storage, and MCP serving.
- `services/sync-gateway/`: Go "spine" for cross-device sync, E2EE blind relay, media ingestion, and realtime notifications.
- `apps/desktop/`, `apps/web/`, `apps/mobile/`: UI clients that consume the core. No UI client is required for the core to function.

### Monorepo Notes

- Rust uses the workspace at `core/Cargo.toml`.
- Frontend packages use the root `pnpm` workspace (`apps/*`, `packages/*`).
- There is no single root build command for every language stack. Build and test each stack independently.

## Hard Engineering Directives

1. Privacy is absolute. Raw text, code, and vectors must stay on the user's machine. Core RAG logic must not depend on public cloud APIs.
2. Specs live in-repo. Requirements, interfaces, and acceptance criteria belong in `docs/` or `openspec/` before implementation.
3. Resource usage matters. The core daemon is a resident background process and should remain frugal, including the idle-memory target of under 100MB.
4. Data and UI stay decoupled. Implement APIs, storage, and MCP interfaces first. If a feature only works through a UI surface, the architecture is wrong.

## Repository Map

- `core/`: Rust headless engine
- `core/mcp-server/`: MCP server with stdio and SSE transports
- `core/file-watcher/`: file watching and re-index triggers
- `core/rag-engine/`: extraction, chunking, and embedding pipeline
- `core/storage/`: SQLite plus `sqlite-vec` persistence
- `services/sync-gateway/`: Go sync service
- `apps/desktop/`: Tauri desktop app
- `apps/web/`: web dashboard and knowledge graph work
- `apps/mobile/`: mobile capture app
- `packages/`: shared frontend packages
- `docs/vision.md`: architecture blueprint and engineering direction
- `docs/prd/`: product requirements docs
- `openspec/changes/`: active spec-driven changes

## Development Workflow

### Spec-Driven Work

- Read the relevant PRD or spec before coding.
- For multi-step features and refactors, use the OpenSpec workflow in `openspec/changes/<change-name>/`.
- Each OpenSpec change should carry `proposal.md`, `design.md`, `tasks.md`, and `.openspec.yaml`.
- Get user approval on proposal and design before implementation.
- If implementation diverges from the spec, update the spec instead of leaving drift behind.
- Archive completed changes into `openspec/changes/archive/`.

### Implementation Discipline

- Prefer test-first work for new features and bugfixes when practical.
- When behavior is unexpected or tests fail, debug from evidence before patching.
- Smoke-test a narrow slice before launching broad or expensive work.
- Do not make factual claims about repo behavior without checking code, docs, tests, or command output.

### Completion Discipline

- Before claiming a task is done, run the relevant verification commands and report what passed or failed.
- Do not silently skip lint, typecheck, or tests if they are relevant to the touched area.
- If verification cannot run, say so explicitly and explain why.

## Stack-Specific Commands

### Rust Core

Run from the repo root:

```bash
cd core && cargo check
cd core && cargo test
cd core && cargo clippy
```

### Frontend

Run from the repo root:

```bash
pnpm build
pnpm lint
pnpm test
pnpm dev:desktop
```

### Go Sync Service

Run from the repo root:

```bash
cd services/sync-gateway && go test ./...
```

## MCP And Runtime Constraints

- In stdio mode, stdout is reserved for JSON-RPC only. Logs must go to stderr or log files.
- The exposed search tool is `search_knowledge`.
- Expected params: `query` (string), `top_k` (int, default `5`), `filter_file_type` (string array, optional).
- Expected result shape: `{ chunk_id, file_path, start_line, end_line, content, score }`.

## Config And Path Rules

- Key config fields include `ollama_url`, `ollama_model`, `mcp_transport`, `bind_addr`, `registered_files`, and `embedding_dim`.
- `embedding_dim` must match the active embedding model.
- Never hardcode Linux-specific paths in code or docs.
- Data and config directories are resolved via the `dirs` crate. Respect `SYNCMIND_DATA_DIR` and `SYNCMIND_CONFIG_DIR` overrides.

## Git Conventions

- Use Conventional Commits: `type(scope): description`.
- Prefer monorepo-aware scopes such as `core`, `core:mcp-server`, `apps:desktop`, `apps:web`, `apps:mobile`, `packages:types`, `packages:ui-kit`, `services:sync-gateway`, and `docs`.
- Keep the subject imperative and under 72 characters.
- Use short-lived feature branches from `main`.

## Agent Operating Rules

- Respect a dirty worktree. Never revert or overwrite user changes you did not make unless asked.
- Avoid destructive git operations like `reset --hard` or checkout-based reverts unless the user explicitly requests them.
- Keep edits focused. Do not introduce unrelated refactors while solving a scoped task.
- Prefer existing patterns over inventing new abstractions, folders, or taxonomies without a strong reason.
- Report user-relevant state changes plainly: what changed, what was verified, and what still needs a decision.
