# Milestone 2: RAG Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Implement the full RAG pipeline in `syncmind-rag-engine`: text extraction, semantic chunking, and embedding generation with Ollama/ONNX fallback.

**Architecture:** Three trait-based modules (`Extractor`, `Chunker`, `Embedder`) with format-specific implementations, unified error types, and comprehensive unit tests.

**Tech Stack:** `pulldown-cmark`, `pdf-extract`, `tree-sitter`, `reqwest`, `ort`

---

## Task 1: Extractor trait + Markdown/Code/PDF implementations

**Files:**
- Create: `core/rag-engine/src/extractor.rs`
- Modify: `core/rag-engine/Cargo.toml`
- Modify: `core/rag-engine/src/lib.rs`
- Test: `core/rag-engine/src/extractor.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Add dependencies to `core/rag-engine/Cargo.toml`**

```toml
[dependencies]
pulldown-cmark = "0.12"
pdf-extract = "0.7"
tree-sitter = "0.24"
reqwest = { version = "0.12", features = ["json"] }
ort = "2"
syncmind-core = { path = "../syncmind-core" }
thiserror = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
```

- [ ] **Step 2: Define `ExtractError` in `core/rag-engine/src/error.rs` (new file)**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExtractError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF extraction failed: {0}")]
    Pdf(String),
    #[error("Unsupported file type: {0}")]
    Unsupported(String),
}
```

- [ ] **Step 3: Define `Extractor` trait and implementations in `core/rag-engine/src/extractor.rs`**

```rust
use std::path::Path;
use crate::error::ExtractError;

pub trait Extractor: Send + Sync {
    fn extract(&self, path: &Path) -> Result<String, ExtractError>;
    fn can_handle(&self, path: &Path) -> bool;
}

pub struct MarkdownExtractor;
pub struct CodeExtractor;
pub struct PdfExtractor;
pub struct CompositeExtractor {
    extractors: Vec<Box<dyn Extractor>>,
}
```

Implementations:
- `MarkdownExtractor`: uses `pulldown-cmark` to parse `.md` files and emit plain text (filter out YAML frontmatter optionally).
- `CodeExtractor`: reads any recognized code extension (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, `.c`, `.cpp`, `.h`, `.hpp`, etc.) as raw UTF-8 text.
- `PdfExtractor`: uses `pdf-extract` to extract text from `.pdf` files.
- `CompositeExtractor`: iterates extractors, returns first `can_handle` match. Falls back to `CodeExtractor` for unknown text files.

- [ ] **Step 4: Write unit tests**

Tests:
- `test_markdown_extracts_plain_text` — create a temp `.md` file, verify headings and paragraphs are extracted.
- `test_code_extracts_raw_text` — create a temp `.rs` file, verify content matches.
- `test_pdf_extracts_text` — skip if no PDF sample available; at minimum test error handling.
- `test_composite_dispatches_by_extension` — verify correct extractor is chosen.

- [ ] **Step 5: Update `core/rag-engine/src/lib.rs`**

```rust
pub mod error;
pub mod extractor;
```

- [ ] **Step 6: Run tests**

```bash
cd core && cargo test -p syncmind-rag-engine
```

Expected: All extractor tests pass.

- [ ] **Step 7: Commit**

```bash
git add core/rag-engine/
git commit -m "feat(core:rag-engine): add Extractor trait and file extractors"
```

---

## Task 2: Chunker trait + Markdown/Code/Fallback implementations

**Files:**
- Create: `core/rag-engine/src/chunker.rs`
- Modify: `core/rag-engine/src/lib.rs`
- Test: `core/rag-engine/src/chunker.rs` (inline tests)

- [ ] **Step 1: Define `Chunker` trait and `Chunk` struct in `core/rag-engine/src/chunker.rs`**

Reuse `Chunk` from `syncmind-storage` models, or define a local `RawChunk` and convert later.

```rust
use std::path::Path;
use syncmind_storage::models::Chunk;

pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str, path: &Path) -> Vec<Chunk>;
}

pub struct MarkdownChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

pub struct CodeChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

pub struct FallbackChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}
```

- [ ] **Step 2: Implement `FallbackChunker`**

Fixed-size overlapping window:
- Split text into lines.
- Build chunks of approximately `chunk_size` chars (not tokens; char count is acceptable for Phase 1).
- Each chunk overlaps previous by `chunk_overlap` chars.
- Track `start_line` and `end_line` (1-indexed).
- Assign `chunk_index` sequentially.

- [ ] **Step 3: Implement `MarkdownChunker`**

Two-phase chunking:
1. Split by heading hierarchy (`#`, `##`, `###`). Each heading section is a logical unit.
2. If a section exceeds `chunk_size`, further split using `FallbackChunker` logic within that section.
3. Update `start_line`/`end_line` to reflect original file positions.

- [ ] **Step 4: Implement `CodeChunker`**

Language-aware chunking using `tree-sitter`:
1. Parse file with `tree-sitter` based on extension.
2. Query for function / class / struct / method definitions.
3. Each definition body becomes a chunk.
4. If definition exceeds `chunk_size`, fallback to `FallbackChunker` on that body.
5. If `tree-sitter` parsing fails or language unsupported, fallback entirely to `FallbackChunker`.

For Phase 1, support at minimum:
- Rust (`tree-sitter-rust`)
- Python (`tree-sitter-python`)
- JavaScript / TypeScript (`tree-sitter-javascript`)

- [ ] **Step 5: Write unit tests**

