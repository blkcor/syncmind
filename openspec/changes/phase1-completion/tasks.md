## 1. OpenSpec scaffolding

- [x] 1.1 Create `openspec/changes/phase1-completion/{proposal.md,design.md,tasks.md}`
- [x] 1.2 Create per-capability spec files under `openspec/changes/phase1-completion/specs/`
- [x] 1.3 Get user approval on proposal/design

## 2. Gap 3 — File delete/rename handling (foundation)

- [x] 2.1 Add `pub enum FileEvent { Upsert(PathBuf), Remove(PathBuf) }` to `core/file-watcher/src/lib.rs`
- [x] 2.2 Change channel type from `mpsc::Sender<Vec<PathBuf>>` to `mpsc::Sender<Vec<FileEvent>>`
- [x] 2.3 Remove `if path.is_file()` filter; classify events by `notify::EventKind`
- [x] 2.4 Handle rename modes (Both / From / To) per platform
- [x] 2.5 Update debounce aggregation: HashMap<Path, FileEvent> + reconcile against disk state at flush (covers FSEvents trailing-Modify pattern)
- [x] 2.6 Add `VectorStore::delete_file_by_path(&self, &Path) -> Result<bool>` in `core/storage/src/store.rs`
- [x] 2.7 Implement transactional delete from vec_chunks → chunks (via FK cascade) → files
- [x] 2.8 Update `core/syncmind-indexing/src/lib.rs::run_indexing_pipeline` to accept `Vec<FileEvent>` and route `Remove` to `delete_file_by_path`
- [x] 2.9 Update `core/syncmind/src/main.rs` daemon to consume new channel type
- [x] 2.10 Update `apps/desktop/src-tauri/src/lib.rs` for the new channel signature
- [x] 2.11 Extend `syncmind unregister <path>` to also delete the file's chunks from the index
- [x] 2.12 Unit test: `watcher_emits_remove_event_on_delete` in `core/file-watcher/src/lib.rs`
- [x] 2.13 Unit test: `store_delete_file_by_path_clears_all_artifacts` (asserts vec_chunks count is 0)
- [x] 2.14 Unit test: `store_delete_file_by_path_idempotent_for_unknown`
- [x] 2.15 `cargo check --workspace` and `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml` green

## 3. Gap 2 — Log file rotation

- [x] 3.1 Add `tracing-appender = "0.2"` to `core/Cargo.toml` workspace dependencies
- [x] 3.2 Add `tracing-appender` to `core/syncmind/Cargo.toml` and `core/syncmind-core/Cargo.toml`
- [x] 3.3 Add `log_level: String`, `log_to_file: bool`, `log_rotation: LogRotation` to `core/syncmind-core/src/config.rs::Config` with defaults
- [x] 3.4 Define `LogRotation { Daily, Hourly, Never }` enum with serde rename_all = snake_case
- [x] 3.5 Implement `syncmind_core::init_tracing` (in new `observability` module) returning a `WorkerGuard`
- [x] 3.6 Rewrite `core/syncmind/src/main.rs::run_daemon` to use `init_tracing` (always file + optional stderr)
- [x] 3.7 On log_dir creation failure, fall back to stderr-only with a printed warning
- [x] 3.8 Unit test: tempfile-redirected log directory produces a rotated file
- [x] 3.9 Unit test: `legacy_config_without_log_fields_uses_defaults` validates serde defaults (#[serde(default)])

## 4. Gap 1 — ONNX model auto-download

- [x] 4.1 Add `fs2 = "0.4"` to `core/Cargo.toml` workspace dependencies
- [x] 4.2 Add `fs2` and `futures-util` to `core/rag-engine/Cargo.toml` dependencies
- [x] 4.3 Add `onnx_model_url: Option<String>`, `onnx_tokenizer_url: Option<String>` to `Config`
- [x] 4.4 Define `DEFAULT_ONNX_MODEL_URL`, `DEFAULT_ONNX_TOKENIZER_URL` in `core/rag-engine/src/embedder.rs`
- [x] 4.5 Implement `pub async fn ensure_onnx_assets(model_dir, model_url, tokenizer_url)`
- [x] 4.6 Atomic write: download to `<file>.part`, then `tokio::fs::rename` to final name
- [x] 4.7 Concurrent protection: `fs2::FileExt::try_lock_exclusive` on `<file>.lock`; non-holder polls and bails on existing artifact
- [x] 4.8 Skip download if target file exists with size > 0
- [x] 4.9 Make `OnnxEmbedder::from_config` async; await `ensure_onnx_assets` before opening the ONNX session
- [x] 4.10 Update `AutoEmbedder::new` call site to `.await` the new `from_config`
- [x] 4.11 Unit test: mock HTTP server (axum on TcpListener) serves a fake model + tokenizer; files appear at expected paths and second call is a no-op
- [x] 4.12 Unit test: `ensure_onnx_assets_propagates_http_404` covers 404 path with descriptive error
- [x] 4.13 `cargo test -p syncmind-rag-engine` green (23 passed, 2 ignored)

## 5. Gap 4 — US-009 documentation & real-world E2E

- [x] 5.1 Create root `README.md` with Phase 1 status table, 30-second quickstart, configuration table
- [x] 5.2 Create `docs/examples/claude_code_mcp.json` matching `claude mcp add-json` payload
- [x] 5.3 Enrich `docs/examples/claude_desktop_config.json` with absolute path comment + `RUST_LOG` example
- [x] 5.4 Create `docs/examples/quickstart.md` with full install → register → Claude Code walkthrough + troubleshooting
- [x] 5.5 Update `docs/prd/001-headless-mcp-core.md` Open Questions: Q3 ONNX distribution marked resolved
- [x] 5.6 Create `scripts/e2e-phase1-realworld.sh` extending the protocol-level test
- [x] 5.7 E2E covers: register → search → modify file → wait debounce → search reflects new content
- [x] 5.8 E2E covers: delete file → wait debounce → search excludes deleted file
- [x] 5.9 E2E auto-detects Ollama and adapts (`bge-m3` if available, else `bge-small` 384-dim path)

## 6. Verification & archive

- [x] 6.1 `cargo test --workspace` — 42 passed, 2 ignored (Ollama / ONNX live tests)
- [x] 6.2 `cargo clippy --workspace --all-targets -- -D warnings` — clean
- [x] 6.3 `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml` — clean
- [x] 6.4 `bash scripts/e2e-phase1-realworld.sh` — all 4 scenarios green
- [x] 6.5 Added `SYNCMIND_CONFIG_DIR` / `SYNCMIND_DATA_DIR` env overrides so tests (and multi-instance users) don't pollute the real user config on macOS (where the `dirs` crate ignores XDG_*)
- [x] 6.6 Refactored existing `core/syncmind/tests/cli.rs` to use the new env overrides (was silently failing on machines with pre-existing user config)
- [ ] 6.7 Commit per Gap with conventional commit messages (pending user direction)
- [ ] 6.8 Open PR `feat(core): complete phase1 headless mcp gaps` (pending user direction)
- [ ] 6.9 After merge: archive OpenSpec change to `openspec/changes/archive/2026-05-20-phase1-completion/`
