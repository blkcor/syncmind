## Context

SyncMind Phase 1 core currently implements a single-stage pure-vector retrieval pipeline:

1. `Extractor` reads file text.
2. `Chunker` splits into chunks. `CodeChunker` uses tree-sitter for Rust/Python/JS but falls back to character-count splitting for other languages and oversized functions.
3. `Embedder` generates vectors (Ollama bge-m3 or ONNX bge-small).
4. `VectorStore` persists chunks and vectors in SQLite + sqlite-vec.
5. `McpServer` exposes `search_knowledge` which embeds the query and performs a single `top_k` vector lookup.

The constraints are strict: all data stays local, idle memory < 100 MB, and the daemon must remain lightweight.

## Goals / Non-Goals

**Goals:**
- Eliminate semantically broken code chunks (especially for Go and oversized functions).
- Guarantee that irrelevant queries do not return forced `top_k` noise.
- Improve retrieval precision for exact-symbol queries (e.g., function names) via lexical fallback.
- Provide an optional reranking stage for users willing to trade disk/model cache for better result ordering.

**Non-Goals:**
- Multi-hop / graph RAG (out of scope per PRD NG-5).
- Multi-query expansion using LLM (too slow and heavy for Phase 1).
- Automatic language detection beyond file extension.
- UI changes in `apps/` (tracked separately; this change focuses on core crates).
- Embedding model switching mid-lifecycle.

## Decisions

### 1. FTS5 for lexical search (not Tantivy, not custom BM25)
**Rationale:** SQLite already ships with FTS5 in most builds (including `rusqlite` bundled). Adding Tantivy would introduce a large new dependency and a second index directory. FTS5 gives us BM25 ranking, prefix queries, and tokenization with zero additional binary dependencies.
**Alternative considered:** `tantivy` crate — rejected because of dependency weight and second storage backend.

### 2. RRF (Reciprocal Rank Fusion) for score merging
**Rationale:** Vector similarity (cosine/L2) and BM25 scores live on incompatible scales. RRF requires no calibration, no training data, and no query-time normalization. It is robust and well-understood in RAG literature.
**Alternative considered:** Weighted linear combination — rejected because it requires tuning weights per corpus and is brittle when one index returns no results.

### 3. Reranker is opt-in and gated by memory check
**Rationale:** A cross-encoder ONNX model (e.g., `bge-reranker-base`) can be 100–150 MB on disk and ~80–120 MB resident. That is acceptable for users who want it, but it must not push the idle daemon over the 100 MB budget. Making it opt-in and enforcing a startup memory check keeps Phase 1 lightweight by default.
**Alternative considered:** Always-on reranker — rejected because it violates the default memory constraint.

### 4. Parent-function signature prefixing for sub-chunks
**Rationale:** When a long function is split, downstream LLMs lose the context of "what function is this?" Prepending the function signature to every sub-chunk preserves semantic continuity without increasing the number of chunks.
**Alternative considered:** Hierarchical chunking with parent links — rejected because it complicates the storage schema and MCP response format for marginal gain.

### 5. Distance threshold applied before fusion / reranking
**Rationale:** Filtering low-similarity vector hits early reduces the candidate pool for RRF and reranker, saving compute. The threshold is expressed as a normalized cosine similarity (0–1) because it is intuitive for users and configuration.
**Alternative considered:** Per-rank cutoff — rejected because raw BM25 and distance scores are not directly comparable; a unified similarity threshold on the vector arm is the cleanest pre-filter.

## Risks / Trade-offs

- **[Risk]** FTS5 requires rebuilding the index for existing databases.
  → **Mitigation:** Detect schema version mismatch on startup. If the FTS5 table is missing, trigger a full re-index of all registered files automatically.
- **[Risk]** Parent-function prefix duplication inflates chunk text and embedding cost.
  → **Mitigation:** Prefix is limited to the function signature line only (typically 1 line). For extremely long signatures, truncate at 200 chars.
- **[Risk]** RRF with small `k` (e.g., `top_k = 3`) produces tiny candidate pools, reducing hybrid benefit.
  → **Mitigation:** Internal candidate pool is always `top_k * 2` from each arm, so RRF has enough overlap to work with. Document that `top_k < 3` is not recommended for hybrid mode.
- **[Risk]** ONNX reranker model download on first enable is a network call.
  → **Mitigation:** Reuse the same `ensure_onnx_assets` pattern already used by the embedder. Document the download in config comments.
- **[Risk]** Go tree-sitter grammar adds compile time and binary size.
  → **Mitigation:** `tree-sitter-go` is a small C grammar. It is only linked if the `go` feature flag is enabled on `rag-engine` (enabled by default in workspace).

## Migration Plan

1. On daemon startup, `VectorStore` checks for the presence of `fts_chunks`.
2. If absent, it sets a `needs_reindex` flag and logs an `info` message.
3. The file watcher pipeline iterates over all registered files and re-upserts them (this happens automatically because the watcher sees them as "new" on first scan).
4. The `vec_chunks` table is untouched; only `fts_chunks` needs population.
5. Rollback: delete `fts_chunks` via SQLite CLI and revert config to `hybrid_search_enabled = false`.

## Open Questions

1. Should the `relevance_threshold` default be calibrated per embedder (bge-m3 vs bge-small have different score distributions)?
2. Should we expose a `hybrid_weight` parameter to let users bias toward lexical vs semantic, or keep pure RRF with no knobs?
3. Is `bge-reranker-base` the right default model, or should we target a smaller ONNX variant (e.g., ~50 MB) to stay comfortably under the memory gate?
