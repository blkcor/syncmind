# ocr-text-extraction Specification Delta

## Purpose
Desktop-side OCR (optical character recognition) for mobile `capture-image` bundles, using the `ocrs` Rust crate integrated into `core/rag-engine`. The same `ocrs` backend also replaces the legacy `tesseract` system-command OCR in the document extraction pipeline. `ocrs` requires local RTen model files; SyncMind reads their paths from `SYNCMIND_OCR_DETECTION_MODEL` and `SYNCMIND_OCR_RECOGNITION_MODEL` and degrades gracefully if they are absent.

## ADDED Requirements

### Requirement: Image text extraction via ocrs
The system SHALL extract text from incoming `capture-image` JPEG/PNG content by calling `core/rag-engine`'s `ocr` module (backed by `ocrs`).

#### Scenario: Successful OCR with English text
- **WHEN** a `capture-image` bundle is dispatched
- **AND** the binary image has been written to `<data-dir>/sync-inbox/images/<id>.jpg`
- **AND** the placeholder markdown has been written to `<data-dir>/sync-inbox/captures/<id>.md`
- **THEN** the system spawns a background task calling `rag_engine::ocr::ocr_image(path)` via `tokio::task::spawn_blocking`
- **AND** `ocrs` loads the image via the `image` crate and performs text recognition with the configured RTen detection and recognition models
- **AND** the extracted text is non-empty and >= 10 characters after trimming whitespace
- **THEN** the system atomically rewrites the `<id>.md` file:
  - frontmatter: `source: mobile-capture`, `ocr_engine: ocrs`, `ocr_languages: en`
  - body: extracted text
  - trailing block: `image_file: ../images/<id>.jpg`
- **AND** the system triggers a re-index of the updated `.md` file

#### Scenario: OCR model files missing
- **WHEN** the OCR model environment variables are unset or point to missing files
- **THEN** `core/rag-engine` returns an OCR initialization error
- **AND** mobile image capture keeps the placeholder markdown unchanged
- **AND** the inbox pipeline does not crash or block

### Requirement: OCR failure / low-confidence degradation
The system SHALL handle OCR failures without blocking the inbox pipeline.

#### Scenario: OCR returns no text or very short text
- **WHEN** `ocrs` returns zero recognized lines
- **OR** all recognized lines joined are shorter than 10 characters after trimming
- **THEN** the system appends `[image: no text detected]` to the body of the existing placeholder markdown
- **AND** the system does NOT change the frontmatter or trigger a re-index

#### Scenario: ocrs engine initialization failure
- **WHEN** the `ocrs::OcrEngine` fails to initialize (missing model data / incompatible device)
- **THEN** the system logs the error
- **AND** the system falls back to the existing placeholder markdown unchanged
- **AND** the system does NOT crash or block the inbox pipeline

### Requirement: Image decode resilience
The system SHALL handle corrupt or unsupported image formats gracefully.

#### Scenario: Corrupt JPEG image
- **WHEN** the image bytes cannot be decoded by the `image` crate
- **THEN** the system logs the error
- **AND** the system appends `[image decode failed — OCR unavailable]` to the body
- **AND** the original binary file remains on disk unchanged

### Requirement: OCR engine lifecycle management
The system SHALL initialize the `ocrs::OcrEngine` once and hold it as a process-wide singleton via `OnceLock` in `core/rag-engine/src/ocr.rs`.

#### Scenario: OcrEngine is initialized once
- **WHEN** multiple `capture-image` bundles arrive in quick succession
- **AND** the engine is initialized after the first bundle
- **THEN** subsequent bundles share the same loaded engine instance
- **AND** engine initialization runs exactly once regardless of how many images arrive

#### Scenario: OcrEngine is shared between local file indexing and mobile capture
- **WHEN** `ImageOcrExtractor::extract()` runs for a local image file during regular indexing
- **AND** concurrently a `capture-image` bundle triggers background OCR
- **THEN** both paths use the same `OnceLock<OcrEngine>` singleton
- **AND** no duplicate engine memory is allocated
