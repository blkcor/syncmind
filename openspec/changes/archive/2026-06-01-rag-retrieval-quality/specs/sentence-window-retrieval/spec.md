# sentence-window-retrieval

Search results automatically include adjacent chunks from the same source file to reconstruct complete logical units.

## ADDED Requirements

### Requirement: Adjacent Chunk Fetch

After retrieving top-K results via vector or hybrid search, the store SHALL fetch up to 2 preceding and 2 following chunks (`chunk_index ± 2`) from the same file for each result chunk. Adjacent chunks SHALL be merged in `chunk_index` order to produce a single, coherent display text per result.

#### Scenario: Matched chunk expands to neighboring chunks
- **WHEN** a search result matches chunk index 5 in a file with chunks 0 through 9
- **THEN** sentence window retrieval SHALL fetch chunks 3, 4, 5, 6, and 7 from the same file
- **AND** the result's display text SHALL concatenate those chunks in ascending `chunk_index` order

#### Scenario: File boundaries limit adjacent fetches
- **WHEN** a search result matches chunk index 1 in a file with chunks 0 through 3
- **THEN** sentence window retrieval SHALL fetch only chunks 0, 1, 2, and 3
- **AND** it SHALL NOT fail because chunk index -1 or chunk index 4 does not exist

#### Scenario: Window zero bypasses expansion
- **WHEN** sentence window retrieval is invoked with `window = 0`
- **THEN** the result's `display_content` SHALL equal the matched chunk's `content`
- **AND** no adjacent chunks SHALL be fetched

### Requirement: Deduplication

When a single result's window would include the same chunk more than once, the merged display text SHALL deduplicate by chunk identifier so the same chunk does not appear twice within that result. Separate top-K hits SHALL remain separate search results even when their adjacent windows overlap.

#### Scenario: Overlapping fetches within a result are unique
- **WHEN** the adjacent chunk query returns duplicate rows for the same `chunk_id`
- **THEN** display assembly SHALL include that chunk's content only once

#### Scenario: Overlapping top-K hits remain separate
- **WHEN** top-K contains result A at chunk index 3 and result B at chunk index 5 from the same file
- **AND** both windows include chunk index 4
- **THEN** result A and result B SHALL remain separate returned results
- **AND** each result SHALL retain its original matched `chunk_id`, score, and metadata

### Requirement: Display Text Assembly

The merged display text SHALL be stored in a new `display_content` field on `SearchResult`. The original `content` field SHALL remain the matched chunk's own text for backward compatibility with existing consumers (pinning, tags). The MCP `search_knowledge` tool SHALL return `display_content` as the primary content visible to AI consumers.

#### Scenario: Display content differs from matched content
- **WHEN** a search result has adjacent chunks available
- **THEN** `SearchResult.content` SHALL contain only the matched chunk text
- **AND** `SearchResult.display_content` SHALL contain the merged adjacent-window text

#### Scenario: Pinning uses the original matched chunk
- **WHEN** a user pins or unpins a search result with expanded `display_content`
- **THEN** the operation SHALL use the result's original matched `chunk_id`
- **AND** sentence window expansion SHALL NOT create or pin synthetic chunks
