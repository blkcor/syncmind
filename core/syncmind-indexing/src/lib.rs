use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use syncmind_file_watcher::FileEvent;
use syncmind_rag_engine::chunker::{Chunker, CodeChunker, CssChunker, FallbackChunker, MarkdownChunker};
use syncmind_rag_engine::embedder::Embedder;
use syncmind_rag_engine::error::{EmbedError, ExtractError};
use syncmind_rag_engine::extractor::{CompositeExtractor, Extractor};
use syncmind_storage::StorageError;
use thiserror::Error;

/// Summary of a single-file indexing run, returned by [`index_file_once`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionReport {
    pub file_path: std::path::PathBuf,
    pub chunks_added: usize,
    pub bytes: u64,
    pub duration_ms: u128,
}

#[derive(Debug, Error)]
pub enum IndexingError {
    #[error("failed to extract text from {path}: {source}")]
    Extract {
        path: PathBuf,
        #[source]
        source: ExtractError,
    },
    #[error("failed to embed chunks for {path}: {source}")]
    Embed {
        path: PathBuf,
        #[source]
        source: EmbedError,
    },
    #[error("failed to read file metadata for {path}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to convert timestamp for {path}: {source}")]
    Timestamp {
        path: PathBuf,
        #[source]
        source: std::time::SystemTimeError,
    },
    #[error("failed to store indexed chunks for {path}: {source}")]
    Store {
        path: PathBuf,
        #[source]
        source: StorageError,
    },
}

/// Index a single file synchronously and report what was ingested.
///
/// Designed for callers (e.g. the desktop Spine client at `apps/desktop/src-tauri/src/spine/inbox.rs`)
/// that need to inject one file into the local index outside the file-watcher event stream and
/// receive a structured outcome they can surface to the user. Internally this calls
/// [`index_file`], so it inherits the same idempotency guarantee: re-running on the same path
/// replaces any prior chunks for that path via `VectorStore::upsert_file`.
///
/// The chunker is chosen by extension via [`chunker_for_path`] using the supplied
/// `chunk_size` / `chunk_overlap` (typically taken from `Config`).
pub async fn index_file_once(
    path: &std::path::Path,
    extractor: &CompositeExtractor,
    embedder: &dyn Embedder,
    store: &syncmind_storage::VectorStore,
    chunk_size: usize,
    chunk_overlap: usize,
) -> Result<IngestionReport, IndexingError> {
    let started = std::time::Instant::now();
    let chunker = chunker_for_path(path, chunk_size, chunk_overlap);

    let bytes =
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|source| IndexingError::Metadata {
                path: path.to_path_buf(),
                source,
            })?;

    let chunks_added = index_file(path, extractor, chunker.as_ref(), embedder, store).await?;

    Ok(IngestionReport {
        file_path: path.to_path_buf(),
        chunks_added,
        bytes,
        duration_ms: started.elapsed().as_millis(),
    })
}

/// Callback invoked after each file indexing attempt. The closure receives
/// the file path and the result. Used by the desktop app to update the
/// shared `IndexingState` and emit events to the frontend / tray.
pub type IndexResultCallback = Arc<dyn Fn(&Path, Result<(), &IndexingError>) + Send + Sync>;

/// Minimum number of non-whitespace characters a chunk must contain to be
/// considered semantically meaningful.  Chunks below this threshold are
/// almost always markdown fences, horizontal rules, or accidental whitespace
/// artefacts from the overlap algorithm.
const MIN_CHUNK_CONTENT_CHARS: usize = 20;

