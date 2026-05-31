# RAG Retrieval Quality Optimization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix chunk semantic completeness (CSS, code, fallback) and retrieval accuracy (threshold, sentence window).

**Architecture:** Modify 4 crates. Add `context_prefix` to `Chunk`, add `CssChunker`, refactor `CodeChunker` to use prefix, enhance `FallbackChunker` with paragraph awareness, fix hybrid threshold bug in `search_hybrid`, add `expand_with_adjacent_chunks`, update config defaults.

**Tech Stack:** Rust, sqlite-vec, tree-sitter (existing), no new crates needed.

---

### Task 1: Add `context_prefix` to `Chunk` + `display_content` to `SearchResult`

**Files:** Modify `core/syncmind-core/src/types.rs`, `core/storage/src/models.rs`

- [ ] **Step 1: Add `context_prefix` field to `Chunk`**

In `core/syncmind-core/src/types.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    /// Optional context (e.g. "class Foo {") prepended during embedding only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_prefix: Option<String>,
}
```

- [ ] **Step 2: Add `display_content` field to `SearchResult`**

In `core/storage/src/models.rs`:
```rust
/// Full display text with adjacent chunks merged in. Set by sentence-window expansion.
/// Falls back to `content` when sentence-window is disabled.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub display_content: Option<String>,
```

- [ ] **Step 3: Verify compilation**

