## Context

SyncMind Phase 1 is a polyglot monorepo with a Rust Cargo workspace at `core/`. The workspace contains 6 crates mapped to PRD domains. All data remains on the user's local machine — no cloud APIs except localhost Ollama HTTP.

Constraints:
- Idle memory < 100MB
- No external network calls except local Ollama
- All raw text and vectors stay on local filesystem

## Goals / Non-Goals

**Goals:**
- Build a compilable Rust workspace with config, storage, RAG pipeline, file watcher, MCP server, and CLI
- Implement `VectorStore` with SQLite + sqlite-vec for upsert and similarity search
- Implement `Extractor`, `Chunker`, and `Embedder` traits with multiple implementations
- Implement MCP `search_knowledge` tool exposed via Stdio and SSE transports
- Keep the daemon runnable without any UI

**Non-Goals:**
- UI (Web, Desktop, Mobile)
- Directory-level recursive watching
- Image OCR
- Cloud sync / Go gateway
- Graph RAG or multi-hop reasoning
- Multi-user / permissions
- Auto-download of ONNX models (manual setup for Phase 1)

## Decisions

**1. SQLite + sqlite-vec over dedicated vector DB**
- Rationale: Eliminates external service dependency, fits <100MB constraint, single-file portability
- Alternative considered: `pgvector` (requires PostgreSQL), `qdrant` (separate process)

**2. Ollama primary with ONNX fallback**
- Rationale: Ollama provides best-quality embeddings with local LLMs; ONNX enables offline operation when Ollama is unavailable
- Alternative considered: Always-ONNX (lower quality), remote APIs (violates privacy constraint)

**3. tokio multi-thread runtime everywhere**
- Rationale: Unified async runtime; CPU-intensive tasks use `spawn_blocking`
- Alternative considered: `async-std` (less ecosystem maturity for our deps)

**4. thiserror for library errors, anyhow for binary**
- Rationale: Structured errors in public APIs, ergonomic propagation in orchestration code
- Trade-off: Slightly more boilerplate in library crates

**5. Static sqlite-vec linking via `rusqlite::ffi::sqlite3_auto_extension`**
- Rationale: No need for users to install sqlite-vec separately
- Trade-off: `unsafe` block required for extension registration

## Risks / Trade-offs

**[Risk] sqlite-vec alpha stability** → Mitigation: Pin to specific alpha version, test thoroughly, have migration path to stable
**[Risk] ONNX model not present on first run** → Mitigation: Clear error message, document manual download step
**[Risk] tree-sitter language support gaps** → Mitigation: Fallback to `FallbackChunker` for unsupported languages
**[Risk] Ollama not running when daemon starts** → Mitigation: `AutoEmbedder` probes and falls back to ONNX
**[Risk] SQLite write contention under heavy file changes** → Mitigation: Serialize writes via `tokio::sync::Semaphore(1)`
