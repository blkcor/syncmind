# css-chunker Specification

## Purpose
CSS/SCSS/Less files are chunked by rule boundaries instead of blind character-count splitting, with selector context preservation for oversized rules.

## Requirements

### Requirement: CSS Rule Boundary Chunking

CSS files (`.css`, `.scss`, `.less`) SHALL be chunked by complete rule boundaries defined by `}`. Each chunk SHALL contain one or more complete CSS rules up to the configured `chunk_size`.

When a single rule exceeds `chunk_size` (e.g., large `@keyframes` blocks), the rule SHALL be sub-chunked by property declaration boundaries. Every sub-chunk MUST be prefixed with the full selector header of the parent rule so downstream embedding retains semantic context.

#### Scenario: CSS rules split at rule boundaries
- **WHEN** a `.css` file contains 10 complete rules and each rule is under `chunk_size`
- **THEN** `CssChunker` SHALL emit one chunk per rule
- **AND** each chunk SHALL include the complete selector, opening brace, declarations, and closing brace for that rule

#### Scenario: Oversized CSS rule split at declaration boundaries
- **WHEN** a single CSS rule exceeds `chunk_size`
- **THEN** `CssChunker` SHALL split the rule at declaration boundaries
- **AND** each sub-chunk SHALL include the parent selector context
- **AND** each sub-chunk SHALL retain accurate source line numbers and a monotonically increasing `chunk_index`

#### Scenario: Empty CSS input
- **WHEN** a `.css` file contains no non-whitespace content
- **THEN** `CssChunker` SHALL emit zero chunks

### Requirement: Selector Context Preservation

The selector header (everything before the opening `{`) of a CSS rule SHALL be preserved for embedding purposes. For sub-chunked oversized rules, the selector SHALL be prepended as a CSS comment (`/* context: .selector */`) to each sub-chunk so the embedding model understands what the properties belong to.

#### Scenario: Oversized rule keeps selector context
- **WHEN** a `.css` rule with selector `.fabric-card:hover` is split into multiple sub-chunks
- **THEN** every sub-chunk SHALL start with a CSS context comment containing `.fabric-card:hover`

#### Scenario: Nested SCSS rule remains a parent rule
- **WHEN** a `.scss` or `.less` file contains a nested rule
- **THEN** `CssChunker` SHALL count brace depth and treat the outer rule as the rule boundary
- **AND** the nested rule content SHALL remain inside that parent rule chunk unless the parent rule exceeds `chunk_size`

### Requirement: File Extension Routing

Files with extensions `.css`, `.scss`, `.less` SHALL be routed to `CssChunker` via `chunker_for_path`. Files with unrecognized extensions SHALL continue to use the enhanced `FallbackChunker`.

#### Scenario: Stylesheet extensions use CssChunker
- **WHEN** `chunker_for_path` receives a path ending in `.css`, `.scss`, or `.less`
- **THEN** it SHALL return `CssChunker`

#### Scenario: Unknown extensions keep fallback behavior
- **WHEN** `chunker_for_path` receives a path with an unrecognized extension
- **THEN** it SHALL return the enhanced `FallbackChunker`
