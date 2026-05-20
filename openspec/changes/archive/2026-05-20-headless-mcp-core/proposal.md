## Why

SyncMind needs a privacy-first, fully offline local context engine that runs as a headless daemon. This change establishes the Rust core that watches registered files, extracts text, chunks it semantically, generates embeddings, and exposes a `search_knowledge` tool via the Model Context Protocol (MCP).

## What Changes

- Add Rust Cargo workspace with 6 crates: `syncmind` (binary), `syncmind-core`, `syncmind-storage`, `syncmind-rag-engine`, `syncmind-file-watcher`, `syncmind-mcp-server`
- Implement configuration system with XDG-compliant paths
- Implement SQLite + sqlite-vec vector storage with upsert and similarity search
- Implement text extraction pipeline (Markdown, code, PDF)
- Implement semantic chunking (heading-aware, AST-based, fallback)
- Implement embedding generation with Ollama primary and ONNX fallback
- Implement file watcher with debounced re-indexing
- Implement MCP server with Stdio and SSE transports
- Add CLI commands: `daemon`, `register`, `unregister`, `status`, `search`

## Capabilities

### New Capabilities
- `config-system`: XDG-compliant configuration loading and persistence
- `vector-storage`: SQLite-backed vector store with sqlite-vec for similarity search
- `text-extraction`: Pluggable extractors for Markdown, code, and PDF files
- `semantic-chunking`: Format-aware chunking with configurable size and overlap
- `embedding-generation`: Local embedding via Ollama with ONNX fallback
- `file-watching`: Debounced file change detection with automatic re-indexing
- `mcp-server`: Model Context Protocol server exposing `search_knowledge`

### Modified Capabilities
- None (this is a greenfield Phase 1 implementation)

## Impact

- New Rust workspace at `core/`
- New dependencies: `rusqlite`, `sqlite-vec`, `pulldown-cmark`, `pdf-extract`, `tree-sitter`, `reqwest`, `ort`, `notify`, `axum`
- No breaking changes (new codebase)
