export interface SearchResult {
  chunk_id: number;
  file_path: string;
  start_line: number;
  end_line: number;
  content: string;
  score: number;
}

export interface Config {
  ollama_url: string;
  ollama_model: string;
  mcp_transport: 'stdio' | 'sse';
  bind_addr: string;
  registered_files: string[];
  embedding_dim: number;
  chunk_size: number;
  chunk_overlap: number;
}

export interface IndexingStatus {
  file_count: number;
  chunk_count: number;
  last_updated: string | null;
  recent_errors: IndexingError[];
}

export interface IndexingError {
  file_path: string;
  message: string;
  timestamp: string;
}

export interface ConfigPatch {
  ollama_url?: string;
  ollama_model?: string;
  registered_files?: string[];
}
