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
                context_prefix: None,
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

        // Paragraph-aware: split into paragraphs by blank lines first.
        let mut paragraphs: Vec<(usize, Vec<&str>)> = Vec::new(); // (start_line, lines)
        let mut cur_start: Option<usize> = None;
        let mut cur_lines: Vec<&str> = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                if !cur_lines.is_empty() {
                    paragraphs.push((cur_start.unwrap(), std::mem::take(&mut cur_lines)));
                }
                cur_start = None;
            } else {
                if cur_start.is_none() {
                    cur_start = Some(idx + 1); // 1-indexed
                }
                cur_lines.push(*line);
            }
        }
        if !cur_lines.is_empty() {
            paragraphs.push((cur_start.unwrap(), cur_lines));
        }

        if paragraphs.is_empty() {
            return Vec::new();
        }

        // Chunk paragraphs together up to chunk_size.
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut chunk_idx = 0usize;
        let mut para_idx = 0usize;

        while para_idx < paragraphs.len() {
            let (p_start, _) = paragraphs[para_idx];
            let mut accum = String::new();
            let start = para_idx;

            while para_idx < paragraphs.len() {
                let para_text = paragraphs[para_idx].1.join("\n");
                let added = if accum.is_empty() {
                    para_text.len()
                } else {
                    para_text.len() + 2 // "\n\n" separator
                };

                if !accum.is_empty() && accum.len() + added > self.chunk_size {
                    break;
                }

                if !accum.is_empty() {
                    accum.push_str("\n\n");
                }
                accum.push_str(&para_text);
                para_idx += 1;

                // Single oversized paragraph → fall back to line-based splitting
                if accum.len() > self.chunk_size && para_idx == start + 1 {
                    break;
                }
            }

            if para_idx == start + 1 && accum.len() > self.chunk_size {
                // Oversized single paragraph: line-based chunking
                let (offset, ref para_lines) = paragraphs[start];
                let line_refs: Vec<&str> = para_lines.to_vec();
                for mut c in self.chunk_lines(&line_refs, offset) {
                    c.chunk_index = chunk_idx;
                    chunk_idx += 1;
                    chunks.push(c);
                }
            } else {
                let (end_offset, _) = paragraphs[para_idx.saturating_sub(1)];
                let end_line = end_offset + paragraphs[para_idx.saturating_sub(1)].1.len().saturating_sub(1);
                chunks.push(Chunk {
                    chunk_index: chunk_idx,
                    start_line: p_start,
                    end_line,
                    content: accum,
                    context_prefix: None,
                });
                chunk_idx += 1;
            }
        }

        chunks
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
            Some("js") | Some("jsx") => Some("javascript"),
            Some("ts") => Some("typescript"),
            Some("tsx") => Some("tsx"),
            Some("go") => Some("go"),
            Some("java") => Some("java"),
            Some("c") | Some("h") => Some("c"),
            Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") | Some("hh") | Some("hxx") => {
                Some("cpp")
            }
            Some("cs") => Some("c_sharp"),
            Some("rb") => Some("ruby"),
            Some("php") => Some("php"),
            Some("swift") => Some("swift"),
            Some("kt") | Some("kts") => Some("kotlin"),
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
            "typescript" | "tsx" => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
                "abstract_method_signature",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "lexical_declaration",
            ],
            "go" => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
            ],
            "java" => &[
                "class_declaration",
                "interface_declaration",
                "enum_declaration",
                "constructor_declaration",
                "method_declaration",
                "field_declaration",
            ],
            "c" => &[
                "function_definition",
                "struct_specifier",
                "union_specifier",
                "enum_specifier",
                "declaration",
            ],
            "cpp" => &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
                "union_specifier",
                "enum_specifier",
                "namespace_definition",
                "declaration",
            ],
            "c_sharp" => &[
                "namespace_declaration",
                "file_scoped_namespace_declaration",
                "class_declaration",
                "struct_declaration",
                "interface_declaration",
                "enum_declaration",
                "constructor_declaration",
                "method_declaration",
                "property_declaration",
                "field_declaration",
            ],
            "ruby" => &[
                "module",
                "class",
                "method",
                "singleton_method",
                "assignment",
                "call",
            ],
            "php" => &[
                "class_declaration",
                "interface_declaration",
                "trait_declaration",
                "enum_declaration",
                "function_definition",
                "method_declaration",
                "property_declaration",
            ],
            "swift" => &[
                "class_declaration",
                "struct_declaration",
                "protocol_declaration",
                "enum_declaration",
                "function_declaration",
                "property_declaration",
                "extension_declaration",
            ],
            "kotlin" => &[
                "class_declaration",
                "object_declaration",
                "function_declaration",
                "property_declaration",
                "type_alias",
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
            "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            "go" => tree_sitter_go::LANGUAGE.into(),
            "java" => tree_sitter_java::LANGUAGE.into(),
            "c" => tree_sitter_c::LANGUAGE.into(),
            "cpp" => tree_sitter_cpp::LANGUAGE.into(),
            "c_sharp" => tree_sitter_c_sharp::LANGUAGE.into(),
            "ruby" => tree_sitter_ruby::LANGUAGE.into(),
            "php" => tree_sitter_php::LANGUAGE_PHP.into(),
            "swift" => tree_sitter_swift::LANGUAGE.into(),
            "kotlin" => tree_sitter_kotlin_ng::LANGUAGE.into(),
            _ => return Err(ChunkError::Parse(format!("unsupported language: {lang}"))),
        };
        parser
            .set_language(&language)
            .map_err(|e| ChunkError::Parse(format!("parser set_language failed: {e:?}")))?;

        let tree = parser
            .parse(text, None)
            .ok_or_else(|| ChunkError::Parse("tree-sitter parse returned None".to_string()))?;

        let root = tree.root_node();
        if root.has_error() {
            return Err(ChunkError::Parse(format!(
                "tree-sitter parse produced errors for language: {lang}"
            )));
        }

        let types = Self::node_types_for_language(lang);
        let mut nodes: Vec<tree_sitter::Node> = Vec::new();
        Self::collect_nodes(root, types, &mut nodes);
        nodes = Self::remove_contained_nodes(nodes);

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
                context_prefix: None,
            });
        }

        Ok(chunks)
    }

    fn remove_contained_nodes<'a>(
        nodes: Vec<tree_sitter::Node<'a>>,
    ) -> Vec<tree_sitter::Node<'a>> {
        let mut filtered = Vec::new();

        'node: for (idx, node) in nodes.iter().enumerate() {
            for (other_idx, other) in nodes.iter().enumerate() {
                if idx == other_idx {
                    continue;
                }
                if other.start_byte() <= node.start_byte()
                    && other.end_byte() >= node.end_byte()
                    && (other.start_byte(), other.end_byte()) != (node.start_byte(), node.end_byte())
                {
                    continue 'node;
                }
            }
            filtered.push(*node);
        }

        filtered.sort_by_key(|node| node.start_byte());
        filtered
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
            if let Some(child) = node.child(i as u32) {
                Self::collect_nodes(child, types, out);
            }
        }
    }

    /// Extract the signature (declaration up to the opening `{`) from a code block.
    /// For single-line declarations without `{`, returns the first line.
    fn extract_signature(content: &str) -> String {
        let mut sig = String::new();
        for line in content.lines() {
            sig.push_str(line);
            if line.contains('{') {
                break;
            }
            sig.push('\n');
        }
        sig.trim_end().to_string()
    }

    /// Split oversized content at blank-line boundaries, falling back to
    /// `FallbackChunker` for individual paragraphs that still exceed the limit.
    /// Prepends `signature` to every sub-chunk so semantic context is preserved.
    fn chunk_semantically(
        content: &str,
        start_line: usize,
        chunk_size: usize,
        chunk_overlap: usize,
        signature: Option<&str>,
    ) -> Vec<Chunk> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let context_prefix = signature.map(|s| s.to_string());
        let effective_size = chunk_size;

        // --- split into paragraphs separated by blank lines ---
        let mut paragraphs: Vec<(usize, Vec<&str>)> = Vec::new();
        let mut cur_offset: Option<usize> = None;
        let mut cur_lines: Vec<&str> = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                if !cur_lines.is_empty() {
                    paragraphs.push((cur_offset.unwrap(), std::mem::take(&mut cur_lines)));
                }
                cur_offset = None;
            } else {
                if cur_offset.is_none() {
                    cur_offset = Some(idx);
                }
                cur_lines.push(*line);
            }
        }
        if !cur_lines.is_empty() {
            paragraphs.push((cur_offset.unwrap(), cur_lines));
        }

        // If there are no paragraph boundaries, fallback directly.
        if paragraphs.is_empty() {
            let fb = FallbackChunker::new(chunk_size, chunk_overlap);
            let mut chunks = fb.chunk_lines(&lines, start_line);
            for c in &mut chunks {
                c.context_prefix.clone_from(&context_prefix);
            }
            return chunks;
        }

        let fb = FallbackChunker::new(chunk_size, chunk_overlap);
        let mut all_chunks: Vec<Chunk> = Vec::new();
        let mut chunk_idx = 0usize;
        let mut i = 0usize;

        while i < paragraphs.len() {
            let mut accum = String::new();
            let mut j = i;
            let para_start_offset = paragraphs[i].0;

            while j < paragraphs.len() {
                let para_text = paragraphs[j].1.join("\n");
                let added = if accum.is_empty() {
                    para_text.len()
                } else {
                    para_text.len() + 1 // blank-line separator
                };

                // Would exceed limit and we already have content → stop
                if !accum.is_empty() && accum.len() + added > effective_size {
                    break;
                }

                if !accum.is_empty() {
                    accum.push('\n');
                }
                accum.push_str(&para_text);
                j += 1;

                // Single paragraph already too big → handle below
                if accum.len() > effective_size && j == i + 1 {
                    break;
                }
            }

            // Case: single paragraph exceeds limit → fallback chunk that paragraph
            if j == i + 1 && accum.len() > effective_size {
                let (offset, ref para_lines) = paragraphs[i];
                let para_start_line = start_line + offset;
                let line_refs: Vec<&str> = para_lines.to_vec();
                let mut sub = fb.chunk_lines(&line_refs, para_start_line);
                for c in &mut sub {
                    c.context_prefix.clone_from(&context_prefix);
                    c.chunk_index = chunk_idx;
                    chunk_idx += 1;
                }
                all_chunks.append(&mut sub);
                i += 1;
                continue;
            }

            // Normal case: build chunk from accumulated paragraphs
            let end_offset = if j > 0 {
                let last = &paragraphs[j - 1];
                last.0 + last.1.len().saturating_sub(1)
            } else {
                para_start_offset
            };

            all_chunks.push(Chunk {
                chunk_index: chunk_idx,
                start_line: start_line + para_start_offset,
                end_line: start_line + end_offset,
                content: accum,
                context_prefix: context_prefix.clone(),
            });
            chunk_idx += 1;

            // Advance with overlap
            if chunk_overlap == 0 || j >= paragraphs.len() {
                i = j;
                continue;
            }

            let mut overlap_chars = 0usize;
            let mut new_i = j;
            while new_i > i && overlap_chars < chunk_overlap {
                new_i -= 1;
                let para_text = paragraphs[new_i].1.join("\n");
                overlap_chars += para_text.len() + 1;
            }
            i = if new_i == i { j } else { new_i };
        }

        all_chunks
    }
}

