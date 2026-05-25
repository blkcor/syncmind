ARG GO_IMAGE=golang:1.25-alpine
ARG RUNTIME_IMAGE=alpine:latest

FROM ${GO_IMAGE} AS builder

WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN go build -o spine ./cmd/spine

FROM ${RUNTIME_IMAGE}
RUN apk --no-cache add ca-certificates
WORKDIR /root/
COPY --from=builder /app/spine .
COPY --from=builder /app/spine.yaml .
CMD ["./spine"]
