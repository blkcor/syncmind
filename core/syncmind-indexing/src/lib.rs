use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use syncmind_rag_engine::chunker::{Chunker, CodeChunker, FallbackChunker, MarkdownChunker};
use syncmind_rag_engine::embedder::Embedder;
use syncmind_rag_engine::extractor::{CompositeExtractor, Extractor};

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

/// Run the indexing pipeline: receive file change events and index each changed file.
pub async fn run_indexing_pipeline(
    config: syncmind_core::Config,
    store: Arc<syncmind_storage::VectorStore>,
    embedder: Arc<dyn Embedder>,
    mut watcher_rx: mpsc::Receiver<Vec<PathBuf>>,
) -> anyhow::Result<()> {
    let extractor = CompositeExtractor::new();

    while let Some(batch) = watcher_rx.recv().await {
        for path in batch {
            let chunker = chunker_for_path(&path, config.chunk_size, config.chunk_overlap);
            if let Err(e) = index_file(&path, &extractor, chunker.as_ref(), embedder.as_ref(), &store).await {
                warn!(path = %path.display(), error = %e, "failed to re-index file");
            } else {
                info!(path = %path.display(), "re-indexed file");
            }
        }
    }

    Ok(())
}
