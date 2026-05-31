# css-chunker

CSS/SCSS/Less files are chunked by rule boundaries instead of blind character-count splitting.

## ADDED Requirements

### Requirement: CSS Rule Boundary Chunking

CSS files (`.css`, `.scss`, `.less`) SHALL be chunked by complete rule boundaries defined by `}`. Each chunk SHALL contain one or more complete CSS rules up to the configured `chunk_size`.

When a single rule exceeds `chunk_size` (e.g., large `@keyframes` blocks), the rule SHALL be sub-chunked by property declaration boundaries. Every sub-chunk MUST be prefixed with the full selector header of the parent rule so downstream embedding retains semantic context.

### Requirement: Selector Context Preservation

The selector header (everything before the opening `{`) of a CSS rule SHALL be preserved for embedding purposes. For sub-chunked oversized rules, the selector SHALL be prepended as a CSS comment (`/* context: .selector */`) to each sub-chunk so the embedding model understands what the properties belong to.

### Requirement: File Extension Routing

Files with extensions `.css`, `.scss`, `.less` SHALL be routed to `CssChunker` via `chunker_for_path`. Files with unrecognized extensions SHALL continue to use the enhanced `FallbackChunker`.

## Acceptance Criteria

- A CSS file containing 10 rules under 512 chars each produces exactly 10 chunks.
- A CSS rule exceeding `chunk_size` is split into sub-chunks, each starting with the selector context comment.
- SCSS nested rules are treated as single rules (split by outer `}`).
- An empty CSS file produces zero chunks.