impl Chunker for CodeChunker {
    fn chunk(&self, text: &str, path: &Path) -> Vec<Chunk> {
        if text.is_empty() {
            return Vec::new();
        }

        let fallback = || FallbackChunker::new(self.chunk_size, self.chunk_overlap).chunk(text, path);

        let Some(lang) = Self::language_from_extension(path) else {
            tracing::warn!(path = %path.display(), "unsupported code language, falling back");
            return fallback();
        };

        let raw_chunks = match Self::parse_with_tree_sitter(text, lang) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => return fallback(),
            Err(e) => {
                tracing::warn!(path = %path.display(), language = lang, error = %e, "tree-sitter parse failed, falling back");
                return fallback();
            }
        };

        let mut all_chunks: Vec<Chunk> = Vec::new();
        let mut global_idx = 0usize;

        for mut c in raw_chunks {
            if c.content.len() > self.chunk_size {
                let signature = Self::extract_signature(&c.content);
                let sub_chunks = Self::chunk_semantically(
                    &c.content,
                    c.start_line,
                    self.chunk_size,
                    self.chunk_overlap,
                    Some(&signature),
                );
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

// ── CssChunker ───────────────────────────────────────────────────────────────

pub struct CssChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl CssChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }

    /// Extract text before the first `{` as the selector.
    fn extract_selector(rule: &str) -> String {
        let mut selector = String::new();
        for ch in rule.chars() {
            if ch == '{' {
                break;
            }
            selector.push(ch);
        }
        selector.trim().to_string()
    }

    fn update_brace_depth(line: &str, depth: &mut i32) {
        for ch in line.chars() {
            match ch {
                '{' => *depth += 1,
                '}' => *depth -= 1,
                _ => {}
            }
        }
    }

    fn split_rule_units<'a>(lines: &[&'a str]) -> Vec<(usize, Vec<&'a str>)> {
        let mut units = Vec::new();
        let mut depth = 0i32;
        let mut wrapper_started = false;
        let mut current_start = 0usize;
        let mut current: Vec<&'a str> = Vec::new();
        let mut current_has_nested_block = false;

        for (idx, line) in lines.iter().enumerate() {
            if !wrapper_started {
                Self::update_brace_depth(line, &mut depth);
                if depth > 0 {
                    wrapper_started = true;
                }
                continue;
            }

            let before_depth = depth;
            let trimmed = line.trim();
            if before_depth == 1 && trimmed == "}" {
                Self::update_brace_depth(line, &mut depth);
                continue;
            }
            if current.is_empty() && trimmed.is_empty() {
                continue;
            }

            if current.is_empty() {
                current_start = idx;
            }
            current.push(*line);

            Self::update_brace_depth(line, &mut depth);
            if before_depth >= 1 && depth > 1 {
                current_has_nested_block = true;
            }

            let completed_nested_block = current_has_nested_block && depth == 1;
            let completed_declaration = !current_has_nested_block
                && depth == 1
                && trimmed.ends_with(';');
            if completed_nested_block || completed_declaration {
                units.push((current_start, std::mem::take(&mut current)));
                current_has_nested_block = false;
            }
        }

        if !current.is_empty() {
            units.push((current_start, current));
        }

        units
    }

    fn chunks_from_rule_units(
        &self,
        units: Vec<(usize, Vec<&str>)>,
        start_line: usize,
        context: &str,
        effective_size: usize,
        chunk_index: &mut usize,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut accum = String::new();
        let mut accum_start = 0usize;
        let mut accum_end = 0usize;

        for (unit_start, unit_lines) in units {
            let unit_text = unit_lines.join("\n");
            let unit_end = unit_start + unit_lines.len().saturating_sub(1);
            let candidate_len = if accum.is_empty() {
                unit_text.len()
            } else {
                accum.len() + 1 + unit_text.len()
            };

            if !accum.is_empty() && candidate_len > effective_size {
                chunks.push(Chunk {
                    chunk_index: *chunk_index,
                    start_line: start_line + accum_start,
                    end_line: start_line + accum_end,
                    content: format!("{}{}", context, accum),
                    context_prefix: None,
                });
                *chunk_index += 1;
                accum.clear();
            }

            if accum.is_empty() {
                accum_start = unit_start;
            } else {
                accum.push('\n');
            }
            accum.push_str(&unit_text);
            accum_end = unit_end;
        }

        if !accum.trim().is_empty() {
            chunks.push(Chunk {
                chunk_index: *chunk_index,
                start_line: start_line + accum_start,
                end_line: start_line + accum_end,
                content: format!("{}{}", context, accum),
                context_prefix: None,
            });
            *chunk_index += 1;
        }

        chunks
    }

    /// Split a single oversized rule at `;` boundaries, prefixing each
    /// sub-chunk with a CSS comment containing the selector context.
    fn sub_chunk_rule(
        &self,
        rule_text: &str,
        start_line: usize,
        selector: &str,
        chunk_index: &mut usize,
    ) -> Vec<Chunk> {
        let lines: Vec<&str> = rule_text.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let fb = FallbackChunker::new(self.chunk_size, self.chunk_overlap);
        let context = format!("/* context: {} */\n", selector);
        let context_len = context.len();
        let effective_size = self.chunk_size.saturating_sub(context_len);

        let units = Self::split_rule_units(&lines);
        if units.len() > 1 {
            let chunks = self.chunks_from_rule_units(
                units,
                start_line,
                &context,
                effective_size,
                chunk_index,
            );
            if !chunks.is_empty() {
                return chunks;
            }
        }

        // Build declarations and chunk them respecting effective_size
        let mut chunks = Vec::new();
        let mut accum = String::with_capacity(effective_size);
        let mut accum_start = 0usize;

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            // Skip the opening brace and closing brace lines if they're alone
            if trimmed == "{" {
                continue;
            }

            let candidate_len = if accum.is_empty() {
                trimmed.len()
            } else {
                accum.len() + 1 + trimmed.len()
            };

            if !accum.is_empty() && candidate_len > effective_size {
                // Emit chunk
                chunks.push(Chunk {
                    chunk_index: *chunk_index,
                    start_line: start_line + accum_start,
                    end_line: start_line + idx.saturating_sub(1),
                    content: format!("{}{}", context, accum),
                    context_prefix: None,
                });
                *chunk_index += 1;
                accum.clear();
                accum_start = idx;
            }

            if !accum.is_empty() {
                accum.push('\n');
            }
            accum.push_str(line);
        }

        // Flush remainder
        if !accum.trim().is_empty() && accum.trim() != "}" {
            chunks.push(Chunk {
                chunk_index: *chunk_index,
                start_line: start_line + accum_start,
                end_line: start_line + lines.len().saturating_sub(1),
                content: format!("{}{}", context, accum),
                context_prefix: None,
            });
            *chunk_index += 1;
        }

        // If sub_chunk produced nothing, use the fallback
        if chunks.is_empty() {
            let fb_chunks = fb.chunk_lines(&lines, start_line);
            for mut c in fb_chunks {
                c.chunk_index = *chunk_index;
                c.context_prefix = Some(format!("/* context: {} */", selector));
                *chunk_index += 1;
                chunks.push(c);
            }
        }

        chunks
    }

    /// Split CSS text by rule boundaries (`}`) with brace-depth tracking for
    /// nested rules (SCSS/Less). Each top-level rule becomes one or more chunks.
    pub fn chunk_css(&self, text: &str) -> Vec<Chunk> {
        if text.trim().is_empty() {
            return Vec::new();
        }

        // Split into rules by tracking brace depth
        let mut rules: Vec<(usize, String)> = Vec::new(); // (start_line, rule_text)
        let lines: Vec<&str> = text.lines().collect();
        let mut depth: i32 = 0;
        let mut rule_start: Option<usize> = None;
        let mut current_rule = String::new();

        for (idx, line) in lines.iter().enumerate() {
            if rule_start.is_none() && !line.trim().is_empty() {
                rule_start = Some(idx + 1); // 1-indexed
            }

            if let Some(_start) = rule_start {
                if !current_rule.is_empty() {
                    current_rule.push('\n');
                }
                current_rule.push_str(line);

                // Track brace depth
                for ch in line.chars() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                // Top-level rule ended
                                rules.push((rule_start.unwrap(), std::mem::take(&mut current_rule)));
                                rule_start = None;
                            }
                        }
                        _ => {}
                    }
                }

                // Top-level at-rules such as @charset and @import do not open
                // a block, but they are meaningful standalone stylesheet units.
                if depth == 0 && line.trim_end().ends_with(';') {
                    rules.push((rule_start.unwrap(), std::mem::take(&mut current_rule)));
                    rule_start = None;
                }
            }
        }

        // Any leftover (unclosed rule) is treated as a complete rule
        if !current_rule.trim().is_empty() {
            if let Some(start) = rule_start {
                rules.push((start, current_rule));
            }
        }

        let mut chunks = Vec::new();
        let mut chunk_idx = 0usize;

        for (rule_start_line, rule_text) in rules {
            let rule_line_count = rule_text.lines().count();
            let selector = Self::extract_selector(&rule_text);

            if rule_text.len() <= self.chunk_size {
                chunks.push(Chunk {
                    chunk_index: chunk_idx,
                    start_line: rule_start_line,
                    end_line: rule_start_line + rule_line_count.saturating_sub(1),
                    content: rule_text,
                    context_prefix: None,
                });
                chunk_idx += 1;
            } else {
                // Oversized rule: sub-chunk at declaration boundaries
                let mut sub = self.sub_chunk_rule(
                    &rule_text,
                    rule_start_line,
                    &selector,
                    &mut chunk_idx,
                );
                chunks.append(&mut sub);
            }
        }

        chunks
    }
}

