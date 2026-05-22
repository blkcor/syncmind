## Context

The Phase 1 PRD requires Markdown, code, and PDF extraction, with image OCR originally deferred but called out as an open quality question. It also requires AST-aware code chunking, but the existing active RAG proposal only expands tree-sitter coverage to Go and focuses on retrieval improvements. This change resolves PRD 001 Open Questions Q1 and Q2 as a standalone ingestion-coverage proposal: local optional OCR/layout fallback for hard PDFs/images, and broad default tree-sitter coverage for common code languages.

Current constraints still apply: all user data stays local, indexing failures must not stop the daemon, and unsupported file types or languages should fall back rather than crash. OCR/layout tooling is heavier than embedded text extraction, so it must be opt-in or automatic only when the cheap path is empty or low quality.

## Goals / Non-Goals

**Goals:**

- Prefer existing embedded PDF text extraction when it produces usable text.
- Add optional local OCR/layout fallback for scanned PDFs, image-only PDFs, and registered image files.
- Keep OCR dependency failures non-fatal and visible through structured warnings.
- Expand default tree-sitter chunking coverage to Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin.
- Preserve `FallbackChunker` for unsupported extensions and parser failures.
- Provide fixture-driven tests for all new language mappings and document extraction modes.

**Non-Goals:**

- Cloud OCR, cloud layout parsing, or external document-processing APIs.
- Implementing a full document layout model in Phase 1; layout extraction only needs to improve text order enough for chunking.
- Automatic language detection from file content.
- Retrieval ranking, hybrid search, reranking, or vector storage schema changes.
- Duplicating `rag-retrieval-enhancement`'s Go-only semantic sub-chunking scope.

## Decisions

### 1. Embedded PDF text remains the primary path

**Rationale:** Clean PDFs are faster and cheaper to process with existing embedded text extraction. OCR should only run when that path is empty, low quality, or explicitly forced by configuration.

**Alternative considered:** Always OCR PDFs. Rejected because it adds unnecessary CPU cost, increases dependency surface, and can produce worse text for clean digital PDFs.

### 2. OCR/layout is local-only and optional

**Rationale:** The PRD privacy model requires local storage and no data exfiltration. Tesseract plus Poppler/pdf rendering are acceptable because they run locally and can be detected at startup or extraction time.

**Alternative considered:** Cloud OCR APIs. Rejected because they violate the local-only ingestion requirement and require credentials/network access.

### 3. Missing OCR dependencies degrade gracefully

**Rationale:** OCR support should improve coverage for users who install local tools, not make baseline indexing fragile. If OCR is disabled or unavailable, clean PDFs still use embedded text extraction; scanned PDFs/images produce a warning and skip only the affected file or preserve best-effort embedded text.

**Alternative considered:** Fail daemon startup when OCR is configured but dependencies are missing. Rejected because optional ingestion support must not block unrelated indexing.

### 4. Tree-sitter support is extension-driven

**Rationale:** The current chunker model uses file extension mapping and explicit grammar registration. Keeping that model avoids content sniffing ambiguity and keeps unsupported languages predictable.

**Alternative considered:** Detect language from file content or shebang. Rejected for Phase 1 because it introduces edge cases without changing the default coverage decision.

### 5. Go coverage is included in the target set but not duplicated

**Rationale:** `rag-retrieval-enhancement` already specifies Go tree-sitter support. This change depends on the same end-state default language set and adds the remaining languages: Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin.

**Alternative considered:** Restating all Go node-boundary details here. Rejected to avoid conflicting deltas if both changes are reviewed concurrently.

### 6. Parser failures fall back per file

**Rationale:** Native tree-sitter grammars can fail to build, load, or parse malformed source. The indexing pipeline should continue by using `FallbackChunker` for that file and recording the reason.

**Alternative considered:** Treat parser failure as file indexing failure. Rejected because fallback chunks are better than losing the file entirely.

## Risks / Trade-offs

- **[Risk]** OCR can be CPU intensive on large PDFs. **Mitigation:** Default to `auto`, run only after embedded text quality checks, and keep `force` as an explicit user choice.
- **[Risk]** OCR output can be noisy and hurt embeddings. **Mitigation:** Preserve extraction metadata/warnings and add tests for image-only fixtures so quality regressions are visible.
- **[Risk]** Poppler/Tesseract installation varies by OS. **Mitigation:** Treat tools as optional runtime dependencies, document detection points in implementation tasks, and test missing-dependency paths.
- **[Risk]** Many tree-sitter grammars increase compile time and binary size. **Mitigation:** Use a central language registry and optional Cargo feature grouping if needed, while keeping the default workspace feature set aligned with the required language list.
- **[Risk]** C and C++ extension mapping can be ambiguous for headers. **Mitigation:** Map common C headers (`.h`) conservatively and include C++ headers (`.hpp`, `.hh`, `.hxx`) explicitly; unsupported or ambiguous cases can still fall back.

## Migration Plan

1. Add configuration defaults with OCR disabled or automatic behavior that preserves existing embedded-PDF extraction for clean PDFs.
2. Introduce OCR/layout adapters behind a local capability check so missing binaries/libraries only affect files that require OCR.
3. Extend the chunker language registry and parser-node boundary definitions for the added languages.
4. Add fixtures and regression tests before enabling the expanded default language set.
5. Rollback is config-only for OCR (`disabled`) and implementation-level for language grammars because unsupported extensions already fall back to `FallbackChunker`.

## Open Questions

None. This proposal resolves PRD 001 Q1 by choosing optional local OCR/layout fallback and resolves Q2 by choosing broad Phase 1 language coverage across the listed default set.
