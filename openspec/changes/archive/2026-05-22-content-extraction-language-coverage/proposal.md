## Why

PRD 001 left two Phase 1 quality decisions open: whether PDF extraction needs local OCR/layout support for complex or scanned documents, and whether code chunking should support more than the initial tree-sitter languages. Resolving both together closes the content-ingestion coverage gap before retrieval quality is tuned further.

## What Changes

- Add optional local-only OCR/layout extraction for PDFs and image files, while keeping embedded PDF text extraction as the default first path.
- Add configuration to force OCR, disable OCR, and tune the low-quality-text fallback threshold without introducing any cloud dependency.
- Require graceful degradation when Tesseract, Poppler/pdf rendering, or image OCR dependencies are unavailable: indexing logs a warning and continues with the best available text or skips only the affected file.
- Expand tree-sitter code chunking coverage from Rust, Python, JavaScript/TypeScript, and Go to also include Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin.
- Preserve `FallbackChunker` behavior for unsupported languages and for any supported language whose parser is unavailable or fails.
- Add extraction and chunking test coverage for clean PDFs, scanned/image-only PDFs, missing OCR dependencies, every added language fixture, unsupported fallback, and existing-language regression.
- No MCP protocol or vector storage schema changes are required.

## Capabilities

### New Capabilities

- `document-extraction-quality`: Local document extraction fallback behavior for complex PDFs and image-only inputs, including optional OCR/layout configuration and graceful dependency failure handling.
- `code-language-coverage`: Tree-sitter language coverage contract for code chunking across the default supported language set and fallback behavior for unsupported inputs.

### Modified Capabilities

- None.

## Impact

- **Core crates affected**: content extraction modules, indexing pipeline error/log handling, code chunker language registry, chunker tests/fixtures.
- **New optional local dependencies**: Tesseract OCR bindings or CLI integration, Poppler/pdf rendering or equivalent local renderer, plus tree-sitter grammars for Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin. Go remains covered by `rag-retrieval-enhancement` and is part of the final default set.
- **Config additions**: OCR mode (`disabled`/`auto`/`force`), quality threshold for embedded PDF text, optional OCR binary/library paths, and optional per-language grammar feature flags if the implementation uses Cargo features.
- **Privacy**: OCR/layout processing is local-only. This change must not send document bytes, extracted text, images, or OCR payloads to external services.
- **Compatibility**: Unsupported languages and missing optional OCR dependencies continue to degrade gracefully; existing Rust/Python/JS/TS/Go behavior must remain compatible.
