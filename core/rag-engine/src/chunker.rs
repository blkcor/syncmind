use std::path::Path;

use crate::error::ChunkError;
pub use syncmind_core::Chunk;

pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str, path: &Path) -> Vec<Chunk>;
}

/// Returns true if `line` is a CommonMark ATX heading (1–6 `#` followed by space).
fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    if chars.next() != Some('#') {
        return false;
    }
    let hash_count = 1 + chars.take_while(|&c| c == '#').count();
    if hash_count > 6 {
        return false;
    }
    let after_hashes = &trimmed[hash_count..];
    after_hashes.starts_with(' ')
}

// ── FallbackChunker ──────────────────────────────────────────────────────────

pub struct FallbackChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl FallbackChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }

    /// Build chunks from a slice of lines with a given starting line number (1-indexed).
    fn chunk_lines(&self, lines: &[&str], start_line: usize) -> Vec<Chunk> {
        if lines.is_empty() {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let mut chunk_idx = 0usize;
        let mut i = 0usize;

        while i < lines.len() {
            let mut content = String::new();
            let mut j = i;
            while j < lines.len() && content.len() + lines[j].len() < self.chunk_size + 1 {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(lines[j]);
                j += 1;
            }

            // Ensure we always make progress (at least one line per chunk).
            if j == i {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(lines[i]);
                j = i + 1;
            }

            let end_line = start_line + j - 1;
            chunks.push(Chunk {
                chunk_index: chunk_idx,
                start_line: start_line + i,
                end_line,
                content,
            });
            chunk_idx += 1;

            // Advance with overlap.
            if self.chunk_overlap == 0 || j >= lines.len() {
                i = j;
                continue;
            }

            // Move `i` backward so the next chunk overlaps by ~chunk_overlap chars.
            // NOTE: overlap can cause chunks to slightly exceed chunk_size; this is
            // acceptable for Phase 1 where we target "approximately chunk_size chars".
            let mut overlap_chars = 0usize;
            let mut new_i = j;
            while new_i > i && overlap_chars < self.chunk_overlap {
                new_i -= 1;
                overlap_chars += lines[new_i].len() + 1; // +1 for newline
            }
            // Ensure progress: if overlap calculation didn't move us forward, force at least one line.
            if new_i == i {
                i = j;
            } else {
                i = new_i;
            }
        }

        chunks
    }
}

impl Chunker for FallbackChunker {
    fn chunk(&self, text: &str, _path: &Path) -> Vec<Chunk> {
        if text.is_empty() {
            return Vec::new();
        }
        let lines: Vec<&str> = text.lines().collect();
        self.chunk_lines(&lines, 1)
    }
}

// ── MarkdownChunker ──────────────────────────────────────────────────────────

pub struct MarkdownChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl MarkdownChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }
}

impl Chunker for MarkdownChunker {
    fn chunk(&self, text: &str, path: &Path) -> Vec<Chunk> {
        if text.is_empty() {
            return Vec::new();
        }

        let lines: Vec<&str> = text.lines().collect();

        // Check if there are any headings.
        let has_headings = lines.iter().any(|l| is_heading(l));

        if !has_headings {
            let fb = FallbackChunker::new(self.chunk_size, self.chunk_overlap);
            return fb.chunk(text, path);
        }

        // Split into heading sections.
        let mut sections: Vec<(usize, Vec<&str>)> = Vec::new(); // (start_line, lines)
        let mut current_start: Option<usize> = None;
        let mut current_lines: Vec<&str> = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if is_heading(line) {
                if let Some(start) = current_start {
                    sections.push((start, current_lines));
                }
                current_start = Some(idx + 1); // 1-indexed
                current_lines = vec![*line];
            } else {
                if current_start.is_none() {
                    // Preamble before first heading: treat as its own section.
                    current_start = Some(idx + 1);
                }
                current_lines.push(*line);
            }
        }
        if let Some(start) = current_start {
            sections.push((start, current_lines));
        }

        // Chunk each section.
        let fb = FallbackChunker::new(self.chunk_size, self.chunk_overlap);
        let mut all_chunks: Vec<Chunk> = Vec::new();
        let mut global_idx = 0usize;

        for (sec_start, sec_lines) in sections {
            let sec_text = sec_lines.join("\n");
            let sec_chunks = fb.chunk(&sec_text, path);
            for mut c in sec_chunks {
                c.chunk_index = global_idx;
                c.start_line += sec_start - 1;
                c.end_line += sec_start - 1;
                all_chunks.push(c);
                global_idx += 1;
            }
        }

