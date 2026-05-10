# Implementation Tasks

## Milestone 1: Foundation + Storage
- [x] Workspace Cargo.toml updated with new crates and dependencies
- [x] `syncmind-core` crate created (config, paths, shared types)
- [x] Config unit tests passing
- [x] `syncmind-storage` crate created (models, errors, VectorStore)
- [x] VectorStore schema initialization with sqlite-vec
- [x] VectorStore upsert_file with transactional replace
- [x] VectorStore search with vector similarity
- [x] Storage integration tests passing
- [x] `syncmind` binary created with CLI stubs
- [x] Full workspace `cargo check`, `cargo test`, `cargo clippy` clean
- [x] Binary smoke test successful

## Milestone 2: RAG Engine
- [x] Extractor trait + Markdown/Code/PDF implementations
- [x] Chunker trait + Markdown/Code/Fallback implementations
- [x] Embedder trait + Ollama + ONNX fallback
- [x] Unit tests for all RAG components

## Milestone 3: File Watch & CLI
- [ ] notify-based file watcher with debouncing
- [ ] `register` / `unregister` commands with config hot-reload
- [ ] `status` and `search` CLI commands

## Milestone 4: MCP Server & E2E
- [ ] MCP Stdio transport
- [ ] MCP SSE transport
- [ ] `search_knowledge` tool implementation
- [ ] E2E tests and Claude Code integration examples
