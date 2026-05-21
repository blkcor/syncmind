## ADDED Requirements

### Requirement: Full-text search index via SQLite FTS5
The storage layer SHALL maintain an FTS5 virtual table that indexes chunk content for lexical (keyword) search.

#### Scenario: FTS5 table creation on init
- **WHEN** `VectorStore::new` is called
- **THEN** it SHALL execute `CREATE VIRTUAL TABLE IF NOT EXISTS fts_chunks USING fts5(content, content_rowid=chunk_id)`
- **AND** it SHALL upsert rows into `fts_chunks` whenever chunks are upserted into the `chunks` table

#### Scenario: FTS5 cleanup on file deletion
- **WHEN** a file is deleted from the index
- **THEN** all associated rows in `fts_chunks` SHALL be removed

### Requirement: Hybrid retrieval combining BM25 and vector similarity
`VectorStore::search` SHALL support a hybrid mode that fetches candidates from both FTS5 (BM25) and sqlite-vec (vector), then fuses the rankings.

#### Scenario: Hybrid search with RRF fusion
- **WHEN** `search` is invoked with `hybrid = true`
- **THEN** it SHALL execute an FTS5 query with the raw query text to retrieve the top `k * 2` BM25 candidates
- **AND** it SHALL execute a vector similarity search with the query embedding to retrieve the top `k * 2` vector candidates
- **AND** it SHALL compute Reciprocal Rank Fusion (RRF) scores: `score = Σ 1 / (k + rank)` with `k = 60`
- **AND** it SHALL return the top `k` results ordered by fused RRF score descending

#### Scenario: Pure vector search backward compatibility
- **WHEN** `search` is invoked with `hybrid = false` (default)
- **THEN** it SHALL behave exactly as before, performing only vector similarity search

### Requirement: Configurable relevance threshold
The system SHALL filter out search results whose relevance score falls below a configured threshold.

#### Scenario: Vector distance threshold
- **WHEN** `search` is called with a `threshold` parameter
- **THEN** results with vector distance greater than `threshold` SHALL be discarded
- **AND** fewer than `top_k` results MAY be returned if the threshold eliminates low-quality matches

#### Scenario: Empty result set for irrelevant queries
- **WHEN** a query has no semantically or lexically similar chunks
- **THEN** the system SHALL return an empty result array
- **AND** it SHALL NOT pad the result list to reach `top_k`

### Requirement: MCP tool parameter exposure
The `search_knowledge` MCP tool SHALL expose new optional parameters for hybrid search and threshold control.

#### Scenario: Hybrid toggle via MCP
- **WHEN** `search_knowledge` is called with `"hybrid": true`
- **THEN** the backend SHALL execute hybrid search
- **AND** when omitted, `hybrid` SHALL default to the value in `config.toml`

#### Scenario: Threshold override via MCP
- **WHEN** `search_knowledge` is called with `"threshold": 0.75`
- **THEN** results below that threshold SHALL be discarded
- **AND** when omitted, the global config threshold SHALL apply