        all_chunks
    }
}

// ── CodeChunker ──────────────────────────────────────────────────────────────

pub struct CodeChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl CodeChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }

    fn language_from_extension(path: &Path) -> Option<&'static str> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Some("rust"),
            Some("py") => Some("python"),
            Some("js") | Some("ts") | Some("jsx") | Some("tsx") => Some("javascript"),
            _ => None,
        }
    }

    fn node_types_for_language(lang: &str) -> &'static [&'static str] {
        match lang {
            "rust" => &[
                "function_item",
                "impl_item",
                "struct_item",
                "trait_item",
                "enum_item",
            ],
            "python" => &["function_definition", "class_definition"],
            "javascript" => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
                "arrow_function",
            ],
            _ => &[],
        }
    }

    fn parse_with_tree_sitter(text: &str, lang: &str) -> Result<Vec<Chunk>, ChunkError> {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = match lang {
            "rust" => tree_sitter_rust::LANGUAGE.into(),
            "python" => tree_sitter_python::LANGUAGE.into(),
            "javascript" => tree_sitter_javascript::LANGUAGE.into(),
            _ => return Err(ChunkError::Parse(format!("unsupported language: {lang}"))),
        };
        parser
            .set_language(&language)
            .map_err(|e| ChunkError::Parse(format!("parser set_language failed: {e:?}")))?;

        let tree = parser
            .parse(text, None)
            .ok_or_else(|| ChunkError::Parse("tree-sitter parse returned None".to_string()))?;

        let root = tree.root_node();
        let types = Self::node_types_for_language(lang);
        let mut nodes: Vec<tree_sitter::Node> = Vec::new();
        Self::collect_nodes(root, types, &mut nodes);

        if nodes.is_empty() {
            // No top-level definitions found; fallback will be used by caller.
            return Ok(Vec::new());
        }

        let mut chunks = Vec::new();
        for node in nodes {
            let start_byte = node.start_byte();
            let end_byte = node.end_byte();
            let content = text[start_byte..end_byte].to_string();
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;
            chunks.push(Chunk {
                chunk_index: 0, // filled later
                start_line,
                end_line,
                content,
            });
        }

        Ok(chunks)
    }

    fn collect_nodes<'a>(
        node: tree_sitter::Node<'a>,
        types: &[&str],
        out: &mut Vec<tree_sitter::Node<'a>>,
    ) {
        if types.contains(&node.kind()) {
            out.push(node);
            // Do NOT recurse into children to avoid nested duplicates.
            return;
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::collect_nodes(child, types, out);
            }
        }
    }
}

