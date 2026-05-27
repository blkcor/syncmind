# document-extraction-quality Specification Delta

## MODIFIED Requirements

### Requirement: Local OCR for image inputs
The extractor SHALL support local OCR extraction for registered image files using the embedded `ocrs` engine. No external OCR binary installation is required, but `ocrs` model files must be available locally.

#### Scenario: Image-only file extracts text locally
- **WHEN** a registered image file is indexed
- **AND** `ocrs` engine initialization succeeds with configured local model files
- **THEN** the extractor SHALL run OCR via `ocrs` over the image
- **AND** return extracted text for chunking

#### Scenario: OCR engine initialization fails
- **WHEN** a registered image file is indexed
- **AND** `ocrs` engine fails to initialize (e.g., incompatible device)
- **THEN** the extractor SHALL return `ExtractError::OcrUnavailable`
- **AND** indexing SHALL skip only that file
- **AND** the daemon SHALL continue indexing other files

### Requirement: Local OCR fallback for low-quality PDF extraction
The extractor SHALL fall back to local OCR extraction when embedded PDF text extraction is empty or below the configured quality threshold and OCR mode is `auto`. The OCR step uses the embedded `ocrs` engine with local model files — no external tesseract installation required.

#### Scenario: Scanned PDF falls back to OCR
- **WHEN** a registered PDF produces no embedded text
- **AND** OCR mode is `auto`
- **AND** a PDF renderer (pdftoppm) is available for page rasterization
- **THEN** the extractor SHALL render the PDF pages via pdftoppm
- **AND** run `ocrs` over each rendered page image
- **AND** return extracted text for chunking

#### Scenario: Low-quality embedded text falls back to OCR
- **WHEN** a registered PDF produces embedded text that fails the configured quality threshold
- **AND** OCR mode is `auto`
- **AND** a PDF renderer (pdftoppm) is available
- **THEN** the extractor SHALL use `ocrs`-based OCR instead of the low-quality embedded text

## REMOVED Requirements

### Requirement: OCR binary availability check
**Reason:** OCR backend changed from tesseract system command to `ocrs` Rust crate. No runtime OCR binary check is needed; model-file loading is handled by `core/rag-engine/src/ocr.rs`.

**Migration:** Remove `ocr_available()` checks. The `OcrConfig::ocr_binary_path` field is no longer used. Users who previously relied on the tesseract binary path override should remove that configuration entry.

### Requirement: OCR Disabled mode
**Reason:** Since `ocrs` is embedded as a Rust dependency (no system install cost), there is no longer a need for a `Disabled` mode. Users who wish to skip image indexing can use file-type filtering instead.

**Migration:** Replace `OcrMode::Disabled` with file-type filter exclusion if image indexing is not desired. `OcrConfig::mode` remains for PDF `auto`/`force` policy, but it no longer disables image OCR.
