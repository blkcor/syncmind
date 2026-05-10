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
