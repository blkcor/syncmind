# Design: Headless MCP Core (Phase 1)

## Architecture

SyncMind Phase 1 is a polyglot monorepo with a Rust Cargo workspace at `core/`. The workspace contains 6 crates mapped to PRD domains:

| Crate | Type | PRD Module | Responsibilities |
|-------|------|------------|------------------|
| `syncmind` | Binary | US-008 CLI | `clap` CLI, daemon orchestration, subcommands |
| `syncmind-core` | Library | US-001 Config | Shared types, `Config` struct, config loading, paths |
| `syncmind-storage` | Library | US-006 Storage | SQLite + sqlite-vec schema, `VectorStore` |
| `syncmind-rag-engine` | Library | US-003–005 | `Extractor`, `Chunker`, `Embedder` traits + impls |
| `syncmind-file-watcher` | Library | US-002 Watcher | `notify`-based file listener, debounced re-index trigger |
| `syncmind-mcp-server` | Library | US-007 MCP | JSON-RPC server, `search_knowledge` tool handler |

### Data Flow

```
CLI / MCP Client
       ↓
   syncmind (binary)
       ↓
   Config (syncmind-core)
       ↓
File Watcher → Extractor → Chunker → Embedder → VectorStore
                                                 (SQLite)
```

### Crate Dependencies

```
syncmind (bin)
  ├── syncmind-core
  ├── syncmind-storage
  ├── syncmind-rag-engine
  ├── syncmind-file-watcher
  └── syncmind-mcp-server

syncmind-mcp-server
  ├── syncmind-core
  ├── syncmind-rag-engine
  └── syncmind-storage

syncmind-file-watcher
  ├── syncmind-core
  ├── syncmind-rag-engine
  └── syncmind-storage

syncmind-rag-engine
  └── syncmind-core

syncmind-storage
  └── syncmind-core
```

## Component Design

### 1. `syncmind-core` — Config & Shared Types

**`config.rs`**
- `Config` struct: `ollama_url`, `ollama_model`, `mcp_transport` (`stdio` | `sse`), `bind_addr`, `registered_files: Vec<PathBuf>`, `embedding_dim: usize`, `chunk_size: usize`, `chunk_overlap: usize`
- `Config::load() -> Result<Config>`: reads `~/.config/syncmind/config.toml` via `dirs::config_dir()`
- `Config::ensure_default() -> Result<()>`: writes a default config if missing
- `Config::save() -> Result<()>`: persists changes (used by `register`/`unregister`)

**`paths.rs`**
- `data_dir() -> PathBuf`: `~/.local/share/syncmind/`
- `db_path() -> PathBuf`: `~/.local/share/syncmind/syncmind.db`
- `log_dir() -> PathBuf`: `~/.local/share/syncmind/logs/`
- `model_cache_dir() -> PathBuf`: `~/.local/share/syncmind/models/`

### 2. `syncmind-storage` — Vector Store

**`store.rs`**
- `VectorStore { conn: rusqlite::Connection }`
- `new(db_path: &Path) -> Result<Self>`: opens connection, loads `sqlite-vec` extension, creates schema
- `upsert_file(meta: FileMeta, chunks: Vec<Chunk>, embeddings: Vec<Vec<f32>>) -> Result<()>`: transactional replace
- `search(query: &[f32], top_k: usize, filter_file_type: Option<&[String]>) -> Result<Vec<SearchResult>>`
- `get_stats() -> Result<(usize, usize)>`: file count, chunk count

**Schema:**
```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    absolute_path TEXT UNIQUE NOT NULL,
    file_type TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    last_indexed INTEGER NOT NULL
);

CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content TEXT NOT NULL
);

CREATE VIRTUAL TABLE vec_chunks USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    embedding FLOAT32[{dim}]
);
```

**`models.rs`**
- `FileMeta`, `Chunk`, `SearchResult` structs

### 3. `syncmind-rag-engine` — RAG Pipeline

