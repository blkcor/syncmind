# Milestone 1: Foundation + Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a compilable Rust workspace with config loading, path management, and a SQLite + sqlite-vec `VectorStore` that can upsert files with chunks/embeddings and perform vector similarity search.

**Architecture:** Add `syncmind-core` (shared types + config) and `syncmind` (binary + CLI) crates to the existing workspace. `syncmind-storage` implements the `VectorStore` with `rusqlite` and `sqlite-vec` statically linked. All paths use `dirs` for XDG compliance.

**Tech Stack:** Rust, tokio, anyhow, thiserror, tracing, serde, toml, clap, dirs, rusqlite (bundled), sqlite-vec (alpha), zerocopy

---

### Task 1: Update workspace `Cargo.toml`

**Files:**
- Modify: `core/Cargo.toml`

- [ ] **Step 1: Add new members and dependencies**

```toml
[workspace]
members = [
    "syncmind",
    "syncmind-core",
    "mcp-server",
    "file-watcher",
    "rag-engine",
    "storage",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["SyncMind Contributors"]
license = "MIT OR Apache-2.0"
repository = "https://github.com/blkcor-bt/syncmind"

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
clap = { version = "4", features = ["derive"] }
dirs = "6"
thiserror = "1"
rusqlite = { version = "0.31", features = ["bundled"] }
sqlite-vec = "0.1.10-alpha.3"
zerocopy = { version = "0.8", features = ["derive"] }
```

- [ ] **Step 2: Verify workspace structure**

Run: `cd core && cargo check --workspace`
Expected: FAIL with "member path `syncmind` does not exist" (expected — crates not created yet)

---

### Task 2: Create `syncmind-core` crate

**Files:**
- Create: `core/syncmind-core/Cargo.toml`
- Create: `core/syncmind-core/src/lib.rs`
- Create: `core/syncmind-core/src/config.rs`
- Create: `core/syncmind-core/src/paths.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "syncmind-core"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
anyhow = { workspace = true }
serde = { workspace = true }
toml = { workspace = true }
tracing = { workspace = true }
dirs = { workspace = true }
```

- [ ] **Step 2: Write `src/config.rs`**

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub ollama_url: String,
    pub ollama_model: String,
    pub mcp_transport: McpTransport,
    pub bind_addr: String,
    pub registered_files: Vec<PathBuf>,
    pub embedding_dim: usize,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Sse,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "bge-m3".to_string(),
            mcp_transport: McpTransport::Stdio,
            bind_addr: "127.0.0.1:3000".to_string(),
            registered_files: Vec::new(),
            embedding_dim: 1024,
            chunk_size: 512,
            chunk_overlap: 50,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let default = Config::default();
            default.save()?;
            tracing::info!("Created default config at {}", path.display());
            return Ok(default);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        Ok(dir.join("syncmind").join("config.toml"))
    }
}
```

- [ ] **Step 3: Write `src/paths.rs`**

```rust
use std::path::PathBuf;

pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?;
    Ok(dir.join("syncmind"))
}

pub fn db_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("syncmind.db"))
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("logs"))
}

pub fn model_cache_dir() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("models"))
}
```

- [ ] **Step 4: Write `src/lib.rs`**

```rust
pub mod config;
pub mod paths;

pub use config::{Config, McpTransport};
pub use paths::{data_dir, db_path, log_dir, model_cache_dir};
```

- [ ] **Step 5: Run cargo check**

Run: `cd core/syncmind-core && cargo check`
Expected: PASS (clean compilation)

---

### Task 3: Add unit tests for `syncmind-core`

**Files:**
- Modify: `core/syncmind-core/src/config.rs`

- [ ] **Step 1: Append tests to `config.rs`**

Add this to the bottom of `core/syncmind-core/src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let original = Config {
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "bge-m3".to_string(),
            mcp_transport: McpTransport::Sse,
            bind_addr: "0.0.0.0:8080".to_string(),
            registered_files: vec![PathBuf::from("/tmp/test.md")],
            embedding_dim: 384,
            chunk_size: 256,
            chunk_overlap: 25,
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();
        std::fs::write(&config_path, toml_str).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        let loaded: Config = toml::from_str(&content).unwrap();

        assert_eq!(original, loaded);
    }

    #[test]
    fn default_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ollama_url"));
        assert!(toml_str.contains("stdio"));
    }
}
```

- [ ] **Step 2: Add `tempfile` dev dependency**

Add to `core/syncmind-core/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run tests**

