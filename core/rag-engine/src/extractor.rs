use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use tracing::warn;

use crate::error::ExtractError;
use crate::ocr::{self, OcrError};
use syncmind_core::{Config, OcrMode};

static PDF_EXTRACT_PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

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
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "c", "h", "cpp", "cc", "cxx", "hpp", "hh",
    "hxx", "cs", "rb", "php", "swift", "kt", "kts", "css", "scss", "less", "toml", "json",
    "yaml", "yml", "sh", "fish", "zsh",
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
            match extract_pdf_text_from_mem(&bytes) {
                Ok(text) if self.ocr.mode == OcrMode::Auto => {
                    match run_pdf_text_fallback(&path, &self.ocr) {
                        Ok(poppler_text)
                            if should_prefer_pdf_text_fallback(&text, &poppler_text) =>
                        {
                            return Ok(poppler_text);
                        }
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "PDF text fallback unavailable");
                        }
                        _ => {}
                    }

                    if text_quality(&text) >= self.ocr.pdf_text_quality_threshold
                        && !has_excessive_nonprintable(&text)
                    {
                        return Ok(text);
                    }

                    match run_pdf_ocr(&path, &self.ocr) {
                        Ok(ocr_text) => return Ok(ocr_text),
                        Err(e) if !text.trim().is_empty() => {
                            warn!(path = %path.display(), error = %e, "PDF OCR fallback unavailable; preserving embedded text");
                            return Ok(text);
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(text) if text_quality(&text) >= self.ocr.pdf_text_quality_threshold => {
                    return Ok(text);
                }
                Ok(text) => return Ok(text),
                Err(e) if self.ocr.mode == OcrMode::Auto => {
                    match run_pdf_text_fallback(&path, &self.ocr) {
                        Ok(poppler_text) if !poppler_text.trim().is_empty() => {
                            return Ok(poppler_text);
                        }
                        Err(text_err) => {
                            warn!(path = %path.display(), error = %text_err, "PDF text fallback unavailable");
                        }
                        _ => {}
                    }

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

fn extract_pdf_text_from_mem(bytes: &[u8]) -> Result<String, ExtractError> {
    let _guard = PDF_EXTRACT_PANIC_HOOK_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }));
    panic::set_hook(previous_hook);

    match result {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(ExtractError::Pdf(format!("{e:?}"))),
        Err(_) => Err(ExtractError::Pdf(
            "pdf-extract panicked while parsing this PDF".to_string(),
        )),
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
    pub pdf_renderer_path: Option<PathBuf>,
    /// Tesseract language specification (e.g. "chi_sim+eng" for Chinese Simplified + English).
    /// Passed as `-l` flag to tesseract.
    pub ocr_language: String,
    /// Tesseract page segmentation mode (PSM).
    /// Default 6 = "Assume a single uniform block of text".
    pub ocr_psm_mode: u8,
    /// DPI for pdftoppm rendering. Higher DPI improves OCR accuracy for small text.
    pub ocr_render_dpi: u32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            mode: OcrMode::Auto,
            pdf_text_quality_threshold: 0.35,
            pdf_renderer_path: None,
            ocr_language: "chi_sim+eng".to_string(),
            ocr_psm_mode: 6,
            ocr_render_dpi: 300,
        }
    }
}

impl From<&Config> for OcrConfig {
    fn from(config: &Config) -> Self {
        Self {
            mode: config.ocr_mode,
            pdf_text_quality_threshold: config.pdf_text_quality_threshold,
            pdf_renderer_path: config.pdf_renderer_path.as_ref().map(PathBuf::from),
            ocr_language: config.ocr_language.clone(),
            ocr_psm_mode: config.ocr_psm_mode,
            ocr_render_dpi: config.ocr_render_dpi,
        }
    }
}

impl OcrConfig {
    fn renderer_command(&self) -> PathBuf {
        self.pdf_renderer_path
            .clone()
            .unwrap_or_else(|| resolve_command("pdftoppm"))
    }

    fn text_extractor_command(&self) -> PathBuf {
        if let Some(renderer) = self.pdf_renderer_path.as_ref() {
            if let Some(candidate) = sibling_poppler_text_command(renderer) {
                if command_exists("pdftotext", &candidate) {
                    return candidate;
                }
            }
        }
        resolve_command("pdftotext")
    }
}