Run: `cd core && cargo check 2>&1`
Expected: compile errors in callers of `Chunk` and `SearchResult` constructors (we'll fix in subsequent tasks).

---

### Task 2: Implement `CssChunker`

**Files:** Modify `core/rag-engine/src/chunker.rs`

- [ ] **Step 1: Add `CssChunker` struct + `Chunker` impl**

```rust
pub struct CssChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl CssChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self { chunk_size, chunk_overlap }
    }

    /// Split CSS text by rule boundaries (brace-depth counter).
    fn split_rules(&self, text: &str) -> Vec<(usize, String)> {
        let mut rules: Vec<(usize, String)> = Vec::new();
        let mut current = String::new();
        let mut depth: i32 = 0;
        let mut start_line: usize = 1;
        let mut rule_start_line: usize = 1;

        for (line_num, line) in text.lines().enumerate() {
            let line_no = line_num + 1;
            if current.is_empty() {
                rule_start_line = line_no;
            }
            for ch in line.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            // Rule complete
                        }
                    }
                    _ => {}
                }
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);

            if depth == 0 && !current.trim().is_empty() {
                rules.push((rule_start_line, std::mem::take(&mut current)));
            }
        }
        // Trailing text without closing brace
        if !current.trim().is_empty() {
            rules.push((rule_start_line, current));
        }
        rules
    }

    /// Extract selector from a CSS rule (text before first `{`).
    fn extract_selector(rule: &str) -> Option<&str> {
        let brace_pos = rule.find('{')?;
        Some(rule[..brace_pos].trim())
    }
}
```

Full implementation in chunker.rs with these methods.

- [ ] **Step 2: Implement `Chunker` trait for `CssChunker`**

Delegate each rule to `FallbackChunker::chunk_lines`. For oversized rules (> chunk_size), split by property lines and prepend `/* context: <selector> */` to each sub-chunk. Set `context_prefix` to `Some(selector)` for embedding.

- [ ] **Step 3: Add tests**

```rust
#[test]
fn css_chunker_splits_by_rules() { /* 5 rules → 5 chunks */ }
#[test]
fn css_chunker_preserves_selector_context() { /* oversized rule sub-chunks have context */ }
#[test]
fn css_chunker_handles_nested_braces() { /* @media { .foo { } } → 1 chunk */ }
#[test]
fn css_chunker_empty_input() { /* "" → 0 chunks */ }
#[test]
fn scss_nested_rules_single_chunk() { /* .parent { .child { } } → 1 chunk */ }
```

---

### Task 3: Refactor `CodeChunker` — context_prefix instead of hard concatenation

**Files:** Modify `core/rag-engine/src/chunker.rs`

- [ ] **Step 1: Change `chunk_semantically` to set `context_prefix`**

In `CodeChunker::chunk_semantically`, replace:
```rust
// Before:
let final_content = format!("{}\n{}", sig_prefix, accum);
```
With:
```rust
// After: content stays pristine
c.context_prefix = sig_prefix; // Option<String>
```

- [ ] **Step 2: Update all callers in `CodeChunker::chunk`**

The `chunk` method already calls `chunk_semantically` with `Some(&signature)`. The signature is now stored as `context_prefix` on each sub-chunk. For non-oversized chunks, set `context_prefix` to `Some(signature)` directly.

- [ ] **Step 3: Update existing tests**

Tests that assert `c.content.contains("func BigFunc()")` for sub-chunks need to change — content no longer has the signature. Check `context_prefix` instead:
```rust
assert_eq!(c.context_prefix.as_deref(), Some("func BigFunc() {"));
assert!(!c.content.contains("func BigFunc()"));
```

- [ ] **Step 4: Run tests**

`cd core && cargo test chunker -- --nocapture`
Expected: some tests fail (those asserting content contains signature). Fix them as in Step 3.

---

### Task 4: Enhance `FallbackChunker` with paragraph awareness

**Files:** Modify `core/rag-engine/src/chunker.rs`

- [ ] **Step 1: Add paragraph split method to `FallbackChunker`**

```rust
fn chunk_paragraphs(&self, text: &str) -> Vec<Chunk> {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    // Group paragraphs up to chunk_size, then emit chunks
    // Oversized single paragraphs fall back to line-based chunking
    // ...
}
```

- [ ] **Step 2: Call paragraph chunking from `chunk` method**

In `FallbackChunker::chunk`: if text contains blank lines (paragraph boundaries), use `chunk_paragraphs`. Otherwise fall through to `chunk_lines`.

- [ ] **Step 3: Add tests**

```rust
#[test]
fn fallback_chunker_paragraph_aware() { /* text with \n\n → paragraph-level chunks */ }
#[test]
fn fallback_chunker_oversized_paragraph() { /* single huge paragraph → line-split */ }
```

---

### Task 5: Inject context_prefix during embedding

**Files:** Modify `core/syncmind-indexing/src/lib.rs`

- [ ] **Step 1: Build embedding text with context_prefix**

In `index_file`, before calling `embedder.embed()`:
```rust
let texts: Vec<String> = chunks.iter().map(|c| {
    if let Some(ref prefix) = c.context_prefix {
        format!("{}\n{}", prefix, c.content)
    } else {
        c.content.clone()
    }
}).collect();
let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
let embeddings = embedder.embed(&text_refs).await...;
```

- [ ] **Step 2: Keep stored content WITHOUT context_prefix**

The `chunk.content` stored in DB is pristine (no prefix). FTS content is built from `chunk.content` + file stem + parent dir (already in `upsert_file`).

- [ ] **Step 3: Run indexing tests**

`cd core && cargo test indexing -- --nocapture`
Expected: all pass (tests use simple chunks without context_prefix).

---

### Task 6: Route CSS/SCSS/Less to `CssChunker`

**Files:** Modify `core/syncmind-indexing/src/lib.rs`

- [ ] **Step 1: Add CSS extensions to `chunker_for_path`**

```rust
if ["css", "scss", "less"].iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
    return Box::new(CssChunker::new(chunk_size, chunk_overlap));
}
```

- [ ] **Step 2: Add routing test**

```rust
#[test]
fn chunker_for_path_routes_css_to_css_chunker() {
    let chunker = chunker_for_path(Path::new("styles.css"), 512, 128);
    // Verify it's a CssChunker by chunking CSS text
}
```

---

### Task 7: Fix hybrid search threshold + add sentence window

**Files:** Modify `core/storage/src/store.rs`, `core/storage/src/models.rs`

- [ ] **Step 1: Fix threshold position in `search_hybrid`**

Move the threshold filter from before RRF normalization to after. The current buggy code at lines 475-497:
```rust
// Before (buggy): filters on raw L2 distance
.filter(|(r, _)| {
    if let Some(th) = threshold {
        Self::l2_to_similarity(r.score) >= th
    } else { true }
})
```

Fix: remove the filter from the intermediate step. After RRF score normalization, apply:
```rust
let mut final_results: Vec<SearchResult> = results
    .into_iter()
    .filter(|(_, rrf)| {
        if let Some(th) = threshold {
            *rrf >= th
        } else { true }
    })
    .map(|(mut r, rrf)| { r.score = rrf; r })
    .take(top_k)
    .collect();
```

- [ ] **Step 2: Implement `expand_with_adjacent_chunks`**

```rust
fn expand_with_adjacent_chunks(
    &self,
    results: Vec<SearchResult>,
    window: usize,
) -> Result<Vec<SearchResult>, StorageError> {
    if window == 0 || results.is_empty() {
        return Ok(results);
    }
    let conn = self.conn.lock().unwrap();
    let mut expanded = Vec::with_capacity(results.len());

    // Collect all (file_id, chunk_id) pairs
    let mut file_ids: Vec<i64> = Vec::new();
    let mut chunk_ranges: Vec<(i64, i64)> = Vec::new(); // (min_idx, max_idx)
    for r in &results {
        // Find file_id from chunks table
        let file_id: i64 = conn.query_row(
            "SELECT file_id FROM chunks WHERE id = ?",
            [r.chunk_id],
            |row| row.get(0),
        )?;
        // Find chunk_index for this chunk
        let chunk_idx: i64 = conn.query_row(
            "SELECT chunk_index FROM chunks WHERE id = ?",
            [r.chunk_id],
            |row| row.get(0),
        )?;
        file_ids.push(file_id);
        let min_idx = (chunk_idx as i64 - window as i64).max(0);
        let max_idx = chunk_idx as i64 + window as i64;
        chunk_ranges.push((min_idx, max_idx));
    }

    // For each result, fetch adjacent chunks
    for (i, r) in results.into_iter().enumerate() {
        let (min_idx, max_idx) = chunk_ranges[i];
        let mut stmt = conn.prepare(
            "SELECT content FROM chunks
             WHERE file_id = ? AND chunk_index BETWEEN ? AND ?
             ORDER BY chunk_index ASC"
        )?;
        let contents: Vec<String> = stmt.query_map(
            params![file_ids[i], min_idx, max_idx],
            |row| row.get(0),
        )?.collect::<Result<Vec<_>, _>>()?;

        let mut expanded_r = r;
        expanded_r.display_content = Some(contents.join("\n\n"));
        expanded.push(expanded_r);
    }
    Ok(expanded)
}
```

- [ ] **Step 3: Wire expansion into `search_with_threshold` and `search_hybrid`**

At the end of both methods, call `expand_with_adjacent_chunks(results, 2)`.

- [ ] **Step 4: Add `display_content` to MCP response**

In `core/mcp-server/src/server.rs`, when serializing results, include `display_content`:
```rust
// display_content is already serialized as part of SearchResult
```

- [ ] **Step 5: Update relevant tests**

Hybrid search threshold tests must verify filtering happens on RRF scores, not L2 distance.

---

### Task 8: Config defaults

**Files:** Modify `core/syncmind-core/src/config.rs`

- [ ] **Step 1: Update defaults**

```rust
chunk_overlap: 128,              // line 248: was 50
relevance_threshold: Some(0.4),  // line 250: was None
```

- [ ] **Step 2: Test backward compat**

Legacy config without `relevance_threshold` field should deserialize to `Some(0.4)` (via `#[serde(default)]`).

---

### Task 9: Integration verification

- [ ] **Step 1: Check compilation**

`cd core && cargo check 2>&1`
Expected: no errors.

- [ ] **Step 2: Clippy**

`cd core && cargo clippy --all-targets -- -D warnings 2>&1`
Expected: clean.

- [ ] **Step 3: Full test suite**

`cd core && cargo test 2>&1`
Expected: all tests pass.

- [ ] **Step 4: Manual chunk quality verification**

Index a CSS file, search for a class name, verify:
- Chunk content is a complete rule (selector + properties + `}`)
- `display_content` includes adjacent rules
- Score is meaningful

- [ ] **Step 5: Commit**

```bash
git add core/
git commit -m "feat(core): improve RAG chunk quality and retrieval accuracy"
```
