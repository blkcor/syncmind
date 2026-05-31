## Why

RAG retrieval quality has two systemic issues: (1) chunks from unsupported file types like CSS/SCSS are blindly character-split by `FallbackChunker`, producing semantically incomplete fragments (CSS rules missing selectors or closing braces); (2) `relevance_threshold` defaults to `None`, so even very low similarity results are returned, and the hybrid search path applies threshold before RRF score normalization instead of after. A user searching "fabric" should never see unrelated Svelte chunks with 100% confidence scores.

## What Changes

- **New `CssChunker`** in `core/rag-engine/src/chunker.rs`: splits CSS/SCSS/Less by rule boundaries (`}`), preserving selectors as context prefix for sub-chunked oversized rules.
- **Remove signature hard-concatenation** in `CodeChunker`: chunk content stays pristine; context (class/function signature) is injected via a metadata prefix during embedding only, not into the displayed content.
- **Sentence Window Retrieval**: `VectorStore::search_hybrid` and `search_with_threshold` now fetch adjacent chunks (`chunk_index ± 2`) for each top result and merge them into a single display text, so users see complete logical units, not fragments.
- **Fix hybrid search threshold bug**: in `search_hybrid`, the threshold filter is moved to operate on RRF-fused scores instead of raw L2 distances.
- **Default `relevance_threshold` to 0.4** in `Config::default()`.
- **Increase `chunk_overlap` default from 50 to 128** for better boundary coverage.
- **Enhanced `FallbackChunker`**: add paragraph awareness (blank-line splitting) so plain-text and unsupported formats produce better semantic boundaries.

## Capabilities

### New Capabilities
- `css-chunker`: CSS/SCSS/Less files are chunked by rule boundaries using a dedicated chunker that preserves selector context.
- `sentence-window-retrieval`: search results automatically include adjacent chunks from the same file to reconstruct complete logical units.

### Modified Capabilities / Behaviors
- `semantic-chunking`: chunk content no longer contains hard-concatenated signatures; context is injected through metadata for embedding only.
- `hybrid-search`: the default relevance threshold is 0.4 and hybrid search applies the threshold on normalized fused scores.
- `search-knowledge`: retrieval results now include merged adjacent chunks; threshold filtering is applied correctly post-RRF fusion.

## Impact

- **Affected code**: `core/rag-engine/src/chunker.rs` (CssChunker + FallbackChunker enhancement + CodeChunker signature change), `core/storage/src/store.rs` (hybrid threshold fix + adjacent chunk fetch), `core/syncmind-indexing/src/lib.rs` (chunker routing + context prefix), `core/syncmind-core/src/config.rs` (defaults).
- **New dependencies**: none. CSS/SCSS/Less parsing uses brace-depth and declaration-boundary parsing instead of `tree-sitter-css`.
- **Build & test gates**: `cd core && cargo check && cargo clippy --all-targets -- -D warnings && cargo test`
- **Non-impacts**: No database schema migration; no config file format change; no new daemon process. The MCP tool schema already exposes `threshold`; the `search_knowledge` response behavior changes to prefer `display_content` for returned text.
