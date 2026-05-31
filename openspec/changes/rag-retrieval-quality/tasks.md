## 1. Chunking Improvements

- [ ] 1.1 Add `context_prefix: Option<String>` field to `Chunk` struct in `core/syncmind-core/src/lib.rs`
- [ ] 1.2 Implement `CssChunker` in `core/rag-engine/src/chunker.rs`: rule-boundary splitting with brace-depth counter, selector extraction, oversized-rule sub-chunking with `/* context: */` prefix
- [ ] 1.3 Route `.css`, `.scss`, `.less` to `CssChunker` in `chunker_for_path` (`core/syncmind-indexing/src/lib.rs`)
- [ ] 1.4 Refactor `CodeChunker::chunk_semantically`: set `context_prefix` instead of hard-concatenating signature into content
- [ ] 1.5 Enhance `FallbackChunker`: add paragraph-aware chunking (blank-line split before line-level chunking)
- [ ] 1.6 Inject `context_prefix` during embedding text construction in `index_file` (`core/syncmind-indexing/src/lib.rs`)
- [ ] 1.7 Tests: CSS rule boundary chunking, SCSS nested rule handling, oversized rule sub-chunking, empty CSS, CodeChunker signature-as-prefix, FallbackChunker paragraph awareness

## 2. Retrieval Improvements

- [ ] 2.1 Fix hybrid search threshold bug in `VectorStore::search_hybrid`: move threshold filter to post-RRF normalized scores
- [ ] 2.2 Implement `expand_with_adjacent_chunks` method on `VectorStore`: fetch `chunk_index ± window` from same file, concatenate into `display_content`
- [ ] 2.3 Add `display_content: String` field to `SearchResult` in `core/storage/src/models.rs`
- [ ] 2.4 Wire sentence window expansion into `search_with_threshold` and `search_hybrid` (controlled by new parameter, default `window=2`)
- [ ] 2.5 Return `display_content` in MCP `search_knowledge` response text (see `server.rs`)
- [ ] 2.6 Tests: hybrid threshold post-RRF, adjacent chunk merge, chunk deduplication, window=0 bypass, single-chunk file

## 3. Configuration

- [ ] 3.1 Set `relevance_threshold: Some(0.4)` in `Config::default()`
- [ ] 3.2 Set `chunk_overlap: 128` in `Config::default()`
- [ ] 3.3 Verify backward compat: old config files without `relevance_threshold` get `Some(0.4)` on load
- [ ] 3.4 Tests: config roundtrip with new defaults, legacy config deserialization

## 4. Integration & Validation

- [ ] 4.1 `cd core && cargo check` passes all crates
- [ ] 4.2 `cd core && cargo clippy --all-targets -- -D warnings` clean
- [ ] 4.3 `cd core && cargo test` all tests pass
- [ ] 4.4 Manual verification: index CSS file, search for a CSS class name, confirm display_content includes complete rule
- [ ] 4.5 Manual verification: search for a term that spans multiple consecutive chunks, confirm sentence window merges them
- [ ] 4.6 Manual verification: with `relevance_threshold = 0.4`, search "fabric", confirm no low-relevance chunks returned
- [ ] 4.7 Update `CLAUDE.md` if any workflow commands change
