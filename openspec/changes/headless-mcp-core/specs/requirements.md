# Requirements: Headless MCP Core

## US-001: Workspace Scaffolding & Configuration

- Config loads from `~/.config/syncmind/config.toml` via XDG directories
- Config includes: `ollama_url`, `ollama_model`, `mcp_transport`, `bind_addr`, `registered_files`, `embedding_dim`, `chunk_size`, `chunk_overlap`
- Config roundtrip serialization with `toml`
- Path utilities for `data_dir`, `db_path`, `log_dir`, `model_cache_dir`

## US-002: File Registration & Change Listening

- Register/unregister file paths via CLI commands
- Watch registered files using `notify` with debouncing (500ms)
- Re-index files on modification
- Handle file removal and renaming

## US-003: Multi-Format Text Extraction

- `Extractor` trait with `extract(path) -> Result<String>`
- `MarkdownExtractor` using `pulldown-cmark`
- `CodeExtractor` for recognized code extensions
- `PdfExtractor` using `pdf-extract`
- `CompositeExtractor` dispatching by extension

## US-004: Semantic Chunking Engine

- `Chunker` trait with `chunk(text, path) -> Vec<Chunk>`
- `MarkdownChunker`: split by heading hierarchy, fallback window
- `CodeChunker`: `tree-sitter` AST-based, fallback window
- `FallbackChunker`: fixed-size overlapping window
- Configurable `chunk_size` and `chunk_overlap`

## US-005: Embedding Generation

- `Embedder` trait with `embed(texts) -> Result<Vec<Vec<f32>>>`
- `OllamaEmbedder`: HTTP POST to `/api/embed`
- `OnnxEmbedder`: `ort` inference with cached model
- `AutoEmbedder`: probes Ollama, falls back to ONNX

## US-006: Local Vector Storage

- `VectorStore` with SQLite + sqlite-vec
- Schema: `files`, `chunks`, `vec_chunks` virtual table
- `upsert_file`: transactional replace
- `search`: vector similarity with top-k
- `get_stats`: file and chunk counts

## US-007: MCP Protocol Server

- JSON-RPC 2.0 server
- `initialize` handshake
- `tools/list` exposing `search_knowledge`
- `tools/call` handler for `search_knowledge`
- Stdio transport (stdout reserved for JSON-RPC)
- SSE transport via `axum`

## US-008: CLI & Daemon Mode

- `clap` derive-based CLI
- Subcommands: `daemon [--foreground]`, `register <path>`, `unregister <path>`, `status`, `search <query> [--top-k N]`
- Daemon init flow: load config → init storage → full index → start watcher → start MCP server
- Foreground mode with `tracing-subscriber` pretty printer

## US-009: Claude Code Integration & E2E Testing

- MCP `search_knowledge` tool returns `{ chunk_id, file_path, start_line, end_line, content, score }`
- E2E test script for Claude Code MCP invocation
