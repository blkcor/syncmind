## 1. Semantic Chunking

- [ ] 1.1 Add `tree-sitter-go` dependency to `rag-engine/Cargo.toml`
- [ ] 1.2 Extend `CodeChunker::language_from_extension` to map `.go` → `"go"`
- [ ] 1.3 Extend `CodeChunker::node_types_for_language` for Go (`function_declaration`, `method_declaration`, `type_spec`, `struct_type`)
- [ ] 1.4 Implement semantic sub-chunking strategy: split oversized AST nodes at blank-line boundaries first, then comment blocks, then `FallbackChunker` fallback
- [ ] 1.5 Prepend parent-function signature line to every semantic sub-chunk to preserve context
- [ ] 1.6 Add chunker unit tests for Go file parsing and sub-chunk line-number accuracy

## 2. Hybrid Search (Storage Layer)

- [ ] 2.1 Add FTS5 virtual table `fts_chunks` to `VectorStore::init_schema`
- [ ] 2.2 Update `VectorStore::upsert_file` to insert rows into `fts_chunks` alongside `chunks` and `vec_chunks`
- [ ] 2.3 Update `VectorStore::delete_file_by_path` to remove associated `fts_chunks` rows
- [ ] 2.4 Implement `VectorStore::search_hybrid` that retrieves `top_k * 2` candidates from both FTS5 (BM25) and `vec_chunks` (vector similarity)
- [ ] 2.5 Implement RRF score fusion (`score = Σ 1 / (60 + rank)`) and return top `k` by fused score
- [ ] 2.6 Add configurable relevance threshold filtering to `search` and `search_hybrid`
- [ ] 2.7 Add schema-version detection on `VectorStore::new`; trigger full re-index if `fts_chunks` is missing
- [ ] 2.8 Add storage unit tests for hybrid retrieval, RRF ordering, and threshold filtering

## 3. Reranking

- [ ] 3.1 Add `bge-reranker` ONNX model download logic (reuse `ensure_onnx_assets` pattern in `rag-engine`)
- [ ] 3.2 Define `Reranker` trait and implement `OnnxReranker` using `ort` session
- [ ] 3.3 Add startup memory-budget check: refuse to load reranker if resident model size exceeds 150 MB
- [ ] 3.4 Integrate optional reranker stage into retrieval pipeline (after search, before returning results)
- [ ] 3.5 Add reranker unit tests for scoring and batch inference

## 4. Configuration & MCP Integration

- [ ] 4.1 Add new fields to `Config`: `hybrid_search_enabled`, `reranker_enabled`, `relevance_threshold`, `reranker_model_path`
- [ ] 4.2 Update `search_knowledge` input schema to expose `hybrid` (bool), `threshold` (f64), and `rerank` (bool) parameters
- [ ] 4.3 Update `SearchKnowledgeHandler` to route queries to hybrid vs vector search, apply threshold, and conditionally invoke reranker
- [ ] 4.4 Update MCP server tests for new tool parameters and routing logic

## 5. Integration & Verification

- [ ] 5.1 Run `cargo check` across the core workspace and resolve compiler errors
- [ ] 5.2 Run `cargo test` across the core workspace; ensure all new and existing tests pass
- [ ] 5.3 Run `cargo clippy` and fix all warnings
- [ ] 5.4 Perform manual end-to-end test: register a Go file, index it, query with hybrid search enabled, and verify no semantically broken chunks appear in results
