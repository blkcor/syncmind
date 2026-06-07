# ocr-text-extraction Specification

## Purpose
Desktop-side OCR (optical character recognition) for mobile `capture-image` bundles, using the `ocrs` Rust crate integrated into `core/rag-engine`. The same `ocrs` backend also replaces the legacy `tesseract` system-command OCR in the document extraction pipeline. `ocrs` requires local RTen model files; SyncMind reads their paths from `SYNCMIND_OCR_DETECTION_MODEL` and `SYNCMIND_OCR_RECOGNITION_MODEL` and degrades gracefully if they are absent.

## Requirements

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
  - body: extracted text, preserving any existing non-empty caption text
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

### Requirement: Image captions are preserved in desktop markdown
Desktop `capture-image` dispatch SHALL parse the optional `caption` field from mobile image payloads and preserve non-empty captions in generated markdown as searchable body text. Caption text SHALL remain present after successful OCR post-processing, OCR no-text fallback, image decode failure fallback, and OCR initialization failure fallback.

#### Scenario: Captioned image writes caption into placeholder markdown
- **WHEN** a `capture-image` bundle is dispatched
- **AND** the payload includes a non-empty `caption`
- **THEN** desktop writes the image file to `<data-dir>/sync-inbox/images/<id>.jpg`
- **AND** writes `<data-dir>/sync-inbox/captures/<id>.md`
- **AND** the markdown body includes the caption text as plain markdown content
- **AND** the caption is indexed with the placeholder markdown

#### Scenario: Null caption does not create empty caption block
- **WHEN** a `capture-image` bundle is dispatched
- **AND** the payload has `caption = null` or an empty caption after trimming
- **THEN** desktop writes the same image and placeholder markdown paths
- **AND** the markdown does not include an empty caption section

#### Scenario: Successful OCR preserves caption
- **WHEN** a captioned `capture-image` bundle has already written placeholder markdown
- **AND** OCR later returns recognized text of at least 10 trimmed characters
- **THEN** desktop rewrites the markdown with the caption text still present
- **AND** includes the OCR text
- **AND** includes the `image_file` reference
- **AND** triggers re-index of the updated markdown

#### Scenario: OCR fallback preserves caption
- **WHEN** a captioned `capture-image` bundle has already written placeholder markdown
- **AND** OCR returns no usable text, fails to decode the image, or is unavailable
- **THEN** desktop keeps the caption text in markdown
- **AND** appends or preserves the appropriate fallback marker without deleting the caption