impl Chunker for CssChunker {
    fn chunk(&self, text: &str, _path: &Path) -> Vec<Chunk> {
        self.chunk_css(text)
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

    #[test]
    fn test_code_chunker_go_functions() {
        let code = r#"
package main

func Foo() int {
    return 1
}

func Bar(x string) string {
    return x
}
"#;
        let chunker = CodeChunker::new(200, 20);
        let chunks = chunker.chunk(code, Path::new("test.go"));
        assert!(
            chunks.len() >= 2,
            "expected at least two chunks for two functions, got {}",
            chunks.len()
        );
        let contents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        assert!(contents.iter().any(|c| c.contains("Foo")));
        assert!(contents.iter().any(|c| c.contains("Bar")));
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
            assert!(c.start_line >= 1);
        }
    }

    #[test]
    fn test_code_chunker_go_type_spec_keeps_type_keyword() {
        let code = r#"
package main

type rect struct {
	width, height float64
}

func main() {}
"#;
        let chunker = CodeChunker::new(1_000, 0);
        let chunks = chunker.chunk(code, Path::new("main.go"));

        let type_chunk = chunks
            .iter()
            .find(|chunk| chunk.content.contains("rect struct"))
            .expect("expected rect type chunk");
        assert!(
            type_chunk.content.trim_start().starts_with("type rect struct"),
            "Go type chunk should include the full type definition, got: {}",
            type_chunk.content
        );
    }