impl Chunker for CodeChunker {
    fn chunk(&self, text: &str, path: &Path) -> Vec<Chunk> {
        if text.is_empty() {
            return Vec::new();
        }

        let fallback = || FallbackChunker::new(self.chunk_size, self.chunk_overlap).chunk(text, path);

        let Some(lang) = Self::language_from_extension(path) else {
            return fallback();
        };

        let raw_chunks = match Self::parse_with_tree_sitter(text, lang) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => return fallback(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "tree-sitter parse failed, falling back");
                return fallback();
            }
        };

        let fb = FallbackChunker::new(self.chunk_size, self.chunk_overlap);
        let mut all_chunks: Vec<Chunk> = Vec::new();
        let mut global_idx = 0usize;

        for mut c in raw_chunks {
            if c.content.len() > self.chunk_size {
                let sub_lines: Vec<&str> = c.content.lines().collect();
                let sub_chunks = fb.chunk_lines(&sub_lines, c.start_line);
                for mut sc in sub_chunks {
                    sc.chunk_index = global_idx;
                    all_chunks.push(sc);
                    global_idx += 1;
                }
            } else {
                c.chunk_index = global_idx;
                all_chunks.push(c);
                global_idx += 1;
            }
        }

        all_chunks
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_chunker_splits_text() {
        let text = "line1\nline2\nline3\nline4\nline5";
        let chunker = FallbackChunker::new(10, 2);
        let chunks = chunker.chunk(text, Path::new("foo.txt"));
        assert!(!chunks.is_empty());
        // Verify overlap: each chunk after first should share at least one line with previous.
        for w in chunks.windows(2) {
            let prev = &w[0];
            let next = &w[1];
            let prev_lines: Vec<&str> = prev.content.lines().collect();
            let next_lines: Vec<&str> = next.content.lines().collect();
            let has_shared = prev_lines.iter().any(|pl| next_lines.iter().any(|nl| pl == nl));
            assert!(has_shared, "chunks should share at least one line: {:?} vs {:?}", prev.content, next.content);
        }
        // Verify line numbers are 1-indexed.
        assert_eq!(chunks[0].start_line, 1);
        // Verify sequential indices.
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
        }
    }

    #[test]
    fn test_fallback_chunker_short_text() {
        let text = "short";
        let chunker = FallbackChunker::new(100, 10);
        let chunks = chunker.chunk(text, Path::new("foo.txt"));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "short");
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 1);
    }

    #[test]
    fn test_markdown_chunker_respects_headings() {
        let text = "# Heading 1\ncontent1\n## Heading 2\ncontent2\n### Heading 3\ncontent3";
        let chunker = MarkdownChunker::new(50, 5);
        let chunks = chunker.chunk(text, Path::new("doc.md"));
        assert!(!chunks.is_empty());
        // Each chunk should start with a heading line or be part of a heading section.
        for c in &chunks {
            assert!(
                c.content.contains("#")
                    || c.content.contains("content1")
                    || c.content.contains("content2")
                    || c.content.contains("content3"),
                "chunk should contain heading or its content"
            );
        }
        // Verify indices are sequential.
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
        }
    }

    #[test]
    fn test_markdown_chunker_no_headings() {
        let text = "just some text\nwithout any headings\nat all";
        let chunker = MarkdownChunker::new(20, 2);
        let chunks = chunker.chunk(text, Path::new("plain.md"));
        // Should behave like FallbackChunker.
        assert!(!chunks.is_empty());
        let fb = FallbackChunker::new(20, 2);
        let fb_chunks = fb.chunk(text, Path::new("plain.md"));
        assert_eq!(chunks.len(), fb_chunks.len());
        for (a, b) in chunks.iter().zip(fb_chunks.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn test_code_chunker_rust_functions() {
        let code = r#"
fn foo() {
    let x = 1;
}

fn bar() {
    let y = 2;
}
"#;
        let chunker = CodeChunker::new(200, 20);
        let chunks = chunker.chunk(code, Path::new("test.rs"));
        assert!(
            chunks.len() >= 2,
            "expected at least two chunks for two functions, got {}",
            chunks.len()
        );
        let contents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        assert!(contents.iter().any(|c| c.contains("foo")));
        assert!(contents.iter().any(|c| c.contains("bar")));
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
            assert!(c.start_line >= 1);
        }
    }

    #[test]
    fn test_code_chunker_unsupported_language() {
        let text = "some text\nmore text";
        let chunker = CodeChunker::new(20, 2);
        let chunks = chunker.chunk(text, Path::new("unknown.xyz"));
        let fb = FallbackChunker::new(20, 2);
        let fb_chunks = fb.chunk(text, Path::new("unknown.xyz"));
        assert_eq!(chunks.len(), fb_chunks.len());
        for (a, b) in chunks.iter().zip(fb_chunks.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn test_chunk_line_numbers_are_1_indexed() {
        let text = "a\nb\nc\nd\ne";
        let chunker = FallbackChunker::new(3, 1);
        let chunks = chunker.chunk(text, Path::new("x.txt"));
        for c in &chunks {
            assert!(
                c.start_line >= 1,
                "start_line should be >= 1, got {}",
                c.start_line
            );
            assert!(
                c.end_line >= c.start_line,
                "end_line should be >= start_line"
            );
        }
    }

    #[test]
    fn test_code_chunker_oversized_function() {
        let mut body = String::new();
        for i in 0..100 {
            body.push_str(&format!("    let x{} = {};\n", i, i));
        }
        let code = format!(
            "fn big() {{\n{}\n}}\n\nfn small() {{\n    let a = 1;\n}}\n",
            body
        );
        let chunker = CodeChunker::new(100, 10);
        let chunks = chunker.chunk(&code, Path::new("big.rs"));
        assert!(
            chunks.len() >= 2,
            "expected oversized function to be split, got {} chunks",
            chunks.len()
        );
        // At least one chunk should contain part of big().
        assert!(chunks.iter().any(|c| c.content.contains("big")));
        // small() should also appear.
        assert!(chunks.iter().any(|c| c.content.contains("small")));
    }
}
