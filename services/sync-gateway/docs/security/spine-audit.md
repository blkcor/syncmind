# Spine E2EE Security Audit Checklist

This document tracks the security posture of the Spine sync gateway. Spine is designed as a **blind relay** — it never holds decryption keys and cannot inspect bundle contents.

## Architecture Guarantees

| # | Requirement | Status |
|---|---|---|
| 1 | Server never stores private keys | ✅ Devices generate and hold Ed25519/X25519 keys locally |
| 2 | Server cannot decrypt payloads | ✅ `encrypted_payload` is stored as opaque `BYTEA` |
| 3 | No decryption logic in codebase | ✅ Verified: zero matches for `decrypt`, `Decrypt`, `unencrypt`, `Unencrypt` |
| 4 | Encrypted payload never logged | ✅ Verified: `encrypted_payload` only appears in DB read/write paths |
| 5 | No image/audio parsing libraries | ✅ Verified: only MIME type string whitelisting, no content inspection |

## Cryptographic Primitives

| # | Primitive | Usage | Implementation |
|---|---|---|---|
| 1 | Ed25519 | Device JWT signing | `crypto/ed25519` (Go standard library) |
| 2 | X25519 | Device pairing ECDH | Stored as raw bytes; exchange happens client-side |
| 3 | SHA-256 | Bundle integrity | `crypto/sha256` (Go standard library) |
| 4 | AES-256-GCM | Payload encryption | Client-side only; server stores ciphertext |

## Authentication

| # | Check | Status |
|---|---|---|
| 1 | JWTs are Ed25519-signed | ✅ `jwt.SigningMethodEd25519` enforced |
| 2 | Public keys loaded from DB per request | ✅ Auth middleware queries `devices` table |
| 3 | `iss` and `aud` claims validated | ✅ `jwt.WithIssuer` and `jwt.WithAudience` used |
| 4 | `jti` blacklist checked via Redis | ✅ Prevents token replay |
| 5 | Deactivated devices rejected | ✅ `is_active` enforced in auth middleware |

## Transport Security

| # | Check | Status |
|---|---|---|
| 1 | Security headers on all responses | ✅ HSTS, X-Content-Type-Options, X-Frame-Options, Referrer-Policy |
| 2 | TLS 1.3 in production | ⚠️ Enforced via reverse proxy (nginx/traefik); configure `tls_cert`/`tls_key` in `spine.yaml` |
| 3 | WebSocket origin check | ✅ `CheckOrigin` currently allows all for local dev; restrict in production |

## Rate Limiting

| # | Check | Status |
|---|---|---|
| 1 | Pairing rate limited globally | ✅ 20 requests/minute via Redis sliding window |
| 2 | Sync upload rate limited per device | ✅ 100 bundles/minute per device |

## Data Retention

| # | Check | Status |
|---|---|---|
| 1 | Bundles expire after 30 days | ✅ `expires_at` set on creation |
| 2 | Acked bundles deleted after 7 days | ✅ Cleanup scheduler hard-deletes stale records |
| 3 | Pairing sessions expire after 5 minutes | ✅ `expires_at` set on creation |

## Audit Commands

Run these commands before each release:

```bash
# Verify no decryption logic
grep -r "decrypt\|Decrypt\|unencrypt\|Unencrypt" services/sync-gateway/

# Verify encrypted_payload scope
grep -r "encrypted_payload" services/sync-gateway/

# Verify no image/audio parsing libraries
grep -r "image/\|audio/\|heic\|m4a" services/sync-gateway/internal/handler/

# Run static analysis
gosec ./...

# Run race detector
go test -race ./...
```
