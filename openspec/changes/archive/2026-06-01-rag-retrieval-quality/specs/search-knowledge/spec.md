# search-knowledge

The MCP `search_knowledge` text response shows expanded display content while preserving original matched chunk identity internally.

## ADDED Requirements

### Requirement: Expanded Search Result Display

The `search_knowledge` MCP tool SHALL format its returned text using `SearchResult.display_content` as the primary visible result content when sentence-window retrieval has populated it. `SearchResult.content` SHALL remain the matched chunk's original text inside the backend `SearchResult` value so reranking, pinning, and other internal consumers can continue to use the original matched chunk identity.

#### Scenario: MCP response prefers display content
- **WHEN** `search_knowledge` returns a result with non-empty `display_content`
- **THEN** the visible content returned to AI consumers SHALL be `display_content`
- **AND** the backend `SearchResult.content` SHALL still contain the original matched chunk text

#### Scenario: MCP response falls back to matched content
- **WHEN** `search_knowledge` returns a result without expanded display content
- **THEN** the visible content returned to AI consumers SHALL be the matched chunk `content`

#### Scenario: Existing threshold parameter remains available
- **WHEN** clients inspect the `search_knowledge` input schema
- **THEN** the tool SHALL continue to expose the optional numeric `threshold` parameter
- **AND** no new MCP tool name SHALL be introduced for retrieval-quality behavior
