use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExtractError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF extraction failed: {0}")]
    Pdf(String),
    #[error("Unsupported file type: {0}")]
    Unsupported(String),
}

#[derive(Error, Debug)]
pub enum ChunkError {
    #[error("Tree-sitter parsing failed: {0}")]
    Parse(String),
}

#[derive(Error, Debug)]
pub enum EmbedError {
    #[error("HTTP request failed: {0}")]
    // reqwest::Error does not implement std::error::Error + Sync + Send in all
    // feature combinations we target, so we wrap its display string.
    Http(String),
    #[error("ONNX inference failed: {0}")]
    Onnx(String),
    #[error("Ollama unreachable or model missing: {0}")]
    OllamaUnavailable(String),
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
