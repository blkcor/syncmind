# hybrid-search

Hybrid retrieval applies relevance thresholds after score normalization and uses a non-empty default threshold.

## MODIFIED Requirements

### Requirement: Configurable relevance threshold

The system SHALL filter out search results whose normalized relevance score falls below a configured threshold. `Config::relevance_threshold` SHALL default to `Some(0.4)` for newly-created configs and for legacy config files that omit the field.

For pure vector search, the store SHALL convert sqlite-vec L2 distance to a similarity score in `[0, 1]` before threshold filtering. For hybrid search, the store SHALL compute Reciprocal Rank Fusion, normalize fused scores to `[0, 1]` by dividing by the maximum fused score in the result set, and then apply the threshold to the normalized fused score.

Callers MAY bypass threshold filtering by explicitly passing `threshold = 0.0`. Omitted threshold values SHALL use `Config::relevance_threshold`. A JSON `null` threshold is treated the same as omission and SHALL NOT bypass the configured threshold.

#### Scenario: Default threshold filters low-quality results
- **WHEN** a search is executed without an explicit threshold override
- **AND** `Config::relevance_threshold` is not set in the config file
- **THEN** the effective threshold SHALL be `0.4`
- **AND** results with normalized relevance scores below `0.4` SHALL be excluded

#### Scenario: Hybrid threshold applies after RRF normalization
- **WHEN** hybrid search combines vector and FTS5 candidates
- **THEN** the store SHALL compute RRF fused scores
- **AND** normalize those fused scores to `[0, 1]`
- **AND** apply `relevance_threshold` to the normalized fused score
- **AND** it SHALL NOT apply the threshold to raw sqlite-vec L2 distance before fusion

#### Scenario: Threshold zero bypasses filtering
- **WHEN** `search_knowledge` is called with `"threshold": 0.0`
- **THEN** the backend SHALL bypass relevance-threshold filtering
- **AND** it MAY return results below the configured threshold

#### Scenario: Null or omitted threshold uses config
- **WHEN** `search_knowledge` is called without a `threshold` argument
- **OR** `search_knowledge` is called with `"threshold": null`
- **THEN** the backend SHALL use `Config::relevance_threshold`

#### Scenario: Threshold override remains bounded
- **WHEN** `search_knowledge` is called with a numeric threshold between `0.0` and `1.0`
- **THEN** the backend SHALL use that value instead of `Config::relevance_threshold`

#### Scenario: Out-of-range threshold is rejected
- **WHEN** `search_knowledge` is called with a numeric threshold outside `0.0..=1.0`
- **THEN** the MCP handler SHALL reject the request as invalid parameters
- **AND** it SHALL NOT silently fall back to the config threshold
