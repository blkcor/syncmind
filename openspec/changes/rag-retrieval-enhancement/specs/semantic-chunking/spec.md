## ADDED Requirements

### Requirement: Extended language support for code chunking
The chunker SHALL support Go source files using tree-sitter AST parsing in addition to the existing Rust, Python, and JavaScript/TypeScript support.

#### Scenario: Go function extraction
- **WHEN** a `.go` file is indexed
- **THEN** `CodeChunker` SHALL parse the file with `tree-sitter-go`
- **AND** extract chunks at `function_declaration`, `method_declaration`, `type_spec`, and `struct_type` boundaries
- **AND** each chunk SHALL contain a complete syntactic unit

#### Scenario: Unsupported language graceful fallback
- **WHEN** a file extension is not mapped to any tree-sitter grammar
- **THEN** `CodeChunker` SHALL delegate to `FallbackChunker`
- **AND** indexing SHALL continue without error

### Requirement: Semantic sub-chunking for oversized code blocks
When an AST node (e.g., a function) exceeds the configured `chunk_size`, the chunker SHALL split it at logical boundaries rather than raw character count.

#### Scenario: Large Go function split at logical boundaries
- **WHEN** a Go function body exceeds `chunk_size`
- **THEN** the chunker SHALL attempt to split at blank-line boundaries first
- **AND** if blank lines are insufficient, split at comment-block boundaries
- **AND** only if neither exists, fall back to `FallbackChunker` line-based splitting
- **AND** every sub-chunk SHALL preserve the parent function signature as a prefix so semantic context is not lost

#### Scenario: Large Rust function split at logical boundaries
- **WHEN** a Rust function body exceeds `chunk_size`
- **THEN** the same logical-boundary strategy (blank lines → comments → fallback) SHALL apply
- **AND** sub-chunks SHALL retain function signature context

### Requirement: Chunk metadata integrity
All chunks produced by any chunker SHALL retain accurate line numbers and unique indices even after sub-chunking.

#### Scenario: Sub-chunk line number accuracy
- **WHEN** an oversized function is split into three sub-chunks
- **THEN** each sub-chunk SHALL report `start_line` and `end_line` relative to the original file
- **AND** `chunk_index` SHALL be monotonically increasing across the file
