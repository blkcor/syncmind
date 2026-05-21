## Why

Current RAG retrieval in the SyncMind core suffers from three systemic issues: (1) chunking splits code files at arbitrary character boundaries, producing semantically incoherent fragments; (2) pure vector search lacks relevance thresholds, forcing a fixed `top_k` and returning low-quality noise when no match exists; (3) there is no lexical fallback or reranking, so exact symbol matches (e.g., function names) and result ordering are suboptimal. These problems degrade the quality of context injected into downstream LLMs and must be resolved before Phase 1 is considered complete.

## What Changes

- **Semantic chunking overhaul**: Extend `CodeChunker` to support Go (tree-sitter) and improve sub-chunking of oversized functions using logical boundaries (blank lines, comment blocks, control-flow nesting) instead of raw character counts.
- **Hybrid search**: Introduce an SQLite FTS5 virtual table alongside `sqlite-vec`, and implement fused retrieval that combines BM25 lexical scores with vector similarity scores using RRF (Reciprocal Rank Fusion).
- **Relevance thresholding**: Add a configurable similarity/distance cutoff to `VectorStore::search` so that results below the threshold are discarded instead of padding `top_k` with irrelevant chunks.
- **Optional reranking**: Add a lightweight ONNX-based cross-encoder reranker (e.g., `bge-reranker`) as a post-retrieval ranking stage. This is opt-in via config to respect the <100 MB idle memory budget.
- **MCP tool schema updates**: Extend `search_knowledge` input schema to expose hybrid weights, reranking toggle, and relevance threshold parameters.
- **BREAKING**: `VectorStore` schema changes (new FTS5 table) require existing databases to be rebuilt on first launch after upgrade.

## Capabilities

### New Capabilities
- `semantic-chunking`: Language-aware chunking with AST-level boundary detection, extended language support (Go), and semantic sub-chunking for oversized code blocks.
- `hybrid-search`: Combined BM25 (FTS5) and vector similarity retrieval with RRF score fusion and configurable relevance thresholds.
- `retrieval-reranking`: Post-retrieval result reordering using a lightweight ONNX cross-encoder model, gated by configuration.

### Modified Capabilities
- `rag-lab`: UI parameter panel will need new controls (hybrid weight, reranker toggle, threshold slider). Requirement changes are additive and non-breaking for the core; UI adaptation tracked separately.

## Impact

- **Core crates affected**: `rag-engine` (chunker, embedder), `storage` (FTS5 schema + search fusion), `mcp-server` (tool schema + handler), `syncmind-core` (config fields).
- **New dependencies**: `tree-sitter-go` (chunker), `bge-reranker` ONNX model assets (~100–150 MB cache, not bundled).
- **Database migration**: FTS5 virtual table `fts_chunks` added; existing `syncmind.db` will be detected and flagged for re-index on schema version mismatch.
- **Config additions**: `hybrid_search_enabled`, `reranker_enabled`, `relevance_threshold`, `reranker_model_path` (optional).
