use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use syncmind_file_watcher::FileEvent;
use syncmind_rag_engine::chunker::{Chunker, CodeChunker, FallbackChunker, MarkdownChunker};
use syncmind_rag_engine::embedder::Embedder;
use syncmind_rag_engine::extractor::{CompositeExtractor, Extractor};

/// Callback invoked after each file indexing attempt. The closure receives
/// the file path and the result. Used by the desktop app to update the
/// shared `IndexingState` and emit events to the frontend / tray.
pub type IndexResultCallback = Arc<dyn Fn(&Path, Result<(), &anyhow::Error>) + Send + Sync>;

/// Select the appropriate chunker for a file based on its extension.
pub fn chunker_for_path(
    path: &std::path::Path,
    chunk_size: usize,
    chunk_overlap: usize,
) -> Box<dyn Chunker> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("md") {
            return Box::new(MarkdownChunker::new(chunk_size, chunk_overlap));
        }
        if ["rs", "py", "ts", "js", "go", "java", "c", "cpp", "h", "hpp"]
            .iter()
            .any(|&e| e.eq_ignore_ascii_case(ext))
        {
            return Box::new(CodeChunker::new(chunk_size, chunk_overlap));
        }
    }
    Box::new(FallbackChunker::new(chunk_size, chunk_overlap))
}

/// Index a single file through the full extract→chunk→embed→store pipeline.
pub async fn index_file(
    path: &std::path::Path,
    extractor: &CompositeExtractor,
    chunker: &dyn Chunker,
    embedder: &dyn Embedder,
    store: &syncmind_storage::VectorStore,
) -> anyhow::Result<()> {
    let text = extractor.extract(path)?;
    let chunks = chunker.chunk(&text, path);

    if chunks.is_empty() {
        return Ok(());
    }

    let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
    let embeddings = embedder.embed(&texts).await?;

    let metadata = std::fs::metadata(path)?;
    let last_modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let last_indexed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    let meta = syncmind_storage::FileMeta {
        absolute_path: path.to_path_buf(),
        file_type: path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_string(),
        last_modified,
        last_indexed,
    };

    store.upsert_file(&meta, &chunks, &embeddings)?;
    Ok(())
}

/// Run the indexing pipeline: receive file change events and route each to
/// either re-indexing or index cleanup based on the event kind.
///
/// `on_result` is invoked after every per-file Upsert indexing attempt so
/// callers (e.g. the desktop app) can update shared status state and emit
/// events. Remove events do not invoke the callback.
pub async fn run_indexing_pipeline(
    config: syncmind_core::Config,
    store: Arc<syncmind_storage::VectorStore>,
    embedder: Arc<dyn Embedder>,
    mut watcher_rx: mpsc::Receiver<Vec<FileEvent>>,
    on_result: Option<IndexResultCallback>,
) -> anyhow::Result<()> {
    let extractor = CompositeExtractor::new();

    while let Some(batch) = watcher_rx.recv().await {
        for event in batch {
            match event {
                FileEvent::Upsert(path) => {
                    let chunker = chunker_for_path(&path, config.chunk_size, config.chunk_overlap);
                    let result = index_file(
                        &path,
                        &extractor,
                        chunker.as_ref(),
                        embedder.as_ref(),
                        &store,
                    )
                    .await;
                    match &result {
                        Err(e) => warn!(path = %path.display(), error = %e, "failed to re-index file"),
                        Ok(()) => info!(path = %path.display(), "re-indexed file"),
                    }
                    if let Some(cb) = on_result.as_ref() {
                        cb(&path, result.as_ref().map(|_| ()));
                    }
                }
                FileEvent::Remove(path) => match store.delete_file_by_path(&path) {
                    Ok(true) => info!(path = %path.display(), "removed file from index"),
                    Ok(false) => info!(path = %path.display(), "remove event for unknown file (no-op)"),
                    Err(e) => warn!(path = %path.display(), error = %e, "failed to remove file from index"),
                },
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Compile-time check: `run_indexing_pipeline` accepts `Vec<FileEvent>`.
    #[allow(dead_code, clippy::let_underscore_future)]
    fn _signature_compiles(
        rx: mpsc::Receiver<Vec<FileEvent>>,
        store: Arc<syncmind_storage::VectorStore>,
        embedder: Arc<dyn Embedder>,
    ) {
        let _ = run_indexing_pipeline(
            syncmind_core::Config::default(),
            store,
            embedder,
            rx,
            None,
        );
    }

    #[allow(dead_code)]
    fn _file_event_variants_exist() {
        let _ = FileEvent::Upsert(PathBuf::from("/tmp/a"));
        let _ = FileEvent::Remove(PathBuf::from("/tmp/b"));
    }
}
