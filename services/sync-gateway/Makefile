.PHONY: build test lint migrate-up migrate-down dev dev-down

BIN := bin/spine
GO := go
GOOSE := $(GO) run github.com/pressly/goose/v3/cmd/goose@latest

build:
	mkdir -p bin
	$(GO) build -o $(BIN) ./cmd/spine

test:
	$(GO) test -v ./...

lint:
	$(GO) vet ./...
	@test -z "$$(gofmt -d .)" || (echo "gofmt issues found"; exit 1)

migrate-up:
	$(GOOSE) -dir migrations postgres "$(DATABASE_URL)" up

migrate-down:
	$(GOOSE) -dir migrations postgres "$(DATABASE_URL)" down

dev:
	docker compose up -d --build

dev-down:
	docker compose down
