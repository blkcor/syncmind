//! Glob-based file path filtering for `search_knowledge`.
//!
//! `parse_file_filter` accepts the raw user-provided patterns from the desktop
//! RAG Lab chip input (or the MCP `filter_file_type` argument) and produces a
//! compiled `GlobSet`. Bare extensions (no glob metacharacters) are rewritten
//! to `**/*.<ext>` for backwards compatibility with existing callers that pass
//! `["rs", "md"]`.

use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("empty pattern is not allowed")]
    EmptyPattern,
    #[error("invalid glob pattern `{pattern}`: {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

/// A compiled set of glob patterns evaluated against absolute file paths.
///
/// `evaluate(path)` returns `true` if any contained glob matches `path`.
/// Multiple patterns combine with **OR** semantics: a chunk matches if any
/// chip's glob matches its source file. An empty `GlobSet` (zero patterns)
/// matches nothing and should be represented as `None` at the call site.
#[derive(Debug, Clone)]
pub struct FileFilter(GlobSet);

impl FileFilter {
    pub fn evaluate(&self, path: &Path) -> bool {
        self.0.is_match(path)
    }
}

/// Parse the user-provided patterns into a compiled `FileFilter`.
///
/// Returns `Ok(None)` if `patterns` is empty (caller treats this as
/// "no filter"). Returns `Err` on the first invalid pattern.
///
/// **Shorthand:** any pattern without `*`, `?`, `[`, or `{` is treated as a
/// bare file extension and rewritten to `**/*.<pattern>`. This preserves the
/// existing MCP / Tauri callers that pass `["rs", "md"]`.
pub fn parse_file_filter(patterns: &[String]) -> Result<Option<FileFilter>, FilterError> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for raw in patterns {
        if raw.is_empty() {
            return Err(FilterError::EmptyPattern);
        }
        let normalized = if has_glob_meta(raw) {
            raw.clone()
        } else {
            format!("**/*.{}", raw)
        };
        let glob = Glob::new(&normalized).map_err(|source| FilterError::InvalidGlob {
            pattern: raw.clone(),
            source,
        })?;
        builder.add(glob);
    }
    let set = builder.build().map_err(|source| FilterError::InvalidGlob {
        pattern: patterns.join(","),
        source,
    })?;
    Ok(Some(FileFilter(set)))
}

fn has_glob_meta(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '*' | '?' | '[' | '{'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn empty_input_returns_none() {
        assert!(parse_file_filter(&[]).unwrap().is_none());
    }

    #[test]
    fn empty_pattern_returns_err() {
        let err = parse_file_filter(&["".to_string()]).unwrap_err();
        assert!(matches!(err, FilterError::EmptyPattern));
    }

    #[test]
    fn bare_extension_matches_recursive() {
        let filter = parse_file_filter(&["rs".to_string()]).unwrap().unwrap();
        assert!(filter.evaluate(&PathBuf::from("/tmp/main.rs")));
        assert!(filter.evaluate(&PathBuf::from("/tmp/deep/nested/lib.rs")));
        assert!(!filter.evaluate(&PathBuf::from("/tmp/main.md")));
    }

    #[test]
    fn flat_glob_matches_filenames_anywhere() {
        // globset's `*.rs` matches the basename at any depth (gitignore-style),
        // which differs from POSIX shell semantics. This is intentional and
        // matches user expectations from `.gitignore` and ripgrep.
        let filter = parse_file_filter(&["*.rs".to_string()]).unwrap().unwrap();
        assert!(filter.evaluate(&PathBuf::from("main.rs")));
        assert!(filter.evaluate(&PathBuf::from("/abs/path/main.rs")));
        assert!(!filter.evaluate(&PathBuf::from("main.md")));
    }

    #[test]
    fn recursive_glob_matches_anywhere() {
        let filter = parse_file_filter(&["**/*.md".to_string()])
            .unwrap()
            .unwrap();
        assert!(filter.evaluate(&PathBuf::from("/repo/README.md")));
        assert!(filter.evaluate(&PathBuf::from("/repo/docs/sub/page.md")));
        assert!(!filter.evaluate(&PathBuf::from("/repo/main.rs")));
    }

    #[test]
    fn brace_expansion_works() {
        let filter = parse_file_filter(&["src/**/*.{ts,tsx}".to_string()])
            .unwrap()
            .unwrap();
        assert!(filter.evaluate(&PathBuf::from("src/App.tsx")));
        assert!(filter.evaluate(&PathBuf::from("src/lib/x.ts")));
        assert!(!filter.evaluate(&PathBuf::from("src/App.jsx")));
        assert!(!filter.evaluate(&PathBuf::from("test/x.ts")));
    }

    #[test]
    fn multiple_patterns_combine_with_or() {
        let filter =
            parse_file_filter(&["rs".to_string(), "**/*.md".to_string()])
                .unwrap()
                .unwrap();
        assert!(filter.evaluate(&PathBuf::from("/repo/main.rs")));
        assert!(filter.evaluate(&PathBuf::from("/repo/README.md")));
        assert!(!filter.evaluate(&PathBuf::from("/repo/main.py")));
    }

    #[test]
    fn invalid_glob_returns_err_with_pattern() {
        let err = parse_file_filter(&["[unclosed".to_string()]).unwrap_err();
        match err {
            FilterError::InvalidGlob { pattern, .. } => {
                assert_eq!(pattern, "[unclosed");
            }
            other => panic!("expected InvalidGlob, got {other:?}"),
        }
    }
}
