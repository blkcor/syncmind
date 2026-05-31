use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tauri::Emitter;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub chunk_id: i64,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<i64>,
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
    /// Actual embedder backend in use (ollama / onnx / unavailable).
    pub active_embedder: String,
    /// Actual model name the active embedder is using.
    pub active_model: String,
    pub hybrid_search_enabled: bool,
    pub reranker_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingStatusDto {
    pub file_count: usize,
    pub chunk_count: usize,
    pub last_updated: Option<String>,
    pub recent_errors: Vec<IndexingErrorDto>,
    /// Actual embedder backend in use (ollama / onnx / unavailable).
    pub active_embedder: String,
    /// Actual model name the active embedder is using.
    pub active_model: String,
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
    pub embedding_dim: Option<usize>,
    pub registered_files: Option<Vec<String>>,
    pub hybrid_search_enabled: Option<bool>,
    pub reranker_enabled: Option<bool>,
}

#[tauri::command]
pub async fn search_knowledge(
    query: String,
    top_k: Option<usize>,
    filter_file_type: Option<Vec<String>>,
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

    let top_k = top_k.unwrap_or(5);
    let patterns = filter_file_type.unwrap_or_default();
    let filter = syncmind_rag_engine::file_filter::parse_file_filter(&patterns)
        .map_err(|e| format!("Invalid file filter: {}", e))?;

    let use_hybrid = {
        let config = state.config.lock().unwrap();
        config.hybrid_search_enabled
    };

    // When hybrid search is active we always use FTS5 + vector fusion and
    // apply any file-type filter as a post-processing pass.  Pure vector mode
    // still uses the dedicated path-filter query for efficiency.
    let mut results = if use_hybrid {
        store
            .search_hybrid(&embeddings[0], &query, top_k, None)
            .map_err(|e| format!("Search failed: {}", e))?
    } else if let Some(ref f) = filter {
        store
            .search_with_path_filter(&embeddings[0], top_k, 5, |path| f.evaluate(path))
            .map_err(|e| format!("Search failed: {}", e))?
    } else {
        store
            .search(&embeddings[0], top_k)
            .map_err(|e| format!("Search failed: {}", e))?
    };

    // Post-filter hybrid results when a file-type glob is also active.
    if use_hybrid {
        if let Some(ref f) = filter {
            results.retain(|r| f.evaluate(&r.file_path));
        }
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results
        .into_iter()
        .map(search_result_to_dto)
        .collect())
}

#[tauri::command]
pub fn get_config(state: State<AppState>) -> ConfigDto {
    let config = state.config.lock().unwrap();
    let info = state
        .embedder_info
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    ConfigDto {
        ollama_url: config.ollama_url.clone(),
        ollama_model: config.ollama_model.clone(),
        mcp_transport: match config.mcp_transport {
            syncmind_core::McpTransport::Stdio => "stdio".into(),
            syncmind_core::McpTransport::Sse => "sse".into(),
        },
        bind_addr: config.bind_addr.clone(),
        registered_files: config
            .registered_files
            .iter()
            .map(|p| p.to_string_lossy().into())
            .collect(),
        embedding_dim: config.embedding_dim,
        chunk_size: config.chunk_size,
        chunk_overlap: config.chunk_overlap,
        active_embedder: info.active_embedder.clone(),
        active_model: info.active_model.clone(),
        hybrid_search_enabled: config.hybrid_search_enabled,
        reranker_enabled: config.reranker_enabled,
    }
}

fn config_to_dto(
    config: &syncmind_core::Config,
    active_embedder: &str,
    active_model: &str,
) -> ConfigDto {
    ConfigDto {
        ollama_url: config.ollama_url.clone(),
        ollama_model: config.ollama_model.clone(),
        mcp_transport: match config.mcp_transport {
            syncmind_core::McpTransport::Stdio => "stdio".into(),
            syncmind_core::McpTransport::Sse => "sse".into(),
        },
        bind_addr: config.bind_addr.clone(),
        registered_files: config
            .registered_files
            .iter()
            .map(|p| p.to_string_lossy().into())
            .collect(),
        embedding_dim: config.embedding_dim,
        chunk_size: config.chunk_size,
        chunk_overlap: config.chunk_overlap,
        active_embedder: active_embedder.to_string(),
        active_model: active_model.to_string(),
        hybrid_search_enabled: config.hybrid_search_enabled,
        reranker_enabled: config.reranker_enabled,
    }
}

