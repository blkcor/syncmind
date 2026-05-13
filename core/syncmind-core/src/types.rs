#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}
