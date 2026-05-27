## 1. Dependencies & Module Scaffolding

- [x] 1.1 Add `whisper-rs` to `apps/desktop/src-tauri/Cargo.toml`
- [x] 1.2 Add `ocrs` to `core/rag-engine/Cargo.toml`
- [x] 1.3 Create `apps/desktop/src-tauri/src/spine/stt.rs` module stub (`pub mod stt` in `mod.rs`)
- [x] 1.4 Create `apps/desktop/src-tauri/src/spine/ratelimit.rs` module stub (`pub mod ratelimit` in `mod.rs`)
- [x] 1.5 Create `core/rag-engine/src/ocr.rs` module stub (`pub mod ocr` in `lib.rs`)
- [x] 1.6 Verify `cargo check` passes for both crates

## 2. ocrs Backend (`core/rag-engine/src/ocr.rs`)

- [x] 2.1 Initialize `ocrs::OcrEngine` as a process-wide singleton via `OnceLock<OcrEngine>`
- [x] 2.2 Implement `pub fn ocr_image<P: AsRef<Path>>(path: P) -> Result<String, OcrError>` — load image via `image` crate, convert to grayscale `ImageBuffer<Luma<u8>>`, run `ocrs` recognition, return joined text lines
- [x] 2.3 Implement `pub fn ocr_image_from_bytes(bytes: &[u8], format: ImageFormat) -> Result<String, OcrError>` — same as above but from in-memory bytes (for PDF page rendering integration)
- [x] 2.4 Define `OcrError` enum with `Init`, `Decode`, `Recognition` variants, each carrying a `String` message; implement `std::error::Error` and `Display`
- [x] 2.5 Unit tests: corrupt bytes, degenerate image/model-init failure

## 3. Refactor `core/rag-engine/src/extractor.rs`

- [x] 3.1 Update `OcrConfig` — remove `ocr_binary_path` field; remove `OcrMode::Disabled` variant (replace with a single boolean field `ocr_enabled: bool` or keep the enum without Disabled); keep `pdf_renderer_path` and `pdf_text_quality_threshold`
- [x] 3.2 Rewrite `run_image_ocr()` to call `ocr::ocr_image()` instead of `Command::new("tesseract")`
- [x] 3.3 Rewrite `run_pdf_ocr()` — keep pdftoppm rendering step, but replace the tesseract OCR on each rendered page with `ocr::ocr_image_from_bytes()`
- [x] 3.4 Remove `ocr_available()` / `command_available("tesseract")` checks (no longer applicable)
- [x] 3.5 Update `ImageOcrExtractor::extract()` to remove the `OcrMode::Disabled` early-return branch (ocrs initialization errors are recoverable)
- [x] 3.6 Update `CompositeExtractor::with_ocr_config()` and all code paths that reference `ocr_binary_path`
- [x] 3.7 Run all existing extractor tests; update any test that relied on `OcrMode::Disabled` or fake-tesseract binaries

## 4. Rate Limiter (`ratelimit.rs`)

- [x] 4.1 Implement `SlidingWindowRateLimiter` struct with `HashMap<String, VecDeque<Instant>>`
- [x] 4.2 Implement `check_and_record(peer_fingerprint: &str) -> bool` (true = allowed, false = rate-limited)
- [x] 4.3 Implement lazy cleanup of expired entries on insert
- [x] 4.4 Thread-safe via `Arc<tokio::sync::Mutex<SlidingWindowRateLimiter>>`
- [x] 4.5 Wire rate limiter into `dispatch.rs` `search-request` handler path
- [x] 4.6 Build and return `kind: "error"` encrypted envelope when rate-limited
- [x] 4.7 Unit tests: within limit, over limit, window reset, error payload shape

## 5. STT Module (`stt.rs`)

- [x] 5.1 Define `SttState` enum: `Unavailable`, `Downloading`, `Ready(Arc<Mutex<WhisperContext>>)`
- [x] 5.2 Implement `download_model(data_dir, model_name)` — reqwest GET to Hugging Face, atomic write with SHA-256 verification
- [x] 5.3 Implement `ensure_model(data_dir) -> PathBuf` — check existence, download if missing, return path
- [x] 5.4 Implement `transcribe_audio(audio_path, markdown_path, data_dir)` async public function
- [x] 5.5 Load `whisper-rs` context from model file (via `WhisperContext::new`)
- [x] 5.6 Decode `.m4a` to 16-bit 16kHz PCM (use FFmpeg subprocess or `hound` WAV intermediate)
- [x] 5.7 Run `whisper_rs::WhisperContext::full()` and collect segments
- [x] 5.8 Format transcription as SRT-like text with timestamps
- [x] 5.9 Atomically rewrite the `.md` capture file with transcription body
- [x] 5.10 Thread-safe singleton via `OnceLock<tokio::sync::Mutex<SttState>>`
- [x] 5.11 Unit tests: empty transcription, checksum/download failure, model init failure

## 6. Dispatch Integration (`dispatch.rs`)

- [x] 6.1 In `capture-audio` handler: after binary write + placeholder markdown, spawn `stt::transcribe_audio(...)` via `tokio::spawn`
- [x] 6.2 In `capture-image` handler: after binary write + placeholder markdown, spawn async task that calls `rag_engine::ocr::ocr_image()` via `tokio::task::spawn_blocking`, then atomically updates the `.md` file and triggers re-index
- [x] 6.3 Ensure spawned tasks use `tokio::task::spawn_blocking` for CPU-heavy whisper/ocrs work
- [x] 6.4 Ensure all existing dispatch tests still pass with post-processing added

## 7. Document Extraction Spec Sync & Verification

- [x] 7.1 Update `openspec/specs/document-extraction-quality/spec.md` to reflect removal of system OCR dependency
- [x] 7.2 `cargo clippy --workspace --all-targets` passes with no new warnings
- [x] 7.3 `cargo test --workspace` passes (all existing + new tests)
- [x] 7.4 `cargo check` on both core/rag-engine and apps/desktop passes
