## Context

Phase 1 was archived on 2026-05-20 with a clean `cargo test`/`cargo clippy` baseline, but `docs/prd/001-headless-mcp-core.md` acceptance criteria audit revealed:

- US-005 line 77 mandates auto-download; the archive's design.md explicitly punted this as a non-goal with the mitigation "document manual download step." That mitigation was never followed through in `docs/`, so users hit a hard `Failed to load ONNX model` error on first run without Ollama.
- PRD line 177 mandates log rotation under `~/.local/share/syncmind/logs/`. The daemon currently initializes `tracing_subscriber::fmt::init()` only when `--foreground` is set, leaving background launches silent.
- PRD line 39 mandates delete/rename handling. The `file-watcher` filters out non-existent paths via `if path.is_file()` on `core/file-watcher/src/lib.rs:63`, so delete events never reach the indexing pipeline. The `VectorStore` API also lacks any delete-by-path entry point.
- US-009 mandates a Claude Code config example, real-world E2E verification, and a Phase 1 README. The existing `docs/examples/claude_desktop_config.json` is a minimal stub; no Claude Code CLI example exists; no root README exists; the only E2E script (`scripts/e2e-mcp-test.sh`) covers JSON-RPC protocol calls but no re-index or delete flows.

## Goals / Non-Goals

**Goals:**
- Close all four PRD gaps such that `docs/prd/001-headless-mcp-core.md` reads green when audited against the implementation.
- Preserve the privacy directive: no new network calls beyond the configurable Hugging Face mirror.
- Preserve the < 100 MB idle memory budget: dependency additions (`tracing-appender`, `fs2`) are small (< 100 KB combined).
- Keep all changes in one OpenSpec change so the PR is auditable as a single Phase 1 closure.

**Non-Goals:**
- Recursive directory watching (still deferred to a later phase per PRD NG-2).
- Image OCR (per PRD NG-3).
- Hot-reload of `log_level` (requires runtime subscriber rebuild; document daemon restart as the workaround).
- ONNX download progress UI / resume / checksum verification (single-shot download; future enhancement).
- Migration of pre-existing indexed files for orphaned paths discovered after the upgrade — `delete_file_by_path` is invoked on new events only.

## Decisions

### 1. Hardcoded Hugging Face default + Config override

Default URLs:
- Model: `https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx`
- Tokenizer: `https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json`

**Rationale:** Zero-config first-run for the majority case, while `Config::onnx_model_url` / `onnx_tokenizer_url` lets users in network-restricted regions (e.g., mainland China) point to an in-country mirror. Selected over "environment variable only" because daemon restarts to pick up env changes are awkward, and over "Config required" because it punishes new users.

**Trade-off:** Hardcoded HTTPS URLs in source require a code change to rotate. Mitigated by Config override.

### 2. Watcher API break to `Vec<FileEvent>`

**Rationale:** The current `Vec<PathBuf>` channel cannot express "this file was deleted." Downstream filtering on `path.exists()` is race-prone (TOCTOU: `rm` followed by same-name `touch` would be misread as modify). An explicit `FileEvent::Upsert | Remove` enum makes the semantic intent unambiguous.

**Affected consumers (all in-repo, no external SDK users):**
- `core/syncmind/src/main.rs:97`
- `apps/desktop/src-tauri/src/lib.rs:257`

Both are updated in the same PR.

**Alternatives considered:**
- *Keep `Vec<PathBuf>`, check `exists()` downstream* → loses rename semantics, TOCTOU risk.
- *Dual channels (legacy `Vec<PathBuf>` + new `Vec<FileEvent>`)* → doubles maintenance with no benefit since both consumers are internal.

### 3. `tracing-appender::rolling::daily` over manual rotation

**Rationale:** Bundled with the `tracing` ecosystem we already use; zero external runtime. `non_blocking` writer avoids stalling the async runtime during disk writes; `WorkerGuard` is preserved on the daemon's main stack frame so logs flush on shutdown.

**Stdio MCP safety:** stdout (JSON-RPC) and stderr / file appender are independent file descriptors. Foreground mode adds a stderr layer; Stdio mode does NOT — this matches `core/mcp-server/src/stdio.rs:11`'s contract.

### 4. `fs2` file lock for download race

**Rationale:** Two simultaneous daemon launches (CLI + Desktop) racing on first-run download would corrupt the partial file. `fs2::FileExt::try_lock_exclusive` on a sidecar `.lock` file is the smallest dependency that solves this correctly. The non-holder polls for completion via `tokio::time::sleep` + file existence check.

**Trade-off:** `fs2` is unmaintained-ish (last release 2018) but tiny and dependency-free. If we hit issues, swap for `file-lock` 2.x. Not worth solving today.

### 5. `delete_file_by_path` mirrors `upsert_file`'s vec_chunks pattern

`vec_chunks` is a sqlite-vec virtual table; foreign-key cascade does NOT propagate to it. The existing `upsert_file` at `core/storage/src/store.rs:122-128` already works around this by manually `DELETE FROM vec_chunks WHERE chunk_id = ?` before deleting from `chunks`/`files`. `delete_file_by_path` uses the identical pattern inside one transaction.

## Risks / Trade-offs

**[Risk] Hugging Face rate-limiting on first run for many users at once.**
→ Mitigation: documentation calls out the optional mirror config; the lock file prevents single-host re-download storms; failed downloads emit a `tracing::error` with the configured URL so users can substitute a mirror without source edits.

**[Risk] Desktop app build breaks if watcher API migration is incomplete.**
→ Mitigation: verification checklist includes `pnpm --filter desktop tauri build` (or at minimum `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml`). Same-PR change keeps the migration atomic.

**[Risk] Rolling file appender silently fails if `~/.local/share/syncmind/logs/` is not writable (e.g., read-only home, sandboxed environment).**
→ Mitigation: `init_tracing` creates the directory with `create_dir_all`; on failure, log the error to stderr (always available) and fall back to stderr-only logging. Test path covers this via `tempfile::tempdir()` redirection.

**[Risk] `delete_file_by_path` invoked on a file that was deleted from disk but is still in `registered_files` (e.g., user unregisters AFTER deleting).**
→ Behavior: idempotent — returns `Ok(false)` when no row matches. Tested explicitly.

**[Risk] Hot-reload of `log_level` is not supported and may confuse users who edit config.toml expecting immediate effect.**
→ Mitigation: README and config.toml comments call out "restart daemon to apply log level changes." Future enhancement: rebuild subscriber on config reload.
