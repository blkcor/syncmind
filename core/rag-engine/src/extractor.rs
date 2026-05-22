use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::warn;

use crate::error::ExtractError;
use syncmind_core::{Config, OcrMode};

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
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "c", "h", "cpp", "cc", "cxx", "hpp",
    "hh", "hxx", "cs", "rb", "php", "swift", "kt", "kts", "toml", "json", "yaml", "yml",
    "sh", "fish", "zsh",
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

#[derive(Debug, Clone, Default)]
pub struct PdfExtractor {
    ocr: OcrConfig,
}

impl PdfExtractor {
    pub fn new(ocr: OcrConfig) -> Self {
        Self { ocr }
    }
}

impl Extractor for PdfExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        let path = validate_path(path)?;
        let bytes = fs::read(&path)?;
        if self.ocr.mode != OcrMode::Force {
            match pdf_extract::extract_text_from_mem(&bytes) {
                Ok(text) if self.ocr.mode == OcrMode::Disabled || text_quality(&text) >= self.ocr.pdf_text_quality_threshold => {
                    return Ok(text);
                }
                Ok(text) if self.ocr.mode == OcrMode::Auto => {
                    match run_pdf_ocr(&path, &self.ocr) {
                        Ok(ocr_text) => return Ok(ocr_text),
                        Err(e) if !text.trim().is_empty() => {
                            warn!(path = %path.display(), error = %e, "PDF OCR fallback unavailable; preserving embedded text");
                            return Ok(text);
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(text) => return Ok(text),
                Err(e) if self.ocr.mode == OcrMode::Auto => {
                    match run_pdf_ocr(&path, &self.ocr) {
                        Ok(ocr_text) => return Ok(ocr_text),
                        Err(ocr_err) => {
                            return Err(ExtractError::Pdf(format!(
                                "embedded extraction failed: {e:?}; OCR fallback failed: {ocr_err}"
                            )));
                        }
                    }
                }
                Err(e) => return Err(ExtractError::Pdf(format!("{e:?}"))),
            }
        }

        run_pdf_ocr(&path, &self.ocr)
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct ImageOcrExtractor {
    ocr: OcrConfig,
}

impl ImageOcrExtractor {
    pub fn new(ocr: OcrConfig) -> Self {
        Self { ocr }
    }
}

impl Extractor for ImageOcrExtractor {
    fn clone_box(&self) -> Box<dyn Extractor> {
        Box::new(self.clone())
    }

    fn extract(&self, path: &Path) -> Result<String, ExtractError> {
        if self.ocr.mode == OcrMode::Disabled {
            return Err(ExtractError::OcrUnavailable(
                "OCR is disabled for image extraction".to_string(),
            ));
        }
        let path = validate_path(path)?;
        run_image_ocr(&path, &self.ocr)
    }

    fn can_handle(&self, path: &Path) -> bool {
        is_image_extension(path)
    }
}

#[derive(Debug, Clone)]
pub struct OcrConfig {
    pub mode: OcrMode,
    pub pdf_text_quality_threshold: f64,
    pub ocr_binary_path: Option<PathBuf>,
    pub pdf_renderer_path: Option<PathBuf>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            mode: OcrMode::Auto,
            pdf_text_quality_threshold: 0.35,
            ocr_binary_path: None,
            pdf_renderer_path: None,
        }
    }
}

impl From<&Config> for OcrConfig {
    fn from(config: &Config) -> Self {
        Self {
            mode: config.ocr_mode,
            pdf_text_quality_threshold: config.pdf_text_quality_threshold,
            ocr_binary_path: config.ocr_binary_path.as_ref().map(PathBuf::from),
            pdf_renderer_path: config.pdf_renderer_path.as_ref().map(PathBuf::from),
        }
    }
}

impl OcrConfig {
    fn tesseract_command(&self) -> &Path {
        self.ocr_binary_path
            .as_deref()
            .unwrap_or_else(|| Path::new("tesseract"))
    }

    fn renderer_command(&self) -> &Path {
        self.pdf_renderer_path
            .as_deref()
            .unwrap_or_else(|| Path::new("pdftoppm"))
    }
}

fn command_available(command: &Path) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn ocr_available(config: &OcrConfig) -> bool {
    command_available(config.tesseract_command())
}

pub fn pdf_renderer_available(config: &OcrConfig) -> bool {
    command_available(config.renderer_command())
}

fn text_quality(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    let useful = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .count();
    useful as f64 / trimmed.chars().count().max(1) as f64
}

fn run_pdf_ocr(path: &Path, config: &OcrConfig) -> Result<String, ExtractError> {
    if !ocr_available(config) {
        return Err(ExtractError::OcrUnavailable(format!(
            "OCR binary not available: {}",
            config.tesseract_command().display()
        )));
    }
    if !pdf_renderer_available(config) {
        return Err(ExtractError::OcrUnavailable(format!(
            "PDF renderer not available: {}",
            config.renderer_command().display()
        )));
    }

    let temp = tempfile::tempdir().map_err(ExtractError::Io)?;
    let prefix = temp.path().join("page");
    let render = Command::new(config.renderer_command())
        .arg("-png")
        .arg(path)
        .arg(&prefix)
        .output()
        .map_err(ExtractError::Io)?;
    if !render.status.success() {
        return Err(ExtractError::OcrUnavailable(format!(
            "PDF renderer failed: {}",
            String::from_utf8_lossy(&render.stderr)
        )));
    }

    let mut images: Vec<PathBuf> = fs::read_dir(temp.path())
        .map_err(ExtractError::Io)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| is_image_extension(p))
        .collect();
    images.sort();

    let mut text = String::new();
    for image in images {
        let page_text = run_image_ocr(&image, config)?;
        if !text.is_empty() && !page_text.is_empty() {
            text.push('\n');
        }
        text.push_str(&page_text);
    }

    if text.trim().is_empty() {
        return Err(ExtractError::OcrUnavailable(
            "OCR returned no text for rendered PDF".to_string(),
        ));
    }
    Ok(text)
}