Run: `cd core/syncmind-core && cargo test`
Expected: 2 tests PASS

---

### Task 4: Create `syncmind-storage` error types and models

**Files:**
- Modify: `core/storage/Cargo.toml`
- Create: `core/storage/src/lib.rs`
- Create: `core/storage/src/models.rs`
- Create: `core/storage/src/error.rs`

- [ ] **Step 1: Update `Cargo.toml`**

```toml
[package]
name = "syncmind-storage"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }
rusqlite = { workspace = true }
sqlite-vec = { workspace = true }
zerocopy = { workspace = true }
syncmind-core = { path = "../syncmind-core" }
```

- [ ] **Step 2: Write `src/error.rs`**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Count mismatch: {chunks} chunks vs {embeddings} embeddings")]
    CountMismatch { chunks: usize, embeddings: usize },
    #[error("Invalid embedding dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },
}
```

- [ ] **Step 3: Write `src/models.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub absolute_path: PathBuf,
    pub file_type: String,
    pub last_modified: i64,
    pub last_indexed: i64,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub chunk_index: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub file_path: PathBuf,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
}
```

- [ ] **Step 4: Write `src/lib.rs`**

```rust
pub mod error;
pub mod models;
pub mod store;

pub use error::StorageError;
pub use models::{Chunk, FileMeta, SearchResult};
pub use store::VectorStore;
```

- [ ] **Step 5: Run cargo check**

Run: `cd core/storage && cargo check`
Expected: PASS (clean compilation of stubs)

---

### Task 5: Implement `VectorStore` schema and initialization

**Files:**
- Create: `core/storage/src/store.rs`

- [ ] **Step 1: Write schema initialization**

```rust
use crate::error::StorageError;
use rusqlite::Connection;
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;

pub struct VectorStore {
    conn: Connection,
    embedding_dim: usize,
}

impl VectorStore {
    pub fn new(db_path: &Path, embedding_dim: usize) -> Result<Self, StorageError> {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(
                std::mem::transmute(sqlite3_vec_init as *const ()),
            ));
        }
        let conn = Connection::open(db_path)?;
        let store = Self { conn, embedding_dim };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                absolute_path TEXT UNIQUE NOT NULL,
                file_type TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                last_indexed INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                chunk_index INTEGER NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL
            );",
        )?;

        let vec_table_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding FLOAT32[{}]
            );",
            self.embedding_dim
        );
        self.conn.execute(&vec_table_sql, [])?;

        Ok(())
    }
}
```

- [ ] **Step 2: Run cargo check**

Run: `cd core/storage && cargo check`
Expected: PASS (or compilation errors to fix)

---

### Task 6: Implement `VectorStore::upsert_file`

**Files:**
- Modify: `core/storage/src/store.rs`

- [ ] **Step 1: Add `upsert_file` method**

Append inside `impl VectorStore`:

```rust
use crate::models::{Chunk, FileMeta};
use rusqlite::params;
use zerocopy::AsBytes;

pub fn upsert_file(
    &self,
    meta: &FileMeta,
    chunks: &[Chunk],
    embeddings: &[Vec<f32>],
) -> Result<(), StorageError> {
    if chunks.len() != embeddings.len() {
        return Err(StorageError::CountMismatch {
            chunks: chunks.len(),
            embeddings: embeddings.len(),
        });
    }

    let tx = self.conn.unchecked_transaction()?;

    let file_id: Option<i64> = self
        .conn
        .query_row(
            "SELECT id FROM files WHERE absolute_path = ?",
            [meta.absolute_path.to_string_lossy().as_ref()],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = file_id {
        self.conn
            .execute("DELETE FROM files WHERE id = ?", [id])?;
    }

    self.conn.execute(
        "INSERT INTO files (absolute_path, file_type, last_modified, last_indexed)
         VALUES (?, ?, ?, ?)",
        params![
            meta.absolute_path.to_string_lossy().as_ref(),
            &meta.file_type,
            meta.last_modified,
            meta.last_indexed,
        ],
    )?;
    let file_id = self.conn.last_insert_rowid();

    for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
        if embedding.len() != self.embedding_dim {
            return Err(StorageError::InvalidDimension {
                expected: self.embedding_dim,
                actual: embedding.len(),
            });
        }

        self.conn.execute(
            "INSERT INTO chunks (file_id, chunk_index, start_line, end_line, content)
             VALUES (?, ?, ?, ?, ?)",
            params![
                file_id,
                chunk.chunk_index,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
            ],
        )?;
        let chunk_id = self.conn.last_insert_rowid();

        self.conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?, ?)",
            params![chunk_id, embedding.as_bytes()],
        )?;
    }

    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 2: Run cargo check**