pub fn apply_config_patch(config: &mut syncmind_core::Config, patch: ConfigPatchDto) {
    if let Some(url) = patch.ollama_url {
        config.ollama_url = url;
    }
    if let Some(model) = patch.ollama_model {
        config.ollama_model = model;
    }
    if let Some(embedding_dim) = patch.embedding_dim {
        config.embedding_dim = embedding_dim;
    }
    config.normalize_embedding_dim();
    if let Some(files) = patch.registered_files {
        config.registered_files = files.into_iter().map(std::path::PathBuf::from).collect();
    }
    if let Some(v) = patch.hybrid_search_enabled {
        config.hybrid_search_enabled = v;
    }
    if let Some(v) = patch.reranker_enabled {
        config.reranker_enabled = v;
    }
}

#[tauri::command]
pub async fn update_config(
    patch: ConfigPatchDto,
    state: State<'_, AppState>,
) -> Result<ConfigDto, String> {
    let updated = {
        let mut config = state.config.lock().unwrap();
        apply_config_patch(&mut config, patch);
        config
            .save()
            .map_err(|e| format!("Failed to save config: {}", e))?;
        config.clone()
    };

    if updated.embedding_dim != state.embedder.embedding_dim() {
        return Err(format!(
            "Config saved with {}-dim embeddings. Restart SyncMind before rebuilding the index so the vector store opens with the new dimension.",
            updated.embedding_dim
        ));
    }

    state
        .refresh_embedder(&updated)
        .await
        .map_err(|e| format!("Embedder refresh failed: {}", e))?;

    let info = state
        .embedder_info
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    Ok(config_to_dto(&updated, &info.active_embedder, &info.active_model))
}

#[tauri::command]
pub fn get_indexing_status(state: State<AppState>) -> Result<IndexingStatusDto, String> {
    let (file_count, chunk_count) = state
        .store
        .get_stats()
        .map_err(|e| format!("Failed to get stats: {}", e))?;

    let indexing = state
        .indexing
        .lock()
        .map_err(|e| format!("indexing state lock poisoned: {}", e))?;

    let info = state
        .embedder_info
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    Ok(IndexingStatusDto {
        file_count,
        chunk_count,
        last_updated: indexing.last_updated.map(iso8601_utc),
        recent_errors: indexing
            .recent_errors
            .iter()
            .map(|e| IndexingErrorDto {
                file_path: e.file_path.to_string_lossy().into_owned(),
                message: e.message.clone(),
                timestamp: iso8601_utc(e.timestamp),
            })
            .collect(),
        active_embedder: info.active_embedder.clone(),
        active_model: info.active_model.clone(),
    })
}

/// Format a unix-seconds timestamp as ISO-8601 UTC (e.g. `2026-05-20T14:32:00Z`).
/// Uses Howard Hinnant's civil_from_days algorithm to avoid a chrono dependency.
fn iso8601_utc(ts: i64) -> String {
    let secs = ts.max(0) as u64;
    let days = secs / 86_400;
    let hh = (secs % 86_400) / 3_600;
    let mm = (secs % 3_600) / 60;
    let ss = secs % 60;

    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era * 400 + yoe as i64 + if m <= 2 { 1 } else { 0 };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss)
}

