# search-rpc-rate-limiting Specification Delta

## Purpose
Rate limiting for mobile-initiated `search-request` bundles, preventing individual peer devices from exceeding 30 queries per minute.

## ADDED Requirements

### Requirement: Per-peer sliding window rate limiter
The system SHALL enforce a rate limit of 30 search-request bundles per minute, keyed by peer fingerprint.

#### Scenario: Under rate limit — request proceeds
- **WHEN** a `search-request` bundle arrives from peer `A`
- **AND** peer `A` has issued < 30 requests in the last 60 seconds
- **THEN** the system processes the search request normally
- **AND** the system records the current timestamp for peer `A`

#### Scenario: Rate limit exceeded — error response
- **WHEN** a `search-request` bundle arrives from peer `A`
- **AND** peer `A` has already issued >= 30 requests in the last 60 seconds
- **THEN** the system does NOT execute the search query
- **AND** the system constructs an error payload:
  ```json
  {
    "v": 1,
    "kind": "error",
    "request_id": "<original request_id>",
    "error_code": "RATE_LIMITED",
    "error_message": "Search rate limit exceeded: 30 requests/minute per device. Try again later.",
    "retry_after_seconds": 30,
    "server_ts": "<RFC3339 UTC>"
  }
  ```
- **AND** the system encrypts the error payload as a standard bundle envelope with `kind: "search-response"`
- **AND** the system uploads the encrypted error bundle to the Spine for the requesting peer
- **AND** the system logs the rate-limit event

#### Scenario: Rate limit resets after window
- **WHEN** peer `A` sends request #31 at `T+60s`
- **AND** the 60-second sliding window contains only requests from `T+0s` to `T+60s`
- **AND** the oldest request in the window has timestamp < `T+0s` (i.e., has fallen out)
- **THEN** the window count drops to <= 30
- **AND** the system processes the request normally

### Requirement: Rate limiter implementation
The system SHALL implement the rate limiter as an in-memory sliding window, per-peer, with automatic cleanup of expired entries.

#### Scenario: Expired entries are cleaned up
- **WHEN** a peer has not sent any requests for > 60 seconds
- **THEN** the system removes that peer's entry from the rate limiter state on the next request (lazy expiration)

#### Scenario: Rate limiter state is not persisted
- **WHEN** the desktop process restarts
- **THEN** the rate limiter state is reset (all peers start at 0)
- **AND** this is acceptable because a restart resets the abuse surface

### Requirement: Rate-limiter configuration
The system SHALL define the rate limit as a configurable constant in the `ratelimit` module.

#### Scenario: Default rate limit values
- **WHEN** the system starts
- **THEN** the default limit is 30 requests per 60-second window
- **AND** both `MAX_REQUESTS` and `WINDOW_SECONDS` are defined as `const` in `ratelimit.rs`
