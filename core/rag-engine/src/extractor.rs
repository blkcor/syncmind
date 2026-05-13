use std::fs;
use std::path::Path;

use tracing::warn;

use crate::error::ExtractError;

pub trait Extractor: Send + Sync {
    fn extract(&self, path: &Path) -> Result<String, ExtractError>;
    fn can_handle(&self, path: &Path) -> bool;
    fn clone_box(&self) -> Box<dyn Extractor>;
}

impl Clone for Box<dyn Extractor> {
    fn clone(&self) -> Box<dyn Extractor> {
        self.clone_box()
    }
}

/// Validate that `path` is an absolute regular file.
/// Resolves symlinks to prevent escaping via symlinks.
fn validate_path(path: &Path) -> Result<std::path::PathBuf, ExtractError> {
    if !path.is_absolute() {
        return Err(ExtractError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Path must be absolute: {}", path.display()),
        )));
    }

    let canonical = fs::canonicalize(path).map_err(ExtractError::Io)?;

    let metadata = fs::metadata(&canonical).map_err(ExtractError::Io)?;
    if !metadata.is_file() {
        return Err(ExtractError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Not a regular file: {}", path.display()),
        )));
    }

    Ok(canonical)
}

#[derive(Debug, Clone)]
pub struct MarkdownExtractor;

impl Extractor for MarkdownExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        let path = validate_path(path)?;
        let content = fs::read_to_string(path)?;
        let text = strip_frontmatter(&content);
        Ok(text.to_string())
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false)
    }
}

fn strip_frontmatter(content: &str) -> &str {
    if let Some(after_open) = content.strip_prefix("---\n") {
        if let Some(end) = after_open.find("\n---\n") {
            let after = end + "\n---\n".len();
            return after_open[after..].trim_start();
        }
    }
    if let Some(after_open) = content.strip_prefix("---\r\n") {
        if let Some(end) = after_open.find("\r\n---\r\n") {
            let after = end + "\r\n---\r\n".len();
            return after_open[after..].trim_start();
        }
    }
    content
}

#[derive(Debug, Clone)]
pub struct CodeExtractor;

const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "js", "go", "java", "c", "cpp", "h", "hpp",
    "toml", "json", "yaml", "yml", "sh", "fish", "zsh",
];

impl Extractor for CodeExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        let path = validate_path(path)?;
        fs::read_to_string(path).map_err(Into::into)
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| CODE_EXTENSIONS.iter().any(|&c| c.eq_ignore_ascii_case(ext)))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct PdfExtractor;

impl Extractor for PdfExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        let path = validate_path(path)?;
        let bytes = fs::read(path)?;
        pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|e| ExtractError::Pdf(format!("{e:?}")))
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
    }
}

pub struct CompositeExtractor {
    extractors: Vec<Box<dyn Extractor>>,
}

impl Clone for CompositeExtractor {
    fn clone(&self) -> Self {
        Self::with_extractors(
            self.extractors
                .iter()
                .map(|e| e.clone_box())
                .collect(),
        )
    }
}

impl Default for CompositeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeExtractor {
    pub fn new() -> Self {
        Self {
            extractors: vec![
                Box::new(MarkdownExtractor),
                Box::new(PdfExtractor),
                Box::new(CodeExtractor),
            ],
        }
    }

    pub fn with_extractors(extractors: Vec<Box<dyn Extractor>>) -> Self {
        Self { extractors }
    }
}

