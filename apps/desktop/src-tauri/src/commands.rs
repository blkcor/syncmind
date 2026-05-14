use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
pub async fn search_knowledge(
    query: String,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResultDto>, String> {
    let embedder = Arc::clone(&state.embedder);
    let store = Arc::clone(&state.store);

    let embeddings = embedder
        .embed(&[&query])
        .await
        .map_err(|e| format!("Embedding failed: {}", e))?;

    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let results = store
        .search(&embeddings[0], top_k.unwrap_or(5))
        .map_err(|e| format!("Search failed: {}", e))?;

    Ok(results
        .into_iter()
        .map(|r| SearchResultDto {
            chunk_id: r.chunk_id,
            file_path: r.file_path.to_string_lossy().into_owned(),
            start_line: r.start_line,
            end_line: r.end_line,
            content: r.content,
            score: r.score,
        })
        .collect())
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
pub fn get_indexing_status(state: State<AppState>) -> Result<IndexingStatusDto, String> {
    let (file_count, chunk_count) = state
        .store
        .get_stats()
        .map_err(|e| format!("Failed to get stats: {}", e))?;

    Ok(IndexingStatusDto {
        file_count,
        chunk_count,
        last_updated: None,
        recent_errors: Vec::new(),
    })
}

#[tauri::command]
pub async fn trigger_reindex(
    file_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config = state.config.lock().unwrap().clone();
    let store = Arc::clone(&state.store);
    let embedder = Arc::clone(&state.embedder);

    let extractor = syncmind_rag_engine::extractor::CompositeExtractor::new();

    if let Some(path_str) = file_path {
        let path = std::path::PathBuf::from(path_str);
        let chunker = syncmind_indexing::chunker_for_path(&path, config.chunk_size, config.chunk_overlap);
        syncmind_indexing::index_file(&path, &extractor, chunker.as_ref(), embedder.as_ref(), &store)
            .await
            .map_err(|e| format!("Re-index failed: {}", e))?;
    } else {
        for path in &config.registered_files {
            let chunker = syncmind_indexing::chunker_for_path(path, config.chunk_size, config.chunk_overlap);
            if let Err(e) = syncmind_indexing::index_file(path, &extractor, chunker.as_ref(), embedder.as_ref(), &store).await {
                tracing::warn!(path = %path.display(), error = %e, "full re-index failed for file");
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path])
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Fallback to opening the parent directory.
        let parent = std::path::PathBuf::from(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(&parent)
                .spawn()
                .map_err(|e| format!("Failed to reveal in explorer: {}", e))?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&parent)
                .spawn()
                .map_err(|e| format!("Failed to reveal in file manager: {}", e))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn is_auto_launch_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    manager.is_enabled().map_err(|e| format!("Failed to query auto-launch: {}", e))
}

#[tauri::command]
pub fn set_auto_launch(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| format!("Failed to enable auto-launch: {}", e))?;
    } else {
        manager.disable().map_err(|e| format!("Failed to disable auto-launch: {}", e))?;
    }
    Ok(())
}