Tests:
- `test_fallback_chunker_splits_text` — verify chunk count, overlap, line numbers.
- `test_markdown_chunker_respects_headings` — verify heading boundaries are preserved.
- `test_code_chunker_splits_by_functions` — create a temp `.rs` file with multiple functions, verify each becomes a chunk.
- `test_chunk_line_numbers_are_1_indexed` — verify start_line >= 1.

- [ ] **Step 6: Run tests**

```bash
cd core && cargo test -p syncmind-rag-engine
```

Expected: All chunker tests pass.

- [ ] **Step 7: Commit**

```bash
git add core/rag-engine/
git commit -m "feat(core:rag-engine): add Chunker trait and semantic chunkers"
```

---

## Task 3: Embedder trait + Ollama + ONNX fallback

**Files:**
- Create: `core/rag-engine/src/embedder.rs`
- Modify: `core/rag-engine/src/lib.rs`
- Test: `core/rag-engine/src/embedder.rs` (inline tests)

- [ ] **Step 1: Define `EmbedError` in `core/rag-engine/src/error.rs`**

```rust
#[derive(Error, Debug)]
pub enum EmbedError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("ONNX inference failed: {0}")]
    Onnx(String),
    #[error("Ollama unreachable or model missing: {0}")]
    OllamaUnavailable(String),
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 2: Define `Embedder` trait in `core/rag-engine/src/embedder.rs`**

```rust
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embedding_dim(&self) -> usize;
}
```

Add `async-trait = "0.1"` to `Cargo.toml`.

- [ ] **Step 3: Implement `OllamaEmbedder`**

```rust
pub struct OllamaEmbedder {
    client: reqwest::Client,
    url: String,
    model: String,
    embedding_dim: usize,
}
```

`embed` method:
1. POST to `{url}/api/embed` with JSON body: `{ "model": "...", "input": texts }`.
2. Parse response JSON to extract `embeddings` array.
3. Validate each embedding length matches `embedding_dim`.
4. Return `Vec<Vec<f32>>`.

- [ ] **Step 4: Implement `OnnxEmbedder`**

```rust
pub struct OnnxEmbedder {
    session: ort::Session,
    embedding_dim: usize,
}
```

`embed` method:
1. Use `ort` to load ONNX model from `~/.local/share/syncmind/models/bge-small-en-v1.5.onnx`.
2. Tokenize texts (simple whitespace splitting or basic tokenization acceptable for Phase 1).
3. Run inference batch.
4. Validate output dimensions.
5. Return `Vec<Vec<f32>>`.

For Phase 1, if the model file does not exist, return a clear error. (Auto-download can be added in Milestone 4 or later.)

- [ ] **Step 5: Implement `AutoEmbedder`**

```rust
pub struct AutoEmbedder {
    inner: Box<dyn Embedder>,
}
```

`new(config: &Config) -> Result<Self, EmbedError>`:
1. Try to probe Ollama: GET `{ollama_url}` or POST `/api/embed` with a test string.
2. If reachable and returns valid embedding: use `OllamaEmbedder`.
3. Else: log `tracing::info!("Ollama unavailable, falling back to ONNX embedder")`.
4. Try to initialize `OnnxEmbedder`.
5. If both fail: return `EmbedError::OllamaUnavailable`.

- [ ] **Step 6: Write unit tests**

Tests:
- `test_ollama_embedder_mock` — mock HTTP server or test serialization/deserialization of request/response.
- `test_auto_embedder_picks_ollama_when_available` — mock probe success.
- `test_auto_embedder_falls_back_to_onnx` — mock probe failure, verify ONNX path taken.
- `test_dimension_mismatch_errors` — verify `EmbedError::DimensionMismatch` is returned.

For tests that require actual HTTP or ONNX, use `#[ignore]` or mock the dependencies.

- [ ] **Step 7: Run tests**

```bash
cd core && cargo test -p syncmind-rag-engine
```

Expected: All embedder tests pass.

- [ ] **Step 8: Commit**

```bash
git add core/rag-engine/
git commit -m "feat(core:rag-engine): add Embedder trait with Ollama and ONNX fallback"
```

---

## Task 4: RAG unit tests and workspace verification

**Files:**
- Modify: `core/rag-engine/src/lib.rs`
- Modify: `core/Cargo.toml` (if new workspace deps needed)

- [ ] **Step 1: Add any missing workspace dependencies**

Ensure `async-trait`, `pulldown-cmark`, `pdf-extract`, `tree-sitter`, `reqwest`, `ort` are in `core/rag-engine/Cargo.toml` (not necessarily workspace-level unless shared).

- [ ] **Step 2: Run full workspace check**

```bash
cd core && cargo check
```

Expected: Zero errors across all crates.

- [ ] **Step 3: Run full workspace tests**

```bash
cd core && cargo test
```

Expected: All tests pass (previous 5 + new RAG tests).

- [ ] **Step 4: Run clippy**

```bash
cd core && cargo clippy
```

Expected: Zero warnings.

- [ ] **Step 5: Update `docs/spec/001-headless-mcp-core/tasks.md`**

Mark Milestone 2 tasks complete.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "test(core:rag-engine): add comprehensive unit tests for RAG pipeline"
```

---

## Spec Coverage Check

| PRD Requirement | Task |
|-----------------|------|
| US-003: Extractor trait, Markdown, Code, PDF | Task 1 |
| US-004: Chunker, chunk_size/overlap, Markdown headings, Code AST | Task 2 |
| US-005: Embedder trait, Ollama batch, ONNX fallback, AutoEmbedder | Task 3 |
| Unit tests for all RAG components | Task 4 |

## Placeholder Scan

- No "TBD" or "TODO" in code steps.
- All trait signatures, error variants, and struct fields are fully specified.
- Test cases include concrete assertions.
