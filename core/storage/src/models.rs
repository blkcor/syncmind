use std::path::PathBuf;

pub use syncmind_core::Chunk;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileMeta {
    pub absolute_path: PathBuf,
    pub file_type: String,
    pub last_modified: i64,
    pub last_indexed: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub file_path: PathBuf,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
}
