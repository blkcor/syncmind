## 1. Document Extraction Configuration

- [x] 1.1 Add config fields for OCR mode (`disabled`/`auto`/`force`), embedded-text quality threshold, and optional local OCR/rendering tool paths
- [x] 1.2 Define default behavior so clean PDFs use embedded text extraction without requiring OCR dependencies
- [x] 1.3 Add validation for invalid OCR mode values and unusable threshold values

## 2. Local OCR/Layout Extraction

- [x] 2.1 Add local OCR dependency detection for Tesseract or the selected local OCR adapter
- [x] 2.2 Add local PDF page rendering dependency detection for Poppler or the selected local renderer
- [x] 2.3 Implement PDF extraction flow: embedded text first, quality check, then OCR/layout fallback when `auto` requires it
- [x] 2.4 Implement `force` mode to run local OCR/layout for PDFs even when embedded text exists
- [x] 2.5 Implement image-file OCR extraction for supported image extensions when OCR is enabled
- [x] 2.6 Ensure missing OCR/rendering dependencies return recoverable warnings and do not abort daemon startup or the full indexing run
- [x] 2.7 Ensure OCR/layout code paths do not make HTTP requests or call cloud services

## 3. Tree-Sitter Language Registry

- [x] 3.1 Add tree-sitter grammar dependencies or feature-gated grammar adapters for Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin
- [x] 3.2 Extend extension-to-language mapping for `.java`, `.c`, `.h`, `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.cs`, `.rb`, `.php`, `.swift`, `.kt`, and `.kts`
- [x] 3.3 Extend language parser registration for the added languages while preserving Rust, Python, JavaScript, TypeScript, and Go mappings
- [x] 3.4 Define language-specific AST boundary node sets for each added language
- [x] 3.5 Route unsupported extensions and parser initialization/parse failures to `FallbackChunker` with structured warnings

## 4. Extraction Tests

- [x] 4.1 Add a clean text PDF fixture test proving embedded PDF extraction is used and OCR is not required
- [x] 4.2 Add a scanned or image-only PDF fixture test proving `auto` mode falls back to OCR when local dependencies are available
- [x] 4.3 Add an image-file OCR fixture test for enabled OCR mode
- [x] 4.4 Add missing OCR dependency tests proving scanned PDFs/images degrade gracefully and unrelated indexing continues
- [x] 4.5 Add forced-OCR configuration test for PDFs with embedded text

## 5. Chunker Tests

- [x] 5.1 Add representative fixtures for Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin
- [x] 5.2 Assert each added fixture produces language-aware chunks at expected declaration/function boundaries
- [x] 5.3 Add unsupported-language fixture coverage proving `FallbackChunker` is used without error
- [x] 5.4 Add regression tests proving Rust, Python, JavaScript, TypeScript, and Go still use tree-sitter chunking
- [x] 5.5 Add parser-failure coverage proving supported languages fall back per file without aborting indexing

## 6. Verification

- [x] 6.1 Run targeted extractor and chunker tests for all new fixtures
- [x] 6.2 Run the core workspace test suite
- [x] 6.3 Run `cargo check` and `cargo clippy` for affected crates
- [x] 6.4 Manually index a mix of clean PDF, scanned PDF/image-only PDF, supported code fixtures, and an unsupported code file to verify end-to-end graceful behavior
