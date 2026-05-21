# embedding-generation Specification

## Purpose
Local embedding generation with Ollama primary and ONNX fallback, including first-run auto-download of ONNX assets to support fully offline operation after initial setup.

## Requirements

### Requirement: ONNX asset auto-download
When `OnnxEmbedder::from_config` is invoked and the model or tokenizer file is missing from `~/.local/share/syncmind/models/`, the system SHALL download the missing assets from the configured Hugging Face URL (or a user-supplied mirror) before initializing the ONNX session.

#### Scenario: First-run download from Hugging Face
- **WHEN** the daemon starts for the first time without Ollama running
- **AND** neither `bge-small-en-v1.5.onnx` nor `tokenizer.json` exist in the model cache directory
- **THEN** the system downloads both files from the default Hugging Face URL
- **AND** the daemon emits `tracing::info!("downloading bge-small-en-v1.5 …")` before each request
- **AND** files land at `~/.local/share/syncmind/models/bge-small-en-v1.5.onnx` and `~/.local/share/syncmind/models/tokenizer.json`

#### Scenario: User-configured mirror takes precedence
- **WHEN** `config.toml` sets `onnx_model_url` and/or `onnx_tokenizer_url`
- **THEN** the system uses those URLs instead of the Hugging Face defaults

#### Scenario: Existing files skip download
- **WHEN** the model and tokenizer files already exist with non-zero size
- **THEN** the system skips download and proceeds to ONNX session initialization

#### Scenario: Atomic write protects against partial files
- **WHEN** a download is in progress
- **THEN** bytes are written to `<filename>.part`
- **AND** only on full completion the file is renamed to the final name
- **AND** crash during download leaves no half-written file at the final path on next launch

#### Scenario: Concurrent daemons do not race
- **WHEN** two daemon processes start within the same second on a fresh installation
- **THEN** only one acquires the `<filename>.lock` exclusive lock and downloads
- **AND** the other polls until the lock releases, then uses the cached file

#### Scenario: Download failure surfaces a clear error
- **WHEN** the configured URL returns HTTP 404 or a network error
- **THEN** the system returns `EmbedError::Onnx` containing the URL and HTTP status
- **AND** the daemon does not crash; the error propagates to the caller of `AutoEmbedder::new`
