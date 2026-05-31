# score-threshold

A configurable relevance threshold filters low-quality retrieval results, with correct application in hybrid search.

## ADDED Requirements

### Requirement: Default Threshold

`Config::relevance_threshold` SHALL default to `Some(0.4)`. This means results with similarity scores below 0.4 are discarded before being returned to the caller.

### Requirement: Hybrid Search Threshold Position

In `search_hybrid`, the threshold filter SHALL be applied to the normalized RRF-fused scores, NOT to the raw L2 distance scores. RRF scores are normalized to [0, 1] by dividing by the maximum score in the result set, then threshold-filtered against `relevance_threshold`.

### Requirement: Threshold Bypass

When the caller explicitly passes `threshold: 0` or `threshold: null` in the MCP tool arguments, the threshold filter SHALL be bypassed, returning all results regardless of score. This allows users to opt out of filtering.

### Requirement: MCP Tool Exposure

The `search_knowledge` MCP tool SHALL accept an optional `threshold` parameter (float, 0.0-1.0) that overrides the config default. When the parameter is omitted, the config default SHALL be used.

## Acceptance Criteria

- With `relevance_threshold = 0.4` and a query for "fabric", no CSS/Svelte chunks with low semantic relevance are returned.
- Hybrid search with `relevance_threshold = 0.4` filters AFTER RRF fusion, not before.
- Passing `threshold: 0` in the MCP request returns all results (bypass).
- Backward compatible: config files without `relevance_threshold` get the new 0.4 default.
