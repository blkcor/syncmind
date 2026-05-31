# sentence-window-retrieval

Search results automatically include adjacent chunks from the same source file to reconstruct complete logical units.

## ADDED Requirements

### Requirement: Adjacent Chunk Fetch

After retrieving top-K results via vector or hybrid search, the store SHALL fetch up to 2 preceding and 2 following chunks (`chunk_index ± 2`) from the same file for each result chunk. Adjacent chunks SHALL be merged in `chunk_index` order to produce a single, coherent display text per result.

### Requirement: Deduplication

When two top-K results share adjacent chunks (e.g., result A at chunk_index 3 and result B at chunk_index 5 both pull chunk 4), the merged display text SHALL deduplicate overlapping content so the same chunk does not appear twice.

### Requirement: Display Text Assembly

The merged display text SHALL be stored in a new `display_content` field on `SearchResult`. The original `content` field SHALL remain the matched chunk's own text for backward compatibility with existing consumers (pinning, tags). The MCP `search_knowledge` tool SHALL return `display_content` as the primary content visible to AI consumers.

## Acceptance Criteria

- Searching for a topic that spans multiple consecutive chunks returns a merged result showing the full context.
- A single-file result with chunk_index 5 pulls chunks 3-7 (when they exist) for display.
- Two overlapping results from the same file don't show duplicated display text.
- Pinned chunks remain pinned by their original `chunk_id`; sentence window expansion doesn't affect pin/unpin behavior.
