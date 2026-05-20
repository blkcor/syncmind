## ADDED Requirements

### Requirement: Docker Compose self-hosting stack
The system SHALL provide a Docker Compose configuration that brings up the Spine and all dependencies with a single command.

#### Scenario: Start local development environment
- **WHEN** a developer runs `make dev` in `services/sync-gateway/`
- **THEN** Docker Compose starts three services: `spine` (Go app on port 8080), `postgres` (PostgreSQL 17), and `redis` (Redis 7)
- **AND** PostgreSQL data persists via a named volume `spine_pg_data`
- **AND** the `spine` service waits for `postgres` and `redis` health checks before starting
- **AND** the `spine` container is built using a multi-stage Dockerfile based on `golang:1.24-alpine`

#### Scenario: Stop local development environment
- **WHEN** a developer runs `make dev-down` or `docker compose down`
- **THEN** all containers are stopped
- **AND** PostgreSQL data remains in the named volume for subsequent restarts

### Requirement: Database migration system
The system SHALL manage PostgreSQL schema via versioned migrations.

#### Scenario: Apply migrations
- **WHEN** a developer runs `make migrate-up`
- **THEN** the system applies all pending migration scripts from `services/sync-gateway/migrations/`
- **AND** the migration tool records applied versions in a schema migrations table

#### Scenario: Rollback migrations
- **WHEN** a developer runs `make migrate-down`
- **THEN** the system reverts the most recently applied migration

### Requirement: Health check endpoint
The system SHALL expose a health endpoint for load balancers and orchestrators.

#### Scenario: Healthy system response
- **WHEN** a request is sent to `GET /health`
- **AND** the Spine can connect to PostgreSQL and Redis
- **THEN** the system returns HTTP 200 OK with body `{"status":"healthy"}`

#### Scenario: Unhealthy system response
- **WHEN** a request is sent to `GET /health`
- **AND** the Spine cannot connect to PostgreSQL or Redis
- **THEN** the system returns HTTP 503 Service Unavailable with body `{"status":"unhealthy","details":"..."}`

### Requirement: Prometheus metrics exposure
The system SHALL expose operational metrics in Prometheus format.

#### Scenario: Metrics endpoint available
- **WHEN** a request is sent to `GET /metrics`
- **THEN** the system returns Prometheus-formatted metrics including at minimum:
  - `spine_sync_bundles_total` (Counter, labeled by `from_device_type`, `to_device_type`)
  - `spine_sync_bundle_size_bytes` (Histogram)
  - `spine_active_websockets` (Gauge)
  - `spine_pairing_sessions_total` (Counter, labeled by `status`)
  - `spine_http_requests_total` (Counter, labeled by `method`, `path`, `status`)

### Requirement: Structured logging
The system SHALL emit structured logs with trace correlation.

#### Scenario: Request log with trace ID
- **WHEN** any HTTP or WebSocket request is processed
- **THEN** the system emits a log entry containing at minimum: `timestamp`, `level`, `trace_id`, `device_id`, `method`, `path`, `status`, `duration_ms`
- **AND** sensitive fields (e.g., `encrypted_payload`, JWT tokens) are redacted from log output

### Requirement: Makefile build commands
The system SHALL provide standard Makefile targets for local development.

#### Scenario: Build binary
- **WHEN** a developer runs `make build`
- **THEN** the system compiles the Go binary to `bin/spine`

#### Scenario: Run tests
- **WHEN** a developer runs `make test`
- **THEN** the system runs all unit tests and integration tests
- **AND** the command exits with code 0 if all tests pass

#### Scenario: Lint and vet
- **WHEN** a developer runs `make lint`
- **THEN** the system runs `go vet` and `gofmt -d` on all packages
- **AND** the command exits with code 0 if no issues are found

### Requirement: Resource target
The system SHALL remain within stated resource limits under defined load.

#### Scenario: Idle memory usage
- **WHEN** the Spine process is running with zero active WebSocket connections and zero pending bundles
- **THEN** the process RSS memory is less than 200MB

#### Scenario: Load memory usage
- **WHEN** the Spine process is serving 10,000 concurrent WebSocket connections
- **THEN** the process RSS memory is less than 1GB