/// Returns `true` when a chunk carries enough non-trivial content to be worth
/// indexing.  A chunk consisting solely of a markdown code fence (```),
/// a horizontal rule, or whitespace is discarded.
fn is_meaningful_chunk(chunk: &syncmind_core::Chunk) -> bool {
    let stripped: String = chunk
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    // Bail out if the stripped text is just fences, stars, dashes, etc.
    if stripped
        .chars()
        .all(|c| matches!(c, '`' | '*' | '-' | '_' | '#' | '=' | '~' | '|' | '>'))
    {
        return false;
    }
    stripped.len() >= MIN_CHUNK_CONTENT_CHARS
}

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
        if ["css", "scss", "less"]
            .iter()
            .any(|&e| e.eq_ignore_ascii_case(ext))
        {
            return Box::new(CssChunker::new(chunk_size, chunk_overlap));
        }
        if [
            "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "c", "h", "cpp", "cc", "cxx",
            "hpp", "hh", "hxx", "cs", "rb", "php", "swift", "kt", "kts",
        ]
        .iter()
        .any(|&e| e.eq_ignore_ascii_case(ext))
        {
            return Box::new(CodeChunker::new(chunk_size, chunk_overlap));
        }
    }
    Box::new(FallbackChunker::new(chunk_size, chunk_overlap))
}

