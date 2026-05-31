## ADDED Requirements

### Requirement: Embedded PDF text is preferred
The extractor SHALL use embedded PDF text extraction as the first extraction strategy for PDF files unless OCR is explicitly forced by configuration.

#### Scenario: Clean PDF uses embedded text
- **WHEN** a registered PDF contains usable embedded text
- **AND** OCR mode is `auto`
- **THEN** the extractor SHALL return the embedded text without rendering pages for OCR
- **AND** indexing SHALL continue through the normal chunking pipeline

#### Scenario: Forced OCR bypasses embedded preference
- **WHEN** a registered PDF contains embedded text
- **AND** OCR mode is `force`
- **THEN** the extractor SHALL use the local OCR/layout pipeline for the PDF
- **AND** it SHALL NOT call any cloud OCR or remote document service

### Requirement: Local OCR fallback for low-quality PDF extraction
The extractor SHALL fall back to local OCR/layout extraction when embedded PDF text extraction is empty or below the configured quality threshold and OCR mode is `auto`.

#### Scenario: Scanned PDF falls back to OCR
- **WHEN** a registered PDF produces no embedded text
- **AND** OCR mode is `auto`
- **AND** local OCR and PDF rendering dependencies are available
- **THEN** the extractor SHALL render the PDF pages locally
- **AND** run local OCR/layout extraction over the rendered pages
- **AND** return extracted text for chunking

#### Scenario: Low-quality embedded text falls back to OCR
- **WHEN** a registered PDF produces embedded text that fails the configured quality threshold
- **AND** OCR mode is `auto`
- **AND** local OCR and PDF rendering dependencies are available
- **THEN** the extractor SHALL use local OCR/layout extraction instead of the low-quality embedded text

### Requirement: Local OCR for image inputs
The extractor SHALL support local OCR extraction for registered image files using the embedded `ocrs` engine when local OCR model files are available.

#### Scenario: Image-only file extracts text locally
- **WHEN** a registered image file is indexed
- **AND** local OCR model files are configured
- **THEN** the extractor SHALL run `ocrs` locally over the image
- **AND** return extracted text for chunking

#### Scenario: OCR model initialization failure skips image extraction
- **WHEN** a registered image file is indexed
- **AND** the local `ocrs` model files are missing or incompatible
- **THEN** the extractor SHALL return a recoverable OCR warning
- **AND** indexing SHALL skip only that file
- **AND** the daemon SHALL continue indexing other files

### Requirement: Missing OCR dependencies degrade gracefully
The indexing pipeline SHALL NOT fail daemon startup or abort a full indexing run when optional OCR/layout dependencies are missing.

#### Scenario: Scanned PDF with missing OCR dependencies
- **WHEN** a registered scanned PDF requires OCR
- **AND** local OCR model files or PDF rendering dependencies are unavailable
- **THEN** the extractor SHALL return a recoverable extraction warning
- **AND** indexing SHALL skip only the affected file or preserve best-effort embedded text if present
- **AND** the daemon SHALL continue processing remaining registered files

#### Scenario: Clean PDF unaffected by missing OCR dependencies
- **WHEN** a registered PDF contains usable embedded text
- **AND** OCR mode is `auto`
- **AND** local OCR model files are unavailable
- **THEN** the extractor SHALL return the embedded text successfully
- **AND** it SHALL NOT require OCR dependencies for that file

### Requirement: OCR processing remains local-only
The document extraction pipeline SHALL NOT send PDFs, rendered pages, images, extracted text, or OCR metadata to any external network service.

#### Scenario: OCR execution uses local tools only
- **WHEN** OCR/layout extraction runs for a PDF or image
- **THEN** all rendering and OCR work SHALL execute through local binaries or local libraries
- **AND** no HTTP request SHALL be made as part of document extraction
