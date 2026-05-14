use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug, Serialize)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(String),
    #[error("Count mismatch: {chunks} chunks vs {embeddings} embeddings")]
    CountMismatch { chunks: usize, embeddings: usize },
    #[error("Invalid embedding dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },
    #[error("Failed to register sqlite-vec extension")]
    ExtensionRegistrationFailed,
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        StorageError::Sqlite(err.to_string())
    }
}