Run: `cd core/storage && cargo check`
Expected: PASS

---

### Task 7: Implement `VectorStore::search` and `get_stats`

**Files:**
- Modify: `core/storage/src/store.rs`

- [ ] **Step 1: Add `search` and `get_stats` methods**

Append inside `impl VectorStore`:

```rust
use crate::models::SearchResult;

pub fn search(
    &self,
    query_embedding: &[f32],
    top_k: usize,
) -> Result<Vec<SearchResult>, StorageError> {
    if query_embedding.len() != self.embedding_dim {
        return Err(StorageError::InvalidDimension {
            expected: self.embedding_dim,
            actual: query_embedding.len(),
        });
    }

    let mut stmt = self.conn.prepare(
        "SELECT
            c.id,
            c.start_line,
            c.end_line,
            c.content,
            f.absolute_path,
            vc.distance
         FROM vec_chunks vc
         JOIN chunks c ON vc.chunk_id = c.id
         JOIN files f ON c.file_id = f.id
         WHERE vc.embedding MATCH ?
         ORDER BY vc.distance
         LIMIT ?",
    )?;

    let rows = stmt.query_map(
        params![query_embedding.as_bytes(), top_k as i64],
        |row| {
            Ok(SearchResult {
                chunk_id: row.get(0)?,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                content: row.get(3)?,
                file_path: PathBuf::from(row.get::<_, String>(4)?),
                score: row.get(5)?,
            })
        },
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub fn get_stats(&self) -> Result<(usize, usize), StorageError> {
    let file_count: usize = self
        .conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
    let chunk_count: usize = self
        .conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
    Ok((file_count, chunk_count))
}
```

Add `use std::path::PathBuf;` to the top of `store.rs` if not already present.

- [ ] **Step 2: Run cargo check**

Run: `cd core/storage && cargo check`
Expected: PASS

---

### Task 8: Add integration tests for `VectorStore`

**Files:**
- Modify: `core/storage/src/store.rs`

- [ ] **Step 1: Append tests to `store.rs`**

Add this to the bottom of `core/storage/src/store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Chunk, FileMeta};
    use std::path::PathBuf;

    fn mock_embedding(dim: usize, value: f32) -> Vec<f32> {
        vec![value; dim]
    }

    #[test]
    fn store_init_and_upsert() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1234567890,
            last_indexed: 1234567890,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 5,
                content: "Hello world".to_string(),
            },
        ];
        let embeddings = vec![mock_embedding(4, 0.1)];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let (files, chunks_count) = store.get_stats().unwrap();
        assert_eq!(files, 1);
        assert_eq!(chunks_count, 1);
    }

    #[test]
    fn store_search_returns_results() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1234567890,
            last_indexed: 1234567890,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 5,
                content: "Hello world".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 6,
                end_line: 10,
                content: "Goodbye world".to_string(),
            },
        ];
        let embeddings = vec![
            vec![0.1, 0.2, 0.3, 0.4],
            vec![0.9, 0.8, 0.7, 0.6],
        ];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let query = vec![0.11, 0.19, 0.31, 0.39];
        let results = store.search(&query, 2).unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].content, "Hello world");
    }

    #[test]
    fn store_upsert_replaces_existing_file() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![Chunk {
            chunk_index: 0,
            start_line: 1,
            end_line: 2,
            content: "First".to_string(),
        }];
        let embeddings = vec![mock_embedding(4, 0.1)];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let (files, chunks_count) = store.get_stats().unwrap();
        assert_eq!(files, 1);
        assert_eq!(chunks_count, 1);
    }
}
```

