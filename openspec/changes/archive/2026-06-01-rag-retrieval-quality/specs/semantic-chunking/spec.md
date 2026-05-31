# semantic-chunking

Semantic code chunking keeps displayed chunk content pristine while retaining parent signature context for embedding and FTS indexing.

## MODIFIED Requirements

### Requirement: Semantic sub-chunking for oversized code blocks

When an AST node (e.g., a function) exceeds the configured `chunk_size`, the chunker SHALL split it at logical boundaries rather than raw character count.

Oversized code sub-chunks SHALL preserve parent function/class signature context through chunk metadata rather than by hard-concatenating that signature into `Chunk.content`. The metadata field SHALL be available to the indexing pipeline as `context_prefix: Option<String>`. The indexing pipeline SHALL prepend `context_prefix` to the text sent to embedding generation and FTS indexing, while storing the original `Chunk.content` for display and pinning.

#### Scenario: Large Go function split at logical boundaries
- **WHEN** a Go function body exceeds `chunk_size`
- **THEN** the chunker SHALL attempt to split at blank-line boundaries first
- **AND** if blank lines are insufficient, split at comment-block boundaries
- **AND** only if neither exists, fall back to `FallbackChunker` line-based splitting
- **AND** every sub-chunk SHALL set parent function signature context in `context_prefix`
- **AND** the parent function signature SHALL NOT be prepended directly to `Chunk.content`

#### Scenario: Large Rust function split at logical boundaries
- **WHEN** a Rust function body exceeds `chunk_size`
- **THEN** the same logical-boundary strategy (blank lines → comments → fallback) SHALL apply
- **AND** every sub-chunk SHALL set parent function signature context in `context_prefix`
- **AND** the parent function signature SHALL NOT be prepended directly to `Chunk.content`

#### Scenario: Embedding text receives context prefix
- **WHEN** the indexing pipeline embeds a chunk with `context_prefix = Some(prefix)`
- **THEN** the text passed to the embedder SHALL be `prefix + "\n" + chunk.content`
- **AND** the stored chunk row SHALL keep `chunk.content` without the prefix

#### Scenario: FTS text receives context prefix
- **WHEN** the indexing pipeline writes a chunk with `context_prefix = Some(prefix)` into the FTS index
- **THEN** the indexed FTS content SHALL include the prefix
- **AND** keyword search SHALL be able to match terms from the parent signature
