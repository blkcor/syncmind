use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}