#[tauri::command]
pub async fn trigger_reindex(
    file_path: Option<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config = state.config.lock().unwrap().clone();
    let store = Arc::clone(&state.store);
    let embedder = Arc::clone(&state.embedder);
    let on_result = Arc::clone(&state.on_index_result);

    let extractor = syncmind_rag_engine::extractor::CompositeExtractor::from_config(&config);

    if let Some(path_str) = file_path {
        let path = std::path::PathBuf::from(path_str);
        let chunker =
            syncmind_indexing::chunker_for_path(&path, config.chunk_size, config.chunk_overlap);
        let result = syncmind_indexing::index_file(
            &path,
            &extractor,
            chunker.as_ref(),
            embedder.as_ref(),
            &store,
        )
        .await;
        on_result(&path, result.as_ref().map(|_| ()));
        result.map_err(|e| format!("Re-index failed: {}", e))?;
    } else {
        state
            .store
            .clear_index()
            .map_err(|e| format!("Failed to clear existing index: {}", e))?;

        let total = config.registered_files.len();
        for (i, path) in config.registered_files.iter().enumerate() {
            let current = i + 1;
            let _ = app_handle.emit(
                "reindex://progress",
                serde_json::json!({
                    "current": current,
                    "total": total,
                    "file_path": path.to_string_lossy(),
                }),
            );

            let chunker =
                syncmind_indexing::chunker_for_path(path, config.chunk_size, config.chunk_overlap);
            let result = syncmind_indexing::index_file(
                path,
                &extractor,
                chunker.as_ref(),
                embedder.as_ref(),
                &store,
            )
            .await;
            if let Err(e) = &result {
                tracing::warn!(path = %path.display(), error = %e, "full re-index failed for file");
            }
            on_result(path, result.as_ref().map(|_| ()));
        }
        let _ = app_handle.emit("reindex://complete", serde_json::json!({}));
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
    manager
        .is_enabled()
        .map_err(|e| format!("Failed to query auto-launch: {}", e))
}

#[tauri::command]
pub fn set_auto_launch(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("Failed to enable auto-launch: {}", e))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("Failed to disable auto-launch: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_dialog_open(open: bool, state: State<AppState>) {
    let mut guard = state.dialog_open.lock().unwrap();
    *guard = open;
}

fn search_result_to_dto(r: syncmind_storage::SearchResult) -> SearchResultDto {
    SearchResultDto {
        chunk_id: r.chunk_id,
        file_path: r.file_path.to_string_lossy().into_owned(),
        start_line: r.start_line,
        end_line: r.end_line,
        content: r.content,
        score: r.score,
        tags: r.tags,
        pinned_at: r.pinned_at,
    }
}

#[tauri::command]
pub fn pin_chunk(
    chunk_id: i64,
    tags: Option<Vec<String>>,
    state: State<AppState>,
) -> Result<(), String> {
    state
        .store
        .pin_chunk(chunk_id, tags.as_deref())
        .map_err(|e| format!("Pin failed: {}", e))
}

#[tauri::command]
pub fn unpin_chunk(chunk_id: i64, state: State<AppState>) -> Result<(), String> {
    state
        .store
        .unpin_chunk(chunk_id)
        .map_err(|e| format!("Unpin failed: {}", e))
}

#[tauri::command]
pub fn is_chunk_pinned(chunk_id: i64, state: State<AppState>) -> Result<bool, String> {
    state
        .store
        .is_chunk_pinned(chunk_id)
        .map_err(|e| format!("Pin lookup failed: {}", e))
}

#[tauri::command]
pub fn list_pinned_chunks(state: State<AppState>) -> Result<Vec<SearchResultDto>, String> {
    let rows = state
        .store
        .list_pinned_chunks()
        .map_err(|e| format!("List pinned chunks failed: {}", e))?;
    Ok(rows.into_iter().map(search_result_to_dto).collect())
}

#[tauri::command]
pub fn update_pin_tags(
    chunk_id: i64,
    tags: Vec<String>,
    state: State<AppState>,
) -> Result<(), String> {
    state
        .store
        .update_pin_tags(chunk_id, &tags)
        .map_err(|e| format!("Update pin tags failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn search_pinned_chunks(
    tag: Option<String>,
    state: State<AppState>,
) -> Result<Vec<SearchResultDto>, String> {
    let rows = state
        .store
        .search_pinned_chunks(tag.as_deref())
        .map_err(|e| format!("Search pinned chunks failed: {}", e))?;
    Ok(rows.into_iter().map(search_result_to_dto).collect())
}

#[tauri::command]
pub fn list_indexed_file_types(state: State<AppState>) -> Result<Vec<String>, String> {
    state
        .store
        .list_indexed_file_types()
        .map_err(|e| format!("List indexed file types failed: {}", e))
}

#[tauri::command]
pub fn validate_file_filter(patterns: Vec<String>) -> Result<(), String> {
    syncmind_rag_engine::file_filter::parse_file_filter(&patterns)
        .map(|_| ())
        .map_err(|e| format!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_config_patch_updates_embedding_dim_for_bge_small() {
        let mut config = syncmind_core::Config {
            ollama_model: "bge-m3".to_string(),
            embedding_dim: 1024,
            ..syncmind_core::Config::default()
        };

        apply_config_patch(
            &mut config,
            ConfigPatchDto {
                ollama_url: None,
                ollama_model: Some("bge-small".to_string()),
                embedding_dim: Some(384),
                registered_files: None,
            },
        );

        assert_eq!(config.ollama_model, "bge-small");
        assert_eq!(config.embedding_dim, 384);
    }

    #[test]
    fn apply_config_patch_normalizes_known_model_even_with_stale_dim() {
        let mut config = syncmind_core::Config {
            ollama_model: "bge-m3".to_string(),
            embedding_dim: 1024,
            ..syncmind_core::Config::default()
        };

        apply_config_patch(
            &mut config,
            ConfigPatchDto {
                ollama_url: None,
                ollama_model: Some("bge-small".to_string()),
                embedding_dim: Some(1024),
                registered_files: None,
            },
        );

        assert_eq!(config.embedding_dim, 384);
    }
}