fn pdf_renderer_command_available(command: &Path) -> bool {
    Command::new(command)
        .arg("-h")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn pdf_text_command_available(command: &Path) -> bool {
    Command::new(command)
        .arg("-v")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn sibling_poppler_text_command(renderer: &Path) -> Option<PathBuf> {
    let file_name = renderer.file_name()?.to_string_lossy();
    let text_name = file_name.strip_suffix("pdftoppm")?.to_owned() + "pdftotext";
    Some(renderer.with_file_name(text_name))
}

fn resolve_command(name: &str) -> PathBuf {
    if command_exists(name, Path::new(name)) {
        return PathBuf::from(name);
    }

    for prefix in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = Path::new(prefix).join(name);
        if command_exists(name, &candidate) {
            return candidate;
        }
    }

    PathBuf::from(name)
}

fn command_exists(name: &str, command: &Path) -> bool {
    if command.is_absolute() && !command.exists() {
        return false;
    }
    match name {
        "pdftoppm" => pdf_renderer_command_available(command),
        "pdftotext" => pdf_text_command_available(command),
        _ => false,
    }
}

pub fn pdf_renderer_available(config: &OcrConfig) -> bool {
    command_exists("pdftoppm", &config.renderer_command())
}

fn pdf_text_extractor_available(config: &OcrConfig) -> bool {
    command_exists("pdftotext", &config.text_extractor_command())
}

#[allow(dead_code)]
fn extract_pdf_text_safely(bytes: &[u8]) -> Result<String, ExtractError> {
    extract_pdf_text_with(bytes, pdf_extract::extract_text_from_mem)
}

#[allow(dead_code)]
fn extract_pdf_text_with_fallback<F>(
    path: &Path,
    bytes: &[u8],
    ocr: &OcrConfig,
    extract_fn: F,
) -> Result<String, ExtractError>
where
    F: Fn(&[u8]) -> Result<String, ExtractError>,
{
    if ocr.mode == OcrMode::Force {
        return run_pdf_ocr(path, ocr);
    }

    match extract_fn(bytes) {
        Ok(text) if text_quality(&text) >= ocr.pdf_text_quality_threshold => Ok(text),
        Ok(text) if ocr.mode == OcrMode::Auto => match run_pdf_ocr(path, ocr) {
            Ok(ocr_text) => Ok(ocr_text),
            Err(e) if !text.trim().is_empty() => {
                warn!(path = %path.display(), error = %e, "PDF OCR fallback unavailable; preserving embedded text");
                Ok(text)
            }
            Err(e) => Err(e),
        },
        Ok(text) => Ok(text),
        Err(e) if ocr.mode == OcrMode::Auto => match run_pdf_ocr(path, ocr) {
            Ok(ocr_text) => Ok(ocr_text),
            Err(ocr_err) => Err(ExtractError::Pdf(format!(
                "embedded extraction failed: {e}; OCR fallback failed: {ocr_err}"
            ))),
        },
        Err(e) => Err(e),
    }
}

#[allow(dead_code)]
fn extract_pdf_text_with<F>(bytes: &[u8], extract_fn: F) -> Result<String, ExtractError>
where
    F: FnOnce(&[u8]) -> Result<String, pdf_extract::OutputError>,
{
    match panic::catch_unwind(AssertUnwindSafe(|| extract_fn(bytes))) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(err)) => Err(ExtractError::Pdf(format!("{err:?}"))),
        Err(payload) => Err(ExtractError::Pdf(format!(
            "third-party extractor panicked: {}",
            panic_payload_message(payload)
        ))),
    }
}

#[allow(dead_code)]
fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
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

fn cjk_ratio(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    let cjk = trimmed.chars().filter(|c| is_cjk(*c)).count();
    cjk as f64 / trimmed.chars().count().max(1) as f64
}