/// Index a single file through the full extract→chunk→embed→store pipeline.
/// Returns the number of chunks ingested (0 if the extractor produced empty text).
pub async fn index_file(
    path: &std::path::Path,
    extractor: &CompositeExtractor,
    chunker: &dyn Chunker,
    embedder: &dyn Embedder,
    store: &syncmind_storage::VectorStore,
) -> Result<usize, IndexingError> {
    let text = extractor
        .extract(path)
        .map_err(|source| IndexingError::Extract {
            path: path.to_path_buf(),
            source,
        })?;
    let mut chunks = chunker.chunk(&text, path);

    // Drop chunks whose non-whitespace content is below the minimum — these
    // are typically markdown fences, horizontal-rule separators, or trailing
    // whitespace that the overlap logic accidentally isolates into their own
    // chunk.  Indexing them pollutes search results with meaningless snippets.
    chunks.retain(is_meaningful_chunk);
    // Renumber after filtering so chunk_index stays sequential.
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = i;
    }

    if chunks.is_empty() {
        return Ok(0);
    }

    // Build embedding texts: prepend context_prefix (e.g. function signature)
    // so the embedding captures semantic context, while stored content stays pristine.
    let embed_texts: Vec<String> = chunks
        .iter()
        .map(|c| {
            if let Some(ref prefix) = c.context_prefix {
                format!("{}\n{}", prefix, c.content)
            } else {
                c.content.clone()
            }
        })
        .collect();
    let texts: Vec<&str> = embed_texts.iter().map(|s| s.as_str()).collect();
    let embeddings = embedder
        .embed(&texts)
        .await
        .map_err(|source| IndexingError::Embed {
            path: path.to_path_buf(),
            source,
        })?;

    let metadata = std::fs::metadata(path).map_err(|source| IndexingError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let last_modified = metadata
        .modified()
        .map_err(|source| IndexingError::Metadata {
            path: path.to_path_buf(),
            source,
        })?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|source| IndexingError::Timestamp {
            path: path.to_path_buf(),
            source,
        })?
        .as_secs() as i64;
    let last_indexed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|source| IndexingError::Timestamp {
            path: path.to_path_buf(),
            source,
        })?
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

    let chunk_count = chunks.len();
    store
        .upsert_file(&meta, &chunks, &embeddings)
        .map_err(|source| IndexingError::Store {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(chunk_count)
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
    let extractor = CompositeExtractor::from_config(&config);

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
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "failed to re-index file")
                        }
                        Ok(n) => info!(path = %path.display(), chunks = n, "re-indexed file"),
                    }
                    if let Some(cb) = on_result.as_ref() {
                        cb(&path, result.as_ref().map(|_| ()));
                    }
                }
                FileEvent::Remove(path) => match store.delete_file_by_path(&path) {
                    Ok(true) => info!(path = %path.display(), "removed file from index"),
                    Ok(false) => {
                        info!(path = %path.display(), "remove event for unknown file (no-op)")
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to remove file from index")
                    }
                },
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::fs;
    use syncmind_rag_engine::extractor::{CompositeExtractor, OcrConfig};

    struct FixedEmbedder {
        dim: usize,
    }

    #[async_trait]
    impl Embedder for FixedEmbedder {
        async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            Ok(texts.iter().map(|_| vec![0.25; self.dim]).collect())
        }

        fn embedding_dim(&self) -> usize {
            self.dim
        }
    }

    struct FailingEmbedder;

    #[async_trait]
    impl Embedder for FailingEmbedder {
        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            Err(EmbedError::OllamaUnavailable("offline fixture".into()))
        }

        fn embedding_dim(&self) -> usize {
            4
        }
    }

    fn minimal_text_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj".to_string(),
            "2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj".to_string(),
            "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj".to_string(),
            format!("4 0 obj << /Length {} >> stream\n{}\nendstream endobj", stream.len(), stream),
            "5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = vec![0usize];
        for object in &objects {
            offsets.push(pdf.len());
            pdf.push_str(object);
            pdf.push('\n');
        }
        let xref_offset = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for offset in offsets.iter().skip(1) {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer << /Root 1 0 R /Size {} >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_offset
        ));
        pdf.into_bytes()
    }

    // Compile-time check: `run_indexing_pipeline` accepts `Vec<FileEvent>`.
    #[allow(dead_code, clippy::let_underscore_future)]
    fn _signature_compiles(
        rx: mpsc::Receiver<Vec<FileEvent>>,
        store: Arc<syncmind_storage::VectorStore>,
        embedder: Arc<dyn Embedder>,
    ) {
        let _ = run_indexing_pipeline(syncmind_core::Config::default(), store, embedder, rx, None);
    }

    #[allow(dead_code)]
    fn _file_event_variants_exist() {
        let _ = FileEvent::Upsert(PathBuf::from("/tmp/a"));
        let _ = FileEvent::Remove(PathBuf::from("/tmp/b"));
    }

    #[test]
    fn chunker_for_path_routes_added_languages_to_code_chunker() {
        let temp = tempfile::tempdir().unwrap();
        for (ext, source, expected) in [
            ("java", "class Demo {\n  void run() {}\n}\n", "class Demo"),
            ("c", "int add(int a, int b) { return a + b; }\n", "int add"),
            ("h", "int add(int a, int b);\n", "int add"),
            ("cpp", "class Demo { void run() {} };\n", "class Demo"),
            ("cc", "class Demo { void run() {} };\n", "class Demo"),
            ("cxx", "class Demo { void run() {} };\n", "class Demo"),
            ("hpp", "class Demo { void run() {} };\n", "class Demo"),
            ("hh", "class Demo { void run() {} };\n", "class Demo"),
            ("hxx", "class Demo { void run() {} };\n", "class Demo"),
            ("cs", "class Demo { void Run() {} }\n", "class Demo"),
            ("rb", "class Demo\n  def run\n  end\nend\n", "class Demo"),
            (
                "php",
                "<?php\nfunction run_demo() { return 1; }\n",
                "function run_demo",
            ),
            ("swift", "struct Demo { func run() {} }\n", "struct Demo"),
            ("kt", "class Demo { fun run() {} }\n", "class Demo"),
            ("kts", "fun runDemo() = 1\n", "fun runDemo"),
        ] {
            let path = temp.path().join(format!("sample.{ext}"));
            let chunker = chunker_for_path(&path, 400, 40);
            let chunks = chunker.chunk(source, &path);
            assert!(
                chunks.iter().any(|chunk| chunk.content.contains(expected)),
                "{ext} should preserve a language-aware declaration chunk"
            );
        }
    }

    #[test]
    fn chunker_for_path_routes_stylesheets_to_css_chunker() {
        let temp = tempfile::tempdir().unwrap();
        for (ext, source, expected) in [
            ("css", ".card { color: red; }\n", ".card"),
            ("scss", ".card { &:hover { color: blue; } }\n", "&:hover"),
            ("less", ".card { color: @accent; }\n", "@accent"),
        ] {
            let path = temp.path().join(format!("sample.{ext}"));
            let chunker = chunker_for_path(&path, 400, 40);
            let chunks = chunker.chunk(source, &path);
            assert!(
                chunks.iter().any(|chunk| chunk.content.contains(expected)),
                "{ext} should be chunked as stylesheet content"
            );
        }
    }

    #[tokio::test]
    async fn indexes_mixed_documents_code_and_unsupported_files_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let clean_pdf = temp.path().join("clean.pdf");
        let java = temp.path().join("Example.java");
        let unsupported = temp.path().join("settings.toml");
        let db = temp.path().join("index.sqlite");

        fs::write(&clean_pdf, minimal_text_pdf("clean embedded pdf text")).unwrap();
        fs::write(&java, "public class Example {\n  public void run() {}\n}\n").unwrap();
        fs::write(&unsupported, "unsupported_text = \"still falls back\"").unwrap();

        let extractor = CompositeExtractor::with_ocr_config(OcrConfig {
            pdf_text_quality_threshold: 0.35,
            pdf_renderer_path: None,
            ocr_language: "eng".to_string(),
            ocr_psm_mode: 6,
            ocr_render_dpi: 300,
        });
        let embedder = FixedEmbedder { dim: 4 };
        let store = syncmind_storage::VectorStore::new(&db, embedder.embedding_dim()).unwrap();

        for path in [&clean_pdf, &java, &unsupported] {
            let chunker = chunker_for_path(path, 400, 40);
            index_file(path, &extractor, chunker.as_ref(), &embedder, &store)
                .await
                .unwrap_or_else(|error| panic!("failed to index {}: {error}", path.display()));
        }

        let results = store
            .search_hybrid(
                &[0.25; 4],
                "embedded OR Example OR unsupported",
                10,
                None,
            )
            .unwrap();
        for path in [&clean_pdf, &java, &unsupported] {
            assert!(
                results.iter().any(|result| result.file_path == *path),
                "expected indexed results for {}",
                path.display()
            );
        }

        let image = temp.path().join("scan.png");
        fs::write(&image, b"syncmind image ocr fixture").unwrap();
        let disabled_extractor = CompositeExtractor::with_ocr_config(OcrConfig::default());
        let chunker = chunker_for_path(&image, 400, 40);
        assert!(
            index_file(
                &image,
                &disabled_extractor,
                chunker.as_ref(),
                &embedder,
                &store
            )
            .await
            .is_err(),
            "invalid image OCR should fail only that file"
        );
    }

    #[tokio::test]
    async fn index_file_once_reports_chunks_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let note = temp.path().join("draft.md");
        let db = temp.path().join("index.sqlite");

        fs::write(
            &note,
            "# Heading\n\nFirst paragraph with enough words to make a chunk.\n\nSecond paragraph likewise has enough words to chunk.\n",
        )
        .unwrap();

        let extractor = CompositeExtractor::with_ocr_config(OcrConfig::default());
        let embedder = FixedEmbedder { dim: 4 };
        let store = syncmind_storage::VectorStore::new(&db, embedder.embedding_dim()).unwrap();

        let report = index_file_once(&note, &extractor, &embedder, &store, 64, 8)
            .await
            .unwrap();
        assert_eq!(report.file_path, note);
        assert!(
            report.chunks_added >= 1,
            "expected at least one chunk, got {}",
            report.chunks_added
        );
        assert!(report.bytes > 0, "bytes should reflect file size");

        // First run produced N chunks; second run on the same path must produce the
        // same number (idempotent via upsert_file's delete-then-insert semantics).
        let first_count = report.chunks_added;
        let report2 = index_file_once(&note, &extractor, &embedder, &store, 64, 8)
            .await
            .unwrap();
        assert_eq!(report2.chunks_added, first_count);
    }

    #[tokio::test]
    async fn index_file_once_surfaces_embedding_errors_as_structured_variant() {
        let temp = tempfile::tempdir().unwrap();
        let note = temp.path().join("draft.md");
        let db = temp.path().join("index.sqlite");

        fs::write(
            &note,
            "# Heading\n\nThis paragraph has enough content to produce a chunk.\n",
        )
        .unwrap();

        let extractor = CompositeExtractor::with_ocr_config(OcrConfig::default());
        let embedder = FailingEmbedder;
        let store = syncmind_storage::VectorStore::new(&db, embedder.embedding_dim()).unwrap();

        let err = index_file_once(&note, &extractor, &embedder, &store, 64, 8)
            .await
            .unwrap_err();

        match err {
            IndexingError::Embed { path, source } => {
                assert_eq!(path, note);
                assert!(source.to_string().contains("offline fixture"));
            }
            other => panic!("expected IndexingError::Embed, got {other:?}"),
        }
    }
}
