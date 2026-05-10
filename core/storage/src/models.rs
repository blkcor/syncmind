use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub absolute_path: PathBuf,
    pub file_type: String,
    pub last_modified: i64,
    pub last_indexed: i64,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub chunk_index: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub file_path: PathBuf,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
}