/// Check if text has a high ratio of non-printable control characters,
/// indicating encoding corruption or garbled extraction that would produce
/// poor quality results even if `text_quality` appears acceptable.
fn has_excessive_nonprintable(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let total = trimmed.chars().count().max(1);
    let non_printable = trimmed
        .chars()
        .filter(|c| c.is_control() && *c != '\n' && *c != '\t')
        .count();
    non_printable as f64 / total as f64 > 0.05
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

fn should_prefer_pdf_text_fallback(embedded: &str, fallback: &str) -> bool {
    let fallback = fallback.trim();
    if fallback.is_empty() {
        return false;
    }

    let fallback_quality = text_quality(fallback);
    if fallback_quality < 0.35 {
        return false;
    }

    let embedded_cjk = cjk_ratio(embedded);
    let fallback_cjk = cjk_ratio(fallback);
    if fallback_cjk >= 0.15 && embedded_cjk < 0.02 {
        return true;
    }

    fallback_quality > text_quality(embedded) + 0.25 && fallback.len() > embedded.trim().len() / 2
}

/// Clean up raw OCR output by removing excessive whitespace, control characters,
/// and normalizing spacing. This improves downstream text quality for chunking and
/// embedding.
fn clean_ocr_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;
    let mut consecutive_newlines = 0;

    for ch in text.chars() {
        match ch {
            '\r' => {
                // Skip carriage returns; \n is used as line separator
                continue;
            }
            '\n' => {
                consecutive_newlines += 1;
                if consecutive_newlines <= 2 {
                    result.push('\n');
                }
                prev_was_space = false;
            }
            '\t' => {
                // Replace tabs with a single space
                if !prev_was_space {
                    result.push(' ');
                }
                consecutive_newlines = 0;
                prev_was_space = true;
            }
            c if c.is_control() => {
                // Skip other control characters (null bytes, escapes, etc.)
                consecutive_newlines = 0;
                continue;
            }
            c if c.is_whitespace() => {
                // Collapse horizontal whitespace runs into a single space
                if !prev_was_space {
                    result.push(' ');
                }
                consecutive_newlines = 0;
                prev_was_space = true;
            }
            _ => {
                result.push(ch);
                consecutive_newlines = 0;
                prev_was_space = false;
            }
        }
    }

    result.trim().to_string()
}

fn run_pdf_text_fallback(path: &Path, config: &OcrConfig) -> Result<String, ExtractError> {
    let pdftotext = config.text_extractor_command();
    if !pdf_text_extractor_available(config) {
        return Err(ExtractError::OcrUnavailable(format!(
            "PDF text extractor not available: {}",
            pdftotext.display()
        )));
    }

    let output = Command::new(&pdftotext)
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        .map_err(ExtractError::Io)?;
    if !output.status.success() {
        return Err(ExtractError::OcrUnavailable(format!(
            "PDF text fallback failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_pdf_ocr(path: &Path, config: &OcrConfig) -> Result<String, ExtractError> {
    let renderer = config.renderer_command();
    if !pdf_renderer_available(config) {
        return Err(ExtractError::OcrUnavailable(format!(
            "PDF renderer not available: {}",
            renderer.display()
        )));
    }

    let temp = tempfile::tempdir().map_err(ExtractError::Io)?;
    let prefix = temp.path().join("page");
    let render = Command::new(&renderer)
        .arg("-png")
        .arg("-r")
        .arg(config.ocr_render_dpi.to_string())
        .arg("-grayscale")
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
        let bytes = fs::read(&image).map_err(ExtractError::Io)?;
        let page_text = ocr::ocr_image_from_bytes(&bytes, image::ImageFormat::Png)
            .map_err(ocr_error_to_extract_error)?;
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
    Ok(clean_ocr_text(&text))
}

fn run_image_ocr(path: &Path, _config: &OcrConfig) -> Result<String, ExtractError> {
    ocr::ocr_image(path).map_err(ocr_error_to_extract_error)
}

fn ocr_error_to_extract_error(error: OcrError) -> ExtractError {
    match error {
        OcrError::Decode(message) => ExtractError::OcrUnavailable(format!("decode: {message}")),
        OcrError::Init(message) => ExtractError::OcrUnavailable(format!("init: {message}")),
        OcrError::Recognition(message) => {
            ExtractError::OcrUnavailable(format!("recognition: {message}"))
        }
    }
}

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "webp"
            )
        })
        .unwrap_or(false)
}

pub struct CompositeExtractor {
    extractors: Vec<Box<dyn Extractor>>,
}

