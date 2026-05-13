## 1. Foundation + Storage

- [x] 1.1 Update workspace `Cargo.toml` with new crates and dependencies
- [x] 1.2 Create `syncmind-core` crate (config, paths, shared types)
- [x] 1.3 Add config unit tests (roundtrip, default serialization)
- [x] 1.4 Create `syncmind-storage` crate (models, errors, VectorStore)
- [x] 1.5 Implement VectorStore schema initialization with sqlite-vec
- [x] 1.6 Implement VectorStore `upsert_file` with transactional replace
- [x] 1.7 Implement VectorStore `search` with vector similarity
- [x] 1.8 Add storage integration tests
- [x] 1.9 Create `syncmind` binary crate with CLI stubs
- [x] 1.10 Full workspace `cargo check`, `cargo test`, `cargo clippy` clean
- [x] 1.11 Binary smoke test successful

## 2. RAG Engine

- [x] 2.1 Define `Extractor` trait and `ExtractError`
- [x] 2.2 Implement `MarkdownExtractor` using `pulldown-cmark`
- [x] 2.3 Implement `CodeExtractor` for recognized extensions
- [x] 2.4 Implement `CompositeExtractor` dispatching by extension
- [x] 2.5 Define `Chunker` trait
- [x] 2.6 Implement `FallbackChunker` (fixed-size overlapping window)
- [x] 2.7 Implement `MarkdownChunker` (heading-aware)
- [x] 2.8 Implement `CodeChunker` (tree-sitter AST-based)
- [x] 2.9 Define `Embedder` trait and `EmbedError`
- [x] 2.10 Implement `OllamaEmbedder`
- [x] 2.11 Implement `OnnxEmbedder`
- [x] 2.12 Implement `AutoEmbedder` (Ollama probe + ONNX fallback)
- [x] 2.13 RAG engine unit tests passing
- [x] 2.14 Full workspace check, test, clippy clean

## 3. File Watch & CLI

- [ ] 3.1 Implement `notify`-based file watcher with debouncing
- [ ] 3.2 Implement `register` / `unregister` CLI commands
- [ ] 3.3 Implement config hot-reload on register/unregister
- [ ] 3.4 Implement `status` CLI command
- [ ] 3.5 Implement `search` CLI command
- [ ] 3.6 File watcher integration tests

## 4. MCP Server & E2E

- [ ] 4.1 Implement MCP Stdio transport
- [ ] 4.2 Implement MCP SSE transport
- [ ] 4.3 Implement `search_knowledge` tool handler
- [ ] 4.4 MCP integration tests
- [ ] 4.5 Claude Code E2E test script
- [ ] 4.6 Final workspace verification