    #[test]
    fn test_code_chunker_added_languages_use_language_boundaries() {
        let cases = [
            (
                "Example.java",
                r#"
public class Example {
    private int value;

    public Example(int value) {
        this.value = value;
    }

    public int value() {
        return value;
    }
}

interface Named {
    String name();
}
"#,
                &["class Example", "value()"][..],
                2,
            ),
            (
                "example.c",
                r#"
struct Point {
    int x;
    int y;
};

int add(int a, int b) {
    return a + b;
}
"#,
                &["struct Point", "add(int"][..],
                2,
            ),
            (
                "example.cpp",
                r#"
namespace math {
class Calculator {
public:
    int add(int a, int b) {
        return a + b;
    }
};
}

int free_value() {
    return 1;
}
"#,
                &["namespace math", "free_value"][..],
                2,
            ),
            (
                "Example.cs",
                r#"
namespace Demo;

public class Example {
    public int Value { get; }

    public Example(int value) {
        Value = value;
    }
}

public interface Named {
    string Name { get; }
}
"#,
                &["class Example", "Value"][..],
                2,
            ),
            (
                "example.rb",
                r#"
module Demo
  class Example
    def value
      42
    end
  end
end

def outside
  1
end
"#,
                &["module Demo", "def outside"][..],
                2,
            ),
            (
                "example.php",
                r#"
<?php
class Example {
    public function value() {
        return 42;
    }
}

function outside() {
    return 1;
}
"#,
                &["class Example", "function outside"][..],
                2,
            ),
            (
                "Example.swift",
                r#"
struct Example {
    let value: Int

    func doubled() -> Int {
        value * 2
    }
}

func outside() -> Int {
    1
}
"#,
                &["struct Example", "func outside"][..],
                2,
            ),
            (
                "Example.kt",
                r#"
class Example(private val value: Int) {
    fun doubled(): Int {
        return value * 2
    }
}

fun outside(): Int {
    return 1
}
"#,
                &["class Example", "fun outside"][..],
                2,
            ),
        ];

        let chunker = CodeChunker::new(1_000, 0);

        for (path, code, expected_fragments, expected_min_chunks) in cases {
            let chunks = chunker.chunk(code, Path::new(path));
            assert!(
                chunks.len() >= expected_min_chunks,
                "expected at least {expected_min_chunks} language-aware chunks for {path}, got {chunks:#?}"
            );
            for fragment in expected_fragments {
                assert!(
                    chunks.iter().any(|c| c.content.contains(fragment)),
                    "expected {path} chunks to contain `{fragment}`, got {chunks:#?}"
                );
            }
        }
    }