impl Clone for CompositeExtractor {
    fn clone(&self) -> Self {
        Self::with_extractors(self.extractors.iter().map(|e| e.clone_box()).collect())
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
        let mut last_error = None;
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
                    last_error = Some(e);
                }
            }
        }
        if let Some(e) = last_error {
            return Err(e);
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
    fn test_code_extractor_handles_stylesheet_text() {
        let dir = tempfile::tempdir().unwrap();
        let extractor = CodeExtractor;

        for (name, content) in [
            ("reset.css", "body { margin: 0; }\n"),
            ("variables.scss", "$accent: #f00;\n.button { color: $accent; }\n"),
            ("theme.less", "@accent: #f00;\n.button { color: @accent; }\n"),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, content).unwrap();

            assert!(extractor.can_handle(&path));
            assert_eq!(extractor.extract(&path).unwrap(), content);
        }
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
        let result = composite.extract(Path::new("notes.unknown"));
        assert!(matches!(result, Err(ExtractError::Unsupported(_))));
    }

    #[test]
    fn test_composite_preserves_only_matching_extractor_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.pdf");
        fs::write(&path, b"not a pdf").unwrap();

        let composite = CompositeExtractor::new();
        let result = composite.extract(&path);

        assert!(
            matches!(result, Err(ExtractError::Pdf(_))),
            "expected Pdf error for matching PDF extractor, got {result:?}"
        );
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
    fn test_pdf_extractor_converts_pdf_extract_panic_to_error() {
        let result = extract_pdf_text_with(
            b"synthetic pdf bytes",
            |_| -> Result<String, pdf_extract::OutputError> {
                panic!("assertion failed: name == \"Identity-H\"");
            },
        );

        match result {
            Err(ExtractError::Pdf(message)) => assert!(
                message.contains("third-party extractor panicked")
                    && message.contains("Identity-H"),
                "expected captured panic message, got {message:?}"
            ),
            other => panic!("expected Pdf error for unsupported CID PDF, got {other:?}"),
        }
    }

    #[test]
    fn test_clean_pdf_uses_embedded_text_without_ocr_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clean.pdf");
        fs::write(&path, minimal_text_pdf("Clean embedded text")).unwrap();

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Auto,
            pdf_renderer_path: Some(PathBuf::from("/definitely/missing/pdftoppm")),
            ..OcrConfig::default()
        });
        let text = extractor.extract(&path).unwrap();

        assert!(text.contains("Clean embedded text"));
    }

    #[test]
    fn test_pdf_extractor_auto_mode_reports_decode_error_after_rendered_page_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("panic.pdf");
        fs::write(&pdf_path, b"%PDF-1.4\n% fake scanned pdf\n").unwrap();

        let renderer_path = dir.path().join("fake-pdftoppm");
        fs::write(
            &renderer_path,
            "#!/bin/sh\nif [ \"$1\" = \"-h\" ]; then echo fake-pdftoppm; exit 0; fi\nprefix=\"$3\"\nprintf 'not a png' > \"${prefix}-1.png\"\n",
        )
        .unwrap();
        make_executable(&renderer_path);

        let ocr = OcrConfig {
            mode: OcrMode::Auto,
            pdf_renderer_path: Some(renderer_path),
            ..OcrConfig::default()
        };

        let result =
            extract_pdf_text_with_fallback(&pdf_path, b"fake pdf bytes", &ocr, |bytes| {
                extract_pdf_text_with(bytes, |_| -> Result<String, pdf_extract::OutputError> {
                    panic!("assertion failed: name == \"Identity-H\"");
                })
            });

        match result {
            Err(ExtractError::Pdf(message)) => {
                assert!(message.contains("embedded extraction failed"));
                assert!(message.contains("OCR fallback failed"));
                assert!(message.contains("decode:"));
            }
            other => panic!("expected combined Pdf error, got {other:?}"),
        }
    }

    #[test]
    fn test_pdf_extractor_auto_mode_reports_combined_error_after_panic_and_ocr_failure() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("panic.pdf");
        fs::write(&pdf_path, b"%PDF-1.4\n% fake scanned pdf\n").unwrap();

        let ocr = OcrConfig {
            mode: OcrMode::Auto,
            pdf_renderer_path: Some(PathBuf::from("/definitely/missing/pdftoppm")),
            ..OcrConfig::default()
        };

        let result = extract_pdf_text_with_fallback(&pdf_path, b"fake pdf bytes", &ocr, |bytes| {
            extract_pdf_text_with(bytes, |_| -> Result<String, pdf_extract::OutputError> {
                panic!("assertion failed: name == \"Identity-H\"");
            })
        });

        match result {
            Err(ExtractError::Pdf(message)) => {
                assert!(message.contains("embedded extraction failed"));
                assert!(message.contains("third-party extractor panicked"));
                assert!(message.contains("OCR fallback failed"));
            }
            other => panic!("expected combined Pdf error, got {other:?}"),
        }
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
    fn test_image_ocr_corrupt_image_returns_recoverable_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.png");
        fs::write(&path, b"fake image bytes").unwrap();

        let extractor = ImageOcrExtractor::new(OcrConfig::default());
        let result = extractor.extract(&path);

        assert!(matches!(result, Err(ExtractError::OcrUnavailable(_))));
    }

    #[test]
    fn test_image_ocr_missing_models_is_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.png");
        image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]))
            .save(&path)
            .unwrap();

        let extractor = ImageOcrExtractor::new(OcrConfig::default());
        let result = extractor.extract(&path);

        match result {
            Err(ExtractError::OcrUnavailable(message)) => assert!(message.contains("init:")),
            other => panic!("expected OCR init error, got {other:?}"),
        }
    }

    #[test]
    fn test_forced_pdf_ocr_requires_local_renderer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.pdf");
        fs::write(&path, b"%PDF-1.4\n% fake pdf\n").unwrap();

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Force,
            pdf_renderer_path: Some(PathBuf::from("/definitely/missing/pdftoppm")),
            ..OcrConfig::default()
        });
        let result = extractor.extract(&path);

        assert!(matches!(result, Err(ExtractError::OcrUnavailable(_))));
    }

    #[test]
    fn test_pdf_renderer_available_uses_renderer_help_probe() {
        let dir = tempfile::tempdir().unwrap();
        let renderer_path = dir.path().join("fake-pdftoppm");
        fs::write(
            &renderer_path,
            "#!/bin/sh\nif [ \"$1\" = \"-h\" ]; then echo fake-pdftoppm; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        make_executable(&renderer_path);

        let config = OcrConfig {
            pdf_renderer_path: Some(renderer_path),
            ..OcrConfig::default()
        };

        assert!(pdf_renderer_available(&config));
    }

    #[test]
    fn test_pdf_text_fallback_prefers_cjk_text_over_encoded_gibberish() {
        let embedded = "eta i) $230231013A1 FRAME hs 2a AT BEE ( SUM) BIR ERE";
        let fallback = "重庆邮电大学\n普通高校毕业生就业协议书\n用人单位 棋行科技";

        assert!(should_prefer_pdf_text_fallback(embedded, fallback));
    }

    #[test]
    fn test_pdf_text_fallback_uses_sibling_poppler_text_extractor() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("doc.pdf");
        fs::write(&pdf_path, b"%PDF-1.4\n% fake pdf\n").unwrap();

        let renderer_path = dir.path().join("fake-pdftoppm");
        fs::write(
            &renderer_path,
            "#!/bin/sh\nif [ \"$1\" = \"-h\" ]; then echo fake-pdftoppm; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        make_executable(&renderer_path);

        let text_path = dir.path().join("fake-pdftotext");
        fs::write(
            &text_path,
            "#!/bin/sh\nif [ \"$1\" = \"-v\" ]; then echo fake-pdftotext >&2; exit 0; fi\nif [ \"$1\" != \"-layout\" ]; then echo 'missing layout flag' >&2; exit 2; fi\nif ! grep -q 'fake pdf' \"$2\"; then echo 'unexpected PDF input' >&2; exit 3; fi\necho '重庆邮电大学 普通高校毕业生就业协议书'\n",
        )
        .unwrap();
        make_executable(&text_path);

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Auto,
            pdf_renderer_path: Some(renderer_path),
            ..OcrConfig::default()
        });
        let text = extractor.extract(&pdf_path).unwrap();

        assert!(text.contains("重庆邮电大学"));
    }

    #[test]
    fn test_pdf_ocr_uses_local_renderer_and_reports_invalid_rendered_image() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("scan.pdf");
        fs::write(&pdf_path, b"%PDF-1.4\n% fake scanned pdf\n").unwrap();

        let renderer_path = dir.path().join("fake-pdftoppm");
        fs::write(
            &renderer_path,
            "#!/bin/sh\nif [ \"$1\" = \"-h\" ]; then echo fake-pdftoppm; exit 0; fi\nif [ \"$1\" != \"-png\" ]; then echo 'unexpected renderer probe' >&2; exit 2; fi\nif ! grep -q 'fake scanned pdf' \"$2\"; then echo 'unexpected PDF input' >&2; exit 3; fi\nprefix=\"$3\"\nprintf 'not a png' > \"${prefix}-1.png\"\n",
        )
        .unwrap();
        make_executable(&renderer_path);

        let extractor = PdfExtractor::new(OcrConfig {
            mode: OcrMode::Force,
            pdf_renderer_path: Some(renderer_path),
            ..OcrConfig::default()
        });
        let result = extractor.extract(&pdf_path);

        match result {
            Err(ExtractError::OcrUnavailable(message)) => assert!(message.contains("decode:")),
            other => panic!("expected OCR decode error, got {other:?}"),
        }
    }

    #[test]
    fn test_text_quality_scores_empty_text_as_zero() {
        assert_eq!(text_quality(""), 0.0);
    }
}
