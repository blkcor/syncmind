use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Count mismatch: {chunks} chunks vs {embeddings} embeddings")]
    CountMismatch { chunks: usize, embeddings: usize },
    #[error("Invalid embedding dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },
    #[error("Failed to register sqlite-vec extension")]
    ExtensionRegistrationFailed,
}