impl Extractor for CompositeExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        let candidates: Vec<&dyn Extractor> = self
            .extractors
            .iter()
            .filter(|e| e.can_handle(path))
            .map(|e| e.as_ref())
            .collect();

        let total = candidates.len();
        for (idx, extractor) in candidates.into_iter().enumerate() {
            match extractor.extract(path) {
                Ok(text) => return Ok(text),
                Err(e) => {
                    if idx + 1 < total {
                        warn!(
                            "Extractor failed for {}: {}, continuing search",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
        Err(ExtractError::Unsupported(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string(),
        ))
    }

    fn can_handle(&self, path: &Path) -> bool {
        self.extractors.iter().any(|e| e.can_handle(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_markdown_extracts_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let mut file = fs::File::create(&path).unwrap();
        write!(
            file,
            "---\ntitle: Hello\n---\n# Heading\n\nSome paragraph text.\n\n- item one\n- item two\n"
        )
        .unwrap();

        let extractor = MarkdownExtractor;
        let text = extractor.extract(&path).unwrap();
        assert!(!text.contains("title: Hello"));
        assert!(text.contains("# Heading"));
        assert!(text.contains("Some paragraph text"));
        assert!(text.contains("- item one"));
    }

    #[test]
    fn test_code_extracts_raw_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        fs::write(&path, content).unwrap();

        let extractor = CodeExtractor;
        let text = extractor.extract(&path).unwrap();
        assert_eq!(text, content);
    }

    #[test]
    fn test_pdf_extension_handled() {
        let extractor = PdfExtractor;
        assert!(extractor.can_handle(Path::new("doc.pdf")));
        assert!(!extractor.can_handle(Path::new("doc.txt")));
    }

    #[test]
    fn test_composite_dispatches_by_extension() {
        let dir = tempfile::tempdir().unwrap();

        let md_path = dir.path().join("readme.md");
        fs::write(&md_path, "# Hello\n\nWorld\n").unwrap();

        let rs_path = dir.path().join("lib.rs");
        fs::write(&rs_path, "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();

        let composite = CompositeExtractor::new();

        assert!(composite.can_handle(&md_path));
        let md_text = composite.extract(&md_path).unwrap();
        assert!(md_text.contains("Hello"));
        assert!(md_text.contains("World"));

        assert!(composite.can_handle(&rs_path));
        let rs_text = composite.extract(&rs_path).unwrap();
        assert!(rs_text.contains("pub fn add"));
    }

    #[test]
    fn test_composite_falls_back_on_failure() {
        #[derive(Clone)]
        struct FailingExtractor;
        impl Extractor for FailingExtractor {
            fn clone_box(&self) -> Box<dyn Extractor> {
                Box::new(self.clone())
            }
            fn extract(&self, _path: &Path) -> Result<String, ExtractError> {
                Err(ExtractError::Unsupported("fail".to_string()))
            }
            fn can_handle(&self, path: &Path) -> bool {
                path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("rs"))
                    .unwrap_or(false)
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lib.rs");
        fs::write(&path, "fn main() {}\n").unwrap();

        let composite = CompositeExtractor::with_extractors(vec![
            Box::new(FailingExtractor),
            Box::new(CodeExtractor),
        ]);

        let text = composite.extract(&path).unwrap();
        assert_eq!(text, "fn main() {}\n");
    }

    #[test]
    fn test_composite_unsupported_error() {
        let composite = CompositeExtractor::new();
        let result = composite.extract(Path::new("image.png"));
        assert!(matches!(result, Err(ExtractError::Unsupported(_))));
    }

    #[test]
    fn test_pdf_extractor_rejects_garbage_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("garbage.pdf");
        fs::write(&path, b"not a pdf").unwrap();

        let extractor = PdfExtractor;
        let result = extractor.extract(&path);
        assert!(
            matches!(result, Err(ExtractError::Pdf(_))),
            "expected Pdf error for garbage bytes, got {:?}",
            result
        );
    }

    #[test]
    fn test_frontmatter_no_false_positive_for_horizontal_rule() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        fs::write(&path, "---\n\nSome text after a horizontal rule.\n").unwrap();

        let extractor = MarkdownExtractor;
        let text = extractor.extract(&path).unwrap();
        assert!(text.contains("Some text after a horizontal rule"));
    }

    #[test]
    fn test_frontmatter_windows_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("win.md");
        fs::write(&path, "---\r\ntitle: Hello\r\n---\r\n# Heading\r\n").unwrap();

        let extractor = MarkdownExtractor;
        let text = extractor.extract(&path).unwrap();
        assert!(!text.contains("title: Hello"));
        assert!(text.contains("# Heading"));
    }
}