fn run_image_ocr(path: &Path, config: &OcrConfig) -> Result<String, ExtractError> {
    if !ocr_available(config) {
        return Err(ExtractError::OcrUnavailable(format!(
            "OCR binary not available: {}",
            config.tesseract_command().display()
        )));
    }
    let output = Command::new(config.tesseract_command())
        .arg(path)
        .arg("stdout")
        .output()
        .map_err(ExtractError::Io)?;
    if !output.status.success() {
        return Err(ExtractError::OcrUnavailable(format!(
            "OCR command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "webp"))
        .unwrap_or(false)
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
        Self::with_ocr_config(OcrConfig::default())
    }

    pub fn from_config(config: &Config) -> Self {
        Self::with_ocr_config(OcrConfig::from(config))
    }

    pub fn with_ocr_config(ocr: OcrConfig) -> Self {
        Self {
            extractors: vec![
                Box::new(MarkdownExtractor),
                Box::new(PdfExtractor::new(ocr.clone())),
                Box::new(ImageOcrExtractor::new(ocr)),
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
    use std::path::Path;

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}

    fn minimal_text_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n".to_string(),
            "2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n".to_string(),
            "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >> endobj\n".to_string(),
            "4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n".to_string(),
            format!(
                "5 0 obj << /Length {} >> stream\n{}\nendstream endobj\n",
                stream.len(),
                stream
            ),
        ];

        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for object in &objects {
            offsets.push(pdf.len());
            pdf.push_str(object);
        }
        let xref_offset = pdf.len();
        pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
        ));
        pdf.into_bytes()
    }

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
        let extractor = PdfExtractor::default();
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

        let extractor = PdfExtractor::default();
        let result = extractor.extract(&path);
        assert!(
            matches!(result, Err(ExtractError::Pdf(_))),
            "expected Pdf error for garbage bytes, got {:?}",
            result
        );
    }

    #[test]
    fn test_clean_pdf_uses_embedded_text_without_ocr_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clean.pdf");
        fs::write(&path, minimal_text_pdf("Clean embedded text")).unwrap();

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Auto,
            ocr_binary_path: Some(PathBuf::from("/definitely/missing/tesseract")),
            pdf_renderer_path: Some(PathBuf::from("/definitely/missing/pdftoppm")),
            ..OcrConfig::default()
        });
        let text = extractor.extract(&path).unwrap();

        assert!(text.contains("Clean embedded text"));
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

    #[test]
    fn test_image_ocr_disabled_returns_recoverable_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.png");
        fs::write(&path, b"fake image bytes").unwrap();

        let extractor = ImageOcrExtractor::new(OcrConfig {
            mode: OcrMode::Disabled,
            ..OcrConfig::default()
        });
        let result = extractor.extract(&path);

        assert!(matches!(result, Err(ExtractError::OcrUnavailable(_))));
    }

    #[test]
    fn test_image_ocr_missing_dependency_is_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.png");
        fs::write(&path, b"fake image bytes").unwrap();

        let extractor = ImageOcrExtractor::new(OcrConfig {
            mode: OcrMode::Auto,
            ocr_binary_path: Some(PathBuf::from("/definitely/missing/tesseract")),
            ..OcrConfig::default()
        });
        let result = extractor.extract(&path);

        assert!(matches!(result, Err(ExtractError::OcrUnavailable(_))));
    }

    #[test]
    fn test_image_ocr_uses_local_binary_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let image_path = dir.path().join("scan.png");
        fs::write(&image_path, b"fake image bytes").unwrap();

        let tesseract_path = dir.path().join("fake-tesseract");
        fs::write(
            &tesseract_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo fake-tesseract; exit 0; fi\necho 'ocr text from image'\n",
        )
        .unwrap();
        make_executable(&tesseract_path);

        let extractor = ImageOcrExtractor::new(OcrConfig {
            mode: OcrMode::Auto,
            ocr_binary_path: Some(tesseract_path),
            ..OcrConfig::default()
        });
        let text = extractor.extract(&image_path).unwrap();

        assert!(text.contains("ocr text from image"));
    }

    #[test]
    fn test_forced_pdf_ocr_requires_local_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.pdf");
        fs::write(&path, b"%PDF-1.4\n% fake pdf\n").unwrap();

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Force,
            ocr_binary_path: Some(PathBuf::from("/definitely/missing/tesseract")),
            pdf_renderer_path: Some(PathBuf::from("/definitely/missing/pdftoppm")),
            ..OcrConfig::default()
        });
        let result = extractor.extract(&path);

        assert!(matches!(result, Err(ExtractError::OcrUnavailable(_))));
    }

    #[test]
    fn test_pdf_ocr_uses_local_renderer_and_ocr_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("scan.pdf");
        fs::write(&pdf_path, b"%PDF-1.4\n% fake scanned pdf\n").unwrap();

        let renderer_path = dir.path().join("fake-pdftoppm");
        fs::write(
            &renderer_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo fake-pdftoppm; exit 0; fi\nprefix=\"$3\"\nprintf 'fake image' > \"${prefix}-1.png\"\n",
        )
        .unwrap();
        make_executable(&renderer_path);

        let tesseract_path = dir.path().join("fake-tesseract");
        fs::write(
            &tesseract_path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo fake-tesseract; exit 0; fi\necho 'ocr text from rendered pdf'\n",
        )
        .unwrap();
        make_executable(&tesseract_path);

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Force,
            ocr_binary_path: Some(tesseract_path),
            pdf_renderer_path: Some(renderer_path),
            ..OcrConfig::default()
        });
        let text = extractor.extract(&pdf_path).unwrap();

        assert!(text.contains("ocr text from rendered pdf"));
    }

    #[test]
    fn test_text_quality_scores_empty_text_as_zero() {
        assert_eq!(text_quality(""), 0.0);
    }
}
