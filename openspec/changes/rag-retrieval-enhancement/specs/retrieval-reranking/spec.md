## ADDED Requirements

### Requirement: Optional ONNX reranker model loading
The system SHALL be able to load a lightweight cross-encoder reranker model via ONNX Runtime when explicitly enabled in configuration.

#### Scenario: Reranker enabled with valid model
- **WHEN** `reranker_enabled = true` and a valid ONNX model exists at the configured path
- **THEN** the system SHALL load the model at startup
- **AND** it SHALL expose a `rerank(query, candidates) -> scored_candidates` interface

#### Scenario: Reranker disabled by default
- **WHEN** `reranker_enabled` is absent or `false`
- **THEN** no reranker model SHALL be loaded
- **AND** retrieval SHALL skip the reranking stage entirely

#### Scenario: Missing model graceful degradation
- **WHEN** `reranker_enabled = true` but the model file is missing
- **THEN** the system SHALL emit a `tracing::warn`
- **AND** it SHALL proceed without reranking rather than fail to start

### Requirement: Post-retrieval reranking stage
After initial retrieval (vector-only or hybrid), the reranker SHALL reorder the candidate chunks by cross-encoder relevance score.

#### Scenario: Vector search with reranking
- **WHEN** reranker is enabled and a vector search returns 10 candidates
- **THEN** the reranker SHALL score each `(query, chunk_content)` pair
- **AND** results SHALL be reordered by reranker score descending
- **AND** the final output SHALL be truncated to `top_k`

#### Scenario: Hybrid search with reranking
- **WHEN** reranker is enabled and hybrid search returns fused candidates
- **THEN** the reranker SHALL score the fused candidate list
- **AND** final ordering SHALL reflect cross-encoder relevance

### Requirement: Memory budget compliance
The reranker SHALL not violate the idle memory budget when enabled.

#### Scenario: Model size constraint
- **WHEN** the reranker model is loaded
- **THEN** its resident memory SHALL be monitored at startup
- **AND** if the model alone exceeds 150 MB, the system SHALL refuse to load it
- **AND** it SHALL fall back to no-reranking mode

#### Scenario: ONNX batch inference
- **WHEN** reranking a candidate list
- **THEN** inference SHALL use batched forward passes with a configurable batch size (default 8)
- **AND** batching SHALL be used to minimize peak memory usage
