use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub chunk_id: i64,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDto {
    pub ollama_url: String,
    pub ollama_model: String,
    pub mcp_transport: String,
    pub bind_addr: String,
    pub registered_files: Vec<String>,
    pub embedding_dim: usize,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingStatusDto {
    pub file_count: usize,
    pub chunk_count: usize,
    pub last_updated: Option<String>,
    pub recent_errors: Vec<IndexingErrorDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingErrorDto {
    pub file_path: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigPatchDto {
    pub ollama_url: Option<String>,
    pub ollama_model: Option<String>,
    pub registered_files: Option<Vec<String>>,
}

#[tauri::command]
pub fn search_knowledge(query: String, _top_k: Option<usize>) -> Vec<SearchResultDto> {
    tracing::info!(query = %query, "search_knowledge stub");
    vec![
        SearchResultDto {
            chunk_id: 1,
            file_path: "/demo/example.rs".into(),
            start_line: 10,
            end_line: 20,
            content: format!("Demo result for query: {}", query),
            score: 0.95,
        },
    ]
}

#[tauri::command]
pub fn get_config(state: State<AppState>) -> ConfigDto {
    let config = state.config.lock().unwrap();
    ConfigDto {
        ollama_url: config.ollama_url.clone(),
        ollama_model: config.ollama_model.clone(),
        mcp_transport: match config.mcp_transport {
            syncmind_core::McpTransport::Stdio => "stdio".into(),
            syncmind_core::McpTransport::Sse => "sse".into(),
        },
        bind_addr: config.bind_addr.clone(),
        registered_files: config.registered_files.iter().map(|p| p.to_string_lossy().into()).collect(),
        embedding_dim: config.embedding_dim,
        chunk_size: config.chunk_size,
        chunk_overlap: config.chunk_overlap,
    }
}

#[tauri::command]
pub fn update_config(patch: ConfigPatchDto, state: State<AppState>) -> ConfigDto {
    let mut config = state.config.lock().unwrap();
    if let Some(url) = patch.ollama_url {
        config.ollama_url = url;
    }
    if let Some(model) = patch.ollama_model {
        config.ollama_model = model;
    }
    if let Some(files) = patch.registered_files {
        config.registered_files = files.into_iter().map(std::path::PathBuf::from).collect();
    }
    ConfigDto {
        ollama_url: config.ollama_url.clone(),
        ollama_model: config.ollama_model.clone(),
        mcp_transport: match config.mcp_transport {
            syncmind_core::McpTransport::Stdio => "stdio".into(),
            syncmind_core::McpTransport::Sse => "sse".into(),
        },
        bind_addr: config.bind_addr.clone(),
        registered_files: config.registered_files.iter().map(|p| p.to_string_lossy().into()).collect(),
        embedding_dim: config.embedding_dim,
        chunk_size: config.chunk_size,
        chunk_overlap: config.chunk_overlap,
    }
}

#[tauri::command]
pub fn get_indexing_status() -> IndexingStatusDto {
    IndexingStatusDto {
        file_count: 42,
        chunk_count: 1337,
        last_updated: Some("2026-05-14T10:00:00Z".into()),
        recent_errors: vec![],
    }
}

#[tauri::command]
pub fn trigger_reindex() -> Result<(), String> {
    tracing::info!("trigger_reindex stub");
    Ok(())
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    tracing::info!(path = %path, "open_file stub");
    Ok(())
}

#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    tracing::info!(path = %path, "reveal_in_finder stub");
    Ok(())
}
