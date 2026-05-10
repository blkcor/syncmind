# Proposal: Headless MCP Core (Phase 1)

## Why

SyncMind needs a privacy-first, fully offline local context engine that runs as a headless daemon. Phase 1 establishes the Rust core that watches registered files, extracts text, chunks it semantically, generates embeddings, and exposes a `search_knowledge` tool via the Model Context Protocol (MCP).

## Scope

Implement all 9 user stories from `docs/prd/001-headless-mcp-core.md`:

- **US-001:** Workspace scaffolding & configuration system
- **US-002:** File registration & change listening
- **US-003:** Multi-format text extraction (Markdown, code, PDF)
- **US-004:** Semantic chunking engine
- **US-005:** Embedding generation with Ollama → ONNX fallback
- **US-006:** Local vector storage (SQLite + sqlite-vec)
- **US-007:** MCP protocol server (Stdio + SSE)
- **US-008:** CLI & daemon mode
- **US-009:** Claude Code integration & E2E testing

## Milestones

1. **Foundation + Storage (US-001 + US-006):** Config system, SQLite schema, `VectorStore` API
2. **RAG Engine (US-003 + US-004 + US-005):** Extractor, Chunker, Embedder traits + implementations
3. **File Watch & CLI (US-002 + US-008):** `notify`-based watcher, CLI commands
4. **MCP Server & E2E (US-007 + US-009):** JSON-RPC transports, `search_knowledge`, integration tests

## Out of Scope

- UI (Web, Desktop, Mobile)
- Directory-level recursive watching
- Image OCR
- Cloud sync / Go gateway
- Graph RAG or multi-hop reasoning
- Multi-user / permissions
