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
        if [
            "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "c", "h", "cpp", "cc",
            "cxx", "hpp", "hh", "hxx", "cs", "rb", "php", "swift", "kt", "kts",
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
    use async_trait::async_trait;
    use std::fs;
    use std::path::PathBuf;
    use syncmind_core::OcrMode;
    use syncmind_rag_engine::error::EmbedError;
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

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &std::path::Path) {}

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
        pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
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

    fn fake_tesseract(path: &std::path::Path) {
        fs::write(
            path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'tesseract fake'; exit 0; fi\necho 'ocr image text from local fixture'\n",
        )
        .unwrap();
        make_executable(path);
    }

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
            ("php", "<?php\nfunction run_demo() { return 1; }\n", "function run_demo"),
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

    #[tokio::test]
    async fn indexes_mixed_documents_code_and_unsupported_files_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let clean_pdf = temp.path().join("clean.pdf");
        let image = temp.path().join("scan.png");
        let java = temp.path().join("Example.java");
        let unsupported = temp.path().join("settings.toml");
        let db = temp.path().join("index.sqlite");
        let tesseract = temp.path().join("fake-tesseract");

        fs::write(&clean_pdf, minimal_text_pdf("clean embedded pdf text")).unwrap();
        fs::write(&image, b"not a real image; fake tesseract ignores input").unwrap();
        fs::write(
            &java,
            "public class Example {\n  public void run() {}\n}\n",
        )
        .unwrap();
        fs::write(&unsupported, "unsupported_text = \"still falls back\"").unwrap();
        fake_tesseract(&tesseract);

        let extractor = CompositeExtractor::with_ocr_config(OcrConfig {
            mode: OcrMode::Auto,
            pdf_text_quality_threshold: 0.35,
            ocr_binary_path: Some(tesseract),
            pdf_renderer_path: None,
        });
        let embedder = FixedEmbedder { dim: 4 };
        let store = syncmind_storage::VectorStore::new(&db, embedder.embedding_dim()).unwrap();

        for path in [&clean_pdf, &image, &java, &unsupported] {
            let chunker = chunker_for_path(path, 400, 40);
            index_file(path, &extractor, chunker.as_ref(), &embedder, &store)
                .await
                .unwrap_or_else(|error| panic!("failed to index {}: {error}", path.display()));
        }

        let results = store
            .search_hybrid(&[0.25; 4], "embedded OR ocr OR Example OR unsupported", 10, None)
            .unwrap();
        for path in [&clean_pdf, &image, &java, &unsupported] {
            assert!(
                results.iter().any(|result| result.file_path == *path),
                "expected indexed results for {}",
                path.display()
            );
        }

        let disabled_extractor = CompositeExtractor::with_ocr_config(OcrConfig {
            mode: OcrMode::Disabled,
            ..OcrConfig::default()
        });
        let chunker = chunker_for_path(&image, 400, 40);
        assert!(
            index_file(&image, &disabled_extractor, chunker.as_ref(), &embedder, &store)
                .await
                .is_err(),
            "OCR-disabled image should fail only that file"
        );
    }
}