**`extractor.rs`**
- `trait Extractor { fn extract(path: &Path) -> Result<String>; }`
- `MarkdownExtractor` (`pulldown-cmark`)
- `CodeExtractor` (raw text by extension)
- `PdfExtractor` (`pdf-extract` or `lopdf`)
- `ImageOcrExtractor` (stub for Phase 2)
- `CompositeExtractor`: dispatches by file extension

**`chunker.rs`**
- `trait Chunker { fn chunk(text: &str, path: &Path) -> Vec<Chunk>; }`
- `MarkdownChunker`: split by heading hierarchy, then overlap-window if chunk exceeds `chunk_size`
- `CodeChunker`: `tree-sitter` AST-based (function/class boundaries), fallback to fixed window
- `FallbackChunker`: fixed-size overlapping window

**`embedder.rs`**
- `trait Embedder { fn embed(texts: &[&str]) -> Result<Vec<Vec<f32>>>; }`
- `OllamaEmbedder`: HTTP POST to `/api/embed`, batch support
- `OnnxEmbedder`: `ort` crate, loads cached `bge-small-en-v1.5` ONNX model
- `AutoEmbedder`: probes Ollama health on init, falls back to ONNX with `tracing::info` log

### 4. `syncmind-file-watcher` — Watcher

**`watcher.rs`**
- `FileWatcher { watcher: notify::RecommendedWatcher, tx: mpsc::Sender<PathEvent> }`
- Watches all paths in `Config::registered_files`
- Debounces events (e.g., 500ms) to avoid re-indexing on rapid saves
- Emits `PathEvent { path, kind: Modified | Removed | Renamed }`
- On `register`/`unregister` CLI commands: rebuild watcher without restart

### 5. `syncmind-mcp-server` — MCP Protocol

**`server.rs`**
- `McpServer { store: Arc<VectorStore>, embedder: Arc<dyn Embedder> }`
- `initialize` handshake returning capabilities (`tools`)
- `tools/list` exposing `search_knowledge`
- `tools/call` handler for `search_knowledge`

**`transport/`**
- `stdio.rs`: reads stdin lines as JSON-RPC, writes responses to stdout (logs → stderr)
- `sse.rs`: `axum` HTTP server with `/sse` endpoint and POST callback

### 6. `syncmind` — Binary CLI

**`main.rs`**
- `clap` derive-based CLI
- Subcommands: `daemon [--foreground]`, `register <path>`, `unregister <path>`, `status`, `search <query> [--top-k N]`
- `daemon` init flow: load config → init storage → full index of registered files → start watcher → start MCP server
- `--foreground`: `tracing-subscriber` pretty printer to stderr

## Error Handling

- **Binary / orchestration:** `anyhow` for ergonomic error propagation
- **Library APIs:** `thiserror` for structured errors (`StorageError`, `EmbedError`, `ExtractError`, etc.)
- **MCP failures:** map to JSON-RPC 2.0 error objects with appropriate codes

## Concurrency Model

- `tokio` multi-thread runtime everywhere
- CPU-intensive tasks (ONNX inference, tree-sitter parsing) use `tokio::task::spawn_blocking`
- Storage writes serialized via `tokio::sync::Semaphore(1)` to avoid SQLite write-queue contention
- Config changes propagated via `tokio::sync::watch` channel

## Configuration

**`~/.config/syncmind/config.toml`**
```toml
ollama_url = "http://localhost:11434"
ollama_model = "bge-m3"
mcp_transport = "stdio"  # or "sse"
bind_addr = "127.0.0.1:3000"
embedding_dim = 1024
chunk_size = 512
chunk_overlap = 50
registered_files = []
```

## Testing Strategy

- **Unit tests:** Config roundtrip, storage CRUD with `:memory:` SQLite, extractor output validation
- **Integration tests:** Full pipeline from file path → search results using mock embedder
- **E2E tests:** Manual/scripted Claude Code MCP invocation

## Privacy & Security

- All raw text and vectors stay on local filesystem
- No external network calls except localhost Ollama HTTP
- No hardcoded API keys
- Database and logs in `~/.local/share/syncmind/`
