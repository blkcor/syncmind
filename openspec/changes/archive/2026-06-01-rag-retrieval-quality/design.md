## Context

SyncMind's RAG pipeline (extract → chunk → embed → store → search) has two quality gaps: chunking for non-code/non-markdown file types is a blind character-count split, and retrieval has no relevance threshold applied correctly. This design addresses both.

## Goals / Non-Goals

**Goals:**
- Every indexed file type gets semantically meaningful chunk boundaries
- Chunk content displayed to users is pristine (no concatenated signatures)
- Search results include adjacent chunks for complete context (Sentence Window)
- Relevance threshold correctly gates low-quality results

**Non-Goals:**
- No database schema migration
- No new MCP tool. The existing `search_knowledge` response uses expanded display content.
- No ONNX reranker download automation (out of scope — manual setup continues)
- No adaptive chunk sizing (stays at config `chunk_size`)

## Architecture

```
Extract ──→ Chunk ──→ Inject Context Prefix ──→ Embed ──→ Store
                                                              │
                                        ┌─────────────────────┘
                                        ↓
                              Search (vector/hybrid)
                                        │
                                        ↓ threshold filter (post-RRF)
                                        │
                                        ↓ fetch adjacent chunks
                                        │
                                        ↓ merge → display_content
```

### Component Changes

#### 1. CssChunker (`core/rag-engine/src/chunker.rs`)

```
Input: CSS text
  ↓ split by '}'  (rule boundaries)
  ↓ identify selector for each rule (text before '{')
  ↓ if rule > chunk_size: split by ';' (property boundaries)
  ↓     prefix each sub-chunk with "/* context: selector */"
  ↓     (the comment is for embedding, stays in content)
  ↓ emit Chunk { content, start_line, end_line, chunk_index }
```

The chunker does NOT use tree-sitter-css to avoid adding a native dependency. CSS syntax is regular enough that `}` and `{` boundary parsing is reliable. Selectors are extracted by taking the text before the first `{` of each rule.

For SCSS/Less: nested rules (rules inside rules) are treated as part of the parent rule. Closing braces are counted; a top-level rule ends when the brace depth reaches 0. This preserves nesting context in the chunk.

#### 2. CodeChunker Signature Change

**Before:**
```rust
// chunk_semantically prepends signature directly:
let final_content = format!("{}\n{}", sig_prefix, accum);
```

**After:**
```rust
// chunk_semantically keeps content pristine:
let final_content = accum;
// A new context_prefix field is set on Chunk:
chunk.context_prefix = sig_prefix; // e.g. "class UserService {"
```

`syncmind_core::Chunk` gains an optional `context_prefix: Option<String>` field.

During indexing (`syncmind-indexing/src/lib.rs`), the context prefix is prepended to the text sent to the embedder:
```rust
let embed_text = if let Some(ref prefix) = chunk.context_prefix {
    format!("{}\n{}", prefix, chunk.content)
} else {
    chunk.content.clone()
};
```

The storage layer stores the ORIGINAL `chunk.content` (without prefix) for display. The FTS index gets the prefixed version so keyword search also benefits from the context. This changes the existing semantic-chunking behavior from "signature in content" to "signature in indexing text only".

#### 3. FallbackChunker Enhancement

Add paragraph awareness: before chunking, split input by blank lines into paragraphs. Chunk paragraphs together up to `chunk_size`. Single oversized paragraphs fall back to line-based splitting. This mirrors the logic already in `CodeChunker::chunk_semantically` but generalized for the fallback path.

#### 4. Sentence Window Retrieval (`core/storage/src/store.rs`)

New method on `VectorStore`:

```rust
fn expand_with_adjacent_chunks(
    &self,
    results: Vec<SearchResult>,
    window: usize, // 2
) -> Result<Vec<SearchResult>, StorageError>
```

For each result, queries `chunks` table WHERE `file_id = ? AND chunk_index BETWEEN ? AND ?`, fetches chunks, concatenates their content into `display_content`. Deduplicates by chunk_id.

Deduplication is per returned result's display window. If two top-K hits overlap, they remain separate search results with their original `chunk_id`, score, pin state, and metadata; each result's `display_content` contains each adjacent chunk at most once.

#### 5. Hybrid Search Threshold Fix

**Before (bug):**
```rust
// Line 475-480: filters on raw L2 distance, then overwrites with RRF score
.filter(|(r, _)| {
    if let Some(th) = threshold {
        Self::l2_to_similarity(r.score) >= th  // r.score is raw L2 distance!
    } else { true }
})
```

**After (fix):**
```rust
// Filter AFTER RRF fusion and normalization, on the final score
// ...RRF fusion happens first...
let max_fused = results
    .iter()
    .map(|(_, rrf_score)| *rrf_score)
    .fold(0.0, f64::max);

let mut final_results: Vec<SearchResult> = results
    .into_iter()
    .map(|(mut r, rrf_score)| {
        let normalized = if max_fused > 0.0 {
            rrf_score / max_fused
        } else {
            rrf_score
        };
        r.score = normalized;
        (r, normalized)
    })
    .filter(|(_, normalized)| {
        if let Some(th) = threshold {
            *normalized >= th
        } else { true }
    })
    .take(top_k)
    .map(|(r, _)| r)
    .collect();
```

#### 6. Config Defaults

```rust
relevance_threshold: Some(0.4),  // was None
chunk_overlap: 128,              // was 50
```

## Data Flow

```
1. User searches "fabric"
2. Query → embed → query_embedding
3. search_hybrid(query_embedding, "fabric", top_k=5, threshold=Some(0.4))
   a. Vector search: top 10 chunks
   b. FTS5 search: top 10 chunks
   c. RRF fusion → normalized scores [0,1]
   d. Filter: keep only scores >= 0.4
   e. Take top 5
4. expand_with_adjacent_chunks(results, window=2)
   a. For each result, fetch chunk_index ± 2 from same file
   b. Concatenate: "...\nchunk N-2\n...\nchunk N (matched)\n...\nchunk N+2\n..."
   c. Set display_content
5. Return to caller
```

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| DEFAULT_THRESHOLD breaks existing users who rely on seeing all results | Threshold can be overridden per-request via MCP `threshold` parameter or set to 0.0 in config |
| Sentence window expansion adds N+1 queries per search | Adjacent chunks are fetched in a single SQL batch query; window size is small (±2) |
| CSS `}` splitting can break on `content: "}"` in pseudo-elements | Unlikely in practice; CSS strings use `\"` escaping; a brace-depth counter handles nested `{}` in `@media` blocks |
| Removing signature concatenation changes embedding vectors → all chunks need re-indexing | Vector dimension stays same (1024); users should re-index after this change (will happen on next daemon restart via file watcher) |