    #[test]
    fn test_code_chunker_added_extension_aliases() {
        let cases = [
            ("header.h", "int header_value(void) {\n    return 1;\n}\n", "header_value"),
            ("source.cc", "int cc_value() {\n    return 1;\n}\n", "cc_value"),
            ("source.cxx", "int cxx_value() {\n    return 1;\n}\n", "cxx_value"),
            ("header.hpp", "class HeaderValue {\npublic:\n    int get() { return 1; }\n};\n", "HeaderValue"),
            ("header.hh", "class HhValue {\npublic:\n    int get() { return 1; }\n};\n", "HhValue"),
            ("header.hxx", "class HxxValue {\npublic:\n    int get() { return 1; }\n};\n", "HxxValue"),
            ("script.kts", "fun scriptValue(): Int {\n    return 1\n}\n", "scriptValue"),
        ];

        let chunker = CodeChunker::new(1_000, 0);

        for (path, code, expected) in cases {
            let chunks = chunker.chunk(code, Path::new(path));
            assert!(
                chunks.iter().any(|c| c.content.contains(expected)),
                "expected {path} to use a parser chunk containing `{expected}`, got {chunks:#?}"
            );
        }
    }

    #[test]
    fn test_existing_code_languages_still_use_language_boundaries() {
        let cases = [
            ("test.rs", "fn first() {}\n\nfn second() {}\n", &["first", "second"][..]),
            (
                "test.py",
                "def first():\n    return 1\n\nclass Second:\n    pass\n",
                &["def first", "class Second"][..],
            ),
            (
                "test.js",
                "function first() {}\n\nclass Second {}\n",
                &["function first", "class Second"][..],
            ),
            (
                "test.ts",
                "function first(): number { return 1; }\n\nclass Second {}\n",
                &["function first", "class Second"][..],
            ),
            (
                "test.go",
                "package main\n\nfunc First() int { return 1 }\n\nfunc Second() int { return 2 }\n",
                &["func First", "func Second"][..],
            ),
        ];

        let chunker = CodeChunker::new(1_000, 0);

        for (path, code, expected_fragments) in cases {
            let lang = CodeChunker::language_from_extension(Path::new(path))
                .unwrap_or_else(|| panic!("expected language mapping for {path}"));
            let parser_chunks = CodeChunker::parse_with_tree_sitter(code, lang)
                .unwrap_or_else(|e| panic!("expected parser chunks for {path}: {e}"));
            assert!(
                !parser_chunks.is_empty(),
                "expected parser chunks for {path}"
            );
            let chunks = chunker.chunk(code, Path::new(path));
            for fragment in expected_fragments {
                assert!(
                    chunks.iter().any(|c| c.content.contains(fragment)),
                    "expected {path} chunks to contain `{fragment}`, got {chunks:#?}"
                );
            }
        }
    }