- [ ] **Step 2: Add `tempfile` dev dependency to `core/storage/Cargo.toml`**

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run tests**

Run: `cd core/storage && cargo test`
Expected: 3 tests PASS

---

### Task 9: Create `syncmind` binary crate

**Files:**
- Create: `core/syncmind/Cargo.toml`
- Create: `core/syncmind/src/main.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "syncmind"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "syncmind"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
clap = { workspace = true }
syncmind-core = { path = "../syncmind-core" }
syncmind-storage = { path = "../storage" }
```

- [ ] **Step 2: Write `src/main.rs`**

```rust
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser)]
#[command(name = "syncmind")]
#[command(about = "SyncMind - Local context engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Daemon {
        #[arg(long)]
        foreground: bool,
    },
    Register { path: std::path::PathBuf },
    Unregister { path: std::path::PathBuf },
    Status,
    Search {
        query: String,
        #[arg(long, default_value = "5")]
        top_k: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { foreground } => {
            if foreground {
                tracing_subscriber::fmt::init();
            }
            info!("Starting SyncMind daemon...");
            let config = syncmind_core::Config::load()?;
            let db_path = syncmind_core::db_path()?;
            std::fs::create_dir_all(db_path.parent().unwrap())?;
            let _store = syncmind_storage::VectorStore::new(&db_path, config.embedding_dim)?;
            info!("Daemon initialized successfully");
            tokio::signal::ctrl_c().await?;
            info!("Shutting down...");
        }
        Commands::Register { path } => {
            println!("Registering {} (not yet implemented)", path.display());
        }
        Commands::Unregister { path } => {
            println!("Unregistering {} (not yet implemented)", path.display());
        }
        Commands::Status => {
            println!("Status: not yet implemented");
        }
        Commands::Search { query, top_k } => {
            println!("Searching for '{}' (top_k={}) (not yet implemented)", query, top_k);
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Run cargo check**

Run: `cd core/syncmind && cargo check`
Expected: PASS

---

### Task 10: Full workspace verification

**Files:**
- Modify: `core/Cargo.toml` (ensure members are correct)

- [ ] **Step 1: Run workspace check**

Run: `cd core && cargo check --workspace`
Expected: PASS for all 6 crates

- [ ] **Step 2: Run workspace tests**

Run: `cd core && cargo test --workspace`
Expected: All tests PASS

- [ ] **Step 3: Run clippy**

Run: `cd core && cargo clippy --workspace -- -D warnings`
Expected: PASS with no warnings

- [ ] **Step 4: Smoke test the binary**

Run: `cd core && cargo run --bin syncmind -- daemon --foreground`
Let it run for 2 seconds, then Ctrl+C.
Expected: Starts without panic, creates `~/.config/syncmind/config.toml` and `~/.local/share/syncmind/syncmind.db` if they don't exist.

---

### Task 11: Update master `tasks.md`

**Files:**
- Create: `core/tasks.md` (or update existing)

- [ ] **Step 1: Write progress to `docs/spec/001-headless-mcp-core/tasks.md`**

```markdown
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
- [ ] Extractor trait + Markdown/Code/PDF implementations
- [ ] Chunker trait + Markdown/Code/Fallback implementations
- [ ] Embedder trait + Ollama + ONNX fallback
- [ ] Unit tests for all RAG components

## Milestone 3: File Watch & CLI
- [ ] notify-based file watcher with debouncing
- [ ] `register` / `unregister` commands with config hot-reload
- [ ] `status` and `search` CLI commands

## Milestone 4: MCP Server & E2E
- [ ] MCP Stdio transport
- [ ] MCP SSE transport
- [ ] `search_knowledge` tool implementation
- [ ] E2E tests and Claude Code integration examples
```

- [ ] **Step 2: Commit**

```bash
git add core/
git add docs/spec/001-headless-mcp-core/
git commit -m "feat(core): milestone 1 - foundation and storage

- Add syncmind-core crate with Config and path utilities
- Add syncmind-storage crate with VectorStore (SQLite + sqlite-vec)
- Add syncmind binary with CLI skeleton
- Implement config roundtrip and storage CRUD tests

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```
