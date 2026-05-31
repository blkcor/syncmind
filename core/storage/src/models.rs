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
    /// Expanded display text with adjacent chunks (sentence window). Empty for non-expanded results.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_content: String,
    /// Tags attached to a pinned chunk. Empty for regular search results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Unix-seconds timestamp of when the chunk was pinned. `None` for non-pinned results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<i64>,
}