    #[test]
    fn test_supported_language_without_boundaries_falls_back_per_file() {
        let text = "not valid rust declarations\nbut still index this file";
        let chunker = CodeChunker::new(20, 2);
        let chunks = chunker.chunk(text, Path::new("broken.rs"));
        let fb = FallbackChunker::new(20, 2);
        let fb_chunks = fb.chunk(text, Path::new("broken.rs"));
        assert_eq!(chunks.len(), fb_chunks.len());
        for (a, b) in chunks.iter().zip(fb_chunks.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn test_semantic_sub_chunking_preserves_signature() {
        // A large Go function with blank lines between logical sections.
        let code = r#"func BigFunc() {
    sectionA()

    sectionB()

    sectionC()

    sectionD()
}"#;
        let chunker = CodeChunker::new(40, 5);
        let chunks = chunker.chunk(code, Path::new("big.go"));
        assert!(
            chunks.len() >= 2,
            "expected semantic split, got {} chunks",
            chunks.len()
        );
        // Every sub-chunk of BigFunc should carry the signature in context_prefix.
        // Content stays pristine: the first sub-chunk naturally includes the
        // signature line, later sub-chunks do NOT have it prepended.
        let mut saw_signature_in_content = false;
        for c in &chunks {
            if c.content.contains("section") {
                assert_eq!(
                    c.context_prefix.as_deref(),
                    Some("func BigFunc() {"),
                    "sub-chunk should preserve signature in context_prefix"
                );
                if c.content.contains("func BigFunc()") {
                    saw_signature_in_content = true;
                }
            }
        }
        // At least the first sub-chunk naturally contains the signature line.
        assert!(saw_signature_in_content);
    }

    #[test]
    fn test_semantic_sub_chunking_line_numbers() {
        let code = "fn a() {\n    1\n\n    2\n\n    3\n\n    4\n}\n";
        let chunker = CodeChunker::new(30, 3);
        let chunks = chunker.chunk(code, Path::new("lines.rs"));
        assert!(
            !chunks.is_empty(),
            "expected at least one chunk"
        );
        // Verify sequential line numbers.
        for c in &chunks {
            assert!(c.start_line >= 1, "start_line should be >= 1");
            assert!(c.end_line >= c.start_line, "end_line >= start_line");
        }
    }

    // ── CssChunker tests ─────────────────────────────────────────────────

    #[test]
    fn test_css_chunker_rule_boundaries() {
        let css = ".card { color: red; }\n.button { background: blue; }\n.link { text-decoration: none; }";
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk(css, Path::new("test.css"));
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].content.contains(".card"));
        assert!(chunks[1].content.contains(".button"));
        assert!(chunks[2].content.contains(".link"));
    }

    #[test]
    fn test_css_chunker_empty_input() {
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk("   \n  ", Path::new("empty.css"));
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_css_chunker_scss_nested_rule() {
        let scss = ".card {\n  color: red;\n  &:hover {\n    color: blue;\n  }\n}\n";
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk(scss, Path::new("test.scss"));
        // Nested rule should stay inside the parent rule, so we get 1 chunk.
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("&:hover"));
        assert!(chunks[0].content.contains(".card"));
    }

    #[test]
    fn test_css_chunker_oversized_rule_sub_chunking() {
        let mut css = String::from(".big-rule {\n");
        for i in 0..50 {
            css.push_str(&format!("  property{}: value{};\n", i, i));
        }
        css.push_str("}\n");
        let chunker = CssChunker::new(100, 10);
        let chunks = chunker.chunk(&css, Path::new("big.css"));
        assert!(
            chunks.len() >= 2,
            "oversized rule should be sub-chunked, got {} chunks",
            chunks.len()
        );
        // Each sub-chunk should contain the selector context.
        for c in &chunks {
            assert!(
                c.content.contains(".big-rule"),
                "sub-chunk should have selector context: {}",
                c.content
            );
        }
    }

    #[test]
    fn test_css_chunker_preserves_top_level_imports() {
        let css = "@charset \"UTF-8\";\n\n@import './variable.scss';\n\n.card { color: red; }\n";
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk(css, Path::new("animate.scss"));

        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.content.contains("@import './variable.scss';")),
            "top-level @import should be preserved as searchable stylesheet content"
        );
    }

    #[test]
    fn test_css_chunker_keyframes_sub_chunks_keep_blocks_complete() {
        let css = r#"@keyframes bounce {
    from,
    20%,
    53%,
    80%,
    to {
        -webkit-animation-timing-function: cubic-bezier(0.215, 0.61, 0.355, 1);
        animation-timing-function: cubic-bezier(0.215, 0.61, 0.355, 1);
        -webkit-transform: translate3d(0, 0, 0);
        transform: translate3d(0, 0, 0);
    }

    40%,
    43% {
        -webkit-animation-timing-function: cubic-bezier(0.755, 0.05, 0.855, 0.06);
        animation-timing-function: cubic-bezier(0.755, 0.05, 0.855, 0.06);
        -webkit-transform: translate3d(0, -30px, 0);
        transform: translate3d(0, -30px, 0);
    }
}"#;
        let chunker = CssChunker::new(220, 10);
        let chunks = chunker.chunk(css, Path::new("animate.scss"));

        assert!(
            chunks.len() >= 2,
            "oversized keyframes should split into complete step chunks"
        );
        assert!(
            chunks[0].content.contains("from,")
                && chunks[0].content.contains("to {")
                && chunks[0].content.contains("transform: translate3d(0, 0, 0);")
                && chunks[0].content.contains("}"),
            "first keyframe step should remain complete: {}",
            chunks[0].content
        );
        for chunk in chunks {
            let balance: i32 = chunk
                .content
                .chars()
                .map(|ch| match ch {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                })
                .sum();
            assert_eq!(balance, 0, "CSS sub-chunk braces should balance: {}", chunk.content);
        }
    }

    #[test]
    fn test_css_chunker_line_numbers() {
        let css = "/* comment */\n.card { color: red; }\n.button { background: blue; }";
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk(css, Path::new("test.css"));
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.start_line >= 1, "start_line should be >= 1");
            assert!(c.end_line >= c.start_line, "end_line >= start_line");
        }
    }

    #[test]
    fn test_css_chunker_sequential_indices() {
        let css = ".a { x: 1; }\n.b { y: 2; }\n.c { z: 3; }";
        let chunker = CssChunker::new(500, 10);
        let chunks = chunker.chunk(css, Path::new("test.css"));
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
        }
    }
}
