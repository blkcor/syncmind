# Spine Quick Start

This guide gets the Spine sync gateway running locally with Docker Compose in under 5 minutes.

## Prerequisites

- Docker & Docker Compose
- Go 1.25+ (for local development)

## Start the Stack

```bash
cd services/sync-gateway
make dev
```

If Docker Hub is unstable in your region, override the images before `make dev`:

```bash
export POSTGRES_IMAGE=docker.m.daocloud.io/library/postgres:17-alpine
export REDIS_IMAGE=docker.m.daocloud.io/library/redis:7-alpine
export GO_IMAGE=docker.m.daocloud.io/library/golang:1.25-alpine
export RUNTIME_IMAGE=docker.m.daocloud.io/library/alpine:latest
make dev
```

The compose file keeps Docker Hub defaults, so this only affects your local shell.

This starts three services:

| Service    | Address                     | Purpose               |
| ---------- | --------------------------- | --------------------- |
| spine      | http://localhost:8080       | Sync gateway API      |
| postgres   | postgres://localhost:5432   | Persistence           |
| redis      | internal only               | Pub/Sub & rate limits |

## Verify Health

```bash
curl http://localhost:8080/health
```

Expected response:

```json
{
  "status": "healthy",
  "timestamp": "2026-05-14T12:00:00Z",
  "postgres": "ok",
  "redis": "ok"
}
```

## View Metrics

```bash
curl http://localhost:8080/metrics
```

## Run Migrations

```bash
make migrate-up
```

## Stop the Stack

```bash
make dev-down
```

## Configuration

Copy `spine.yaml` and adjust:

```yaml
database_url: "postgres://postgres:postgres@localhost:5432/syncmind?sslmode=disable"
redis_addr: "localhost:6379"
bind_addr: ":8080"
jwt_issuer: "syncmind-device"
jwt_audience: "syncmind-spine"
```

## API Overview

| Endpoint                    | Auth | Description                   |
| --------------------------- | ---- | ----------------------------- |
| `POST /v1/pairing/initiate` | No   | Start device pairing          |
| `POST /v1/pairing/complete` | No   | Complete pairing with QR scan |
| `POST /v1/sync/bundle`      | JWT  | Upload encrypted bundle       |
| `GET /v1/sync/bundles`      | JWT  | List pending bundles          |
| `GET /v1/sync/bundles/:id`  | JWT  | Download bundle payload       |
| `DELETE /v1/sync/bundles/:id` | JWT | Acknowledge bundle           |
| `POST /v1/media/upload`     | JWT  | Upload media (JPEG/PNG/HEIC)  |
| `WS /v1/sync/live`          | JWT  | Real-time sync notifications  |
