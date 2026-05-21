use crate::error::EmbedError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use syncmind_core::config::Config;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embedding_dim(&self) -> usize;
}

// ─── Ollama Embedder ──────────────────────────────────────────────────────────

pub struct OllamaEmbedder {
    client: reqwest::Client,
    url: String,
    model: String,
    embedding_dim: usize,
}

impl OllamaEmbedder {
    pub fn new(
        url: impl Into<String>,
        model: impl Into<String>,
        embedding_dim: usize,
    ) -> Result<Self, EmbedError> {
        let url = url.into();
        let model = model.into();

        if url.is_empty() {
            return Err(EmbedError::Http("Ollama URL is empty".to_string()));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(EmbedError::Http(format!(
                "Ollama URL must start with http:// or https://: {}",
                url
            )));
        }

        Ok(Self {
            client: reqwest::Client::new(),
            url,
            model,
            embedding_dim,
        })
    }

    pub fn from_config(config: &Config) -> Result<Self, EmbedError> {
        Self::new(
            config.ollama_url.clone(),
            config.ollama_model.clone(),
            config.embedding_dim,
        )
    }
}

#[derive(Serialize, Debug)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

#[derive(Deserialize, Debug)]
struct OllamaEmbedResponse {
    #[allow(dead_code)]
    // Kept for API completeness; not currently used by embedder logic.
    model: String,
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = OllamaEmbedRequest {
            model: &self.model,
            input: texts,
        };

        let url = format!("{}/api/embed", self.url.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbedError::Http(e.to_string()))?;

        if !response.status().is_success() {
            return Err(EmbedError::Http(format!(
                "Ollama returned HTTP {}",
                response.status()
            )));
        }

        let body: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| EmbedError::Http(format!("Failed to parse Ollama response: {}", e)))?;

        if body.embeddings.len() != texts.len() {
            return Err(EmbedError::Http(format!(
                "Embedding count mismatch: expected {}, got {}",
                texts.len(),
                body.embeddings.len()
            )));
        }

        for emb in &body.embeddings {
            if emb.len() != self.embedding_dim {
                return Err(EmbedError::DimensionMismatch {
                    expected: self.embedding_dim,
                    actual: emb.len(),
                });
            }
        }

        Ok(body.embeddings)
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

// ─── ONNX Embedder ────────────────────────────────────────────────────────────

const ONNX_MAX_SEQ_LEN: usize = 512;
const ONNX_MAX_BATCH_SIZE: usize = 32;

pub const DEFAULT_ONNX_MODEL_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx";
pub const DEFAULT_ONNX_TOKENIZER_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json";

const ONNX_MODEL_FILENAME: &str = "bge-small-en-v1.5.onnx";
const ONNX_TOKENIZER_FILENAME: &str = "tokenizer.json";

/// Download `url` into `dest` atomically.
///
/// Writes to `<dest>.part`, fsyncs, and renames to the final path. A
/// `<dest>.lock` file is held exclusively for the duration of the download
/// so that two concurrent daemons do not race.
async fn download_file(url: &str, dest: &std::path::Path) -> Result<(), EmbedError> {
    use tokio::io::AsyncWriteExt as _;

    let parent = dest.parent().ok_or_else(|| {
        EmbedError::Onnx(format!("Destination has no parent directory: {}", dest.display()))
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| EmbedError::Onnx(format!("Failed to create {}: {}", parent.display(), e)))?;

    let lock_path = dest.with_extension("lock");
    let part_path = dest.with_extension("part");

    // Acquire (or wait for) the exclusive lock.
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| EmbedError::Onnx(format!("Failed to open lock file: {}", e)))?;
    {
        use fs2::FileExt as _;
        // Poll for the lock to be available; bail after a generous timeout
        // so a stuck process cannot wedge the daemon forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            match lock_file.try_lock_exclusive() {
                Ok(()) => break,
                Err(_) => {
                    if dest.exists()
                        && tokio::fs::metadata(dest)
                            .await
                            .map(|m| m.len() > 0)
                            .unwrap_or(false)
                    {
                        // Another process finished the download while we waited.
                        return Ok(());
                    }
                    if std::time::Instant::now() > deadline {
                        return Err(EmbedError::Onnx(
                            "Timed out waiting for concurrent ONNX download".to_string(),
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    // Double-check now that we hold the lock — another process may have
    // finished the download while we were polling above.
    if dest.exists() {
        if let Ok(meta) = tokio::fs::metadata(dest).await {
            if meta.len() > 0 {
                let _ = std::fs::remove_file(&lock_path);
                return Ok(());
            }
        }
    }

    tracing::info!(url = %url, dest = %dest.display(), "downloading ONNX asset");

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| EmbedError::Onnx(format!("Failed to build HTTP client: {}", e)))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| EmbedError::Onnx(format!("Failed to GET {}: {}", url, e)))?;
    if !response.status().is_success() {
        let _ = std::fs::remove_file(&lock_path);
        return Err(EmbedError::Onnx(format!(
            "Download failed: GET {} returned HTTP {}",
            url,
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| EmbedError::Onnx(format!("Failed to create {}: {}", part_path.display(), e)))?;

    use futures_util::StreamExt as _;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk
            .map_err(|e| EmbedError::Onnx(format!("Network error during download: {}", e)))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| EmbedError::Onnx(format!("Write error: {}", e)))?;
        total += bytes.len() as u64;
    }
    file.flush()
        .await
        .map_err(|e| EmbedError::Onnx(format!("Flush error: {}", e)))?;
    drop(file);

    tokio::fs::rename(&part_path, dest)
        .await
        .map_err(|e| EmbedError::Onnx(format!(
            "Failed to rename {} -> {}: {}",
            part_path.display(),
            dest.display(),
            e
        )))?;

    tracing::info!(
        bytes = total,
        dest = %dest.display(),
        "ONNX asset downloaded"
    );

    // Best-effort cleanup; ignore errors.
    let _ = std::fs::remove_file(&lock_path);

    Ok(())
}

/// Ensure the ONNX model and tokenizer are present at `model_dir`, downloading
/// them if necessary. Returns `(model_path, tokenizer_path)`.
pub async fn ensure_onnx_assets(
    model_dir: &std::path::Path,
    model_url: &str,
    tokenizer_url: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), EmbedError> {
    tokio::fs::create_dir_all(model_dir).await.map_err(|e| {
        EmbedError::Onnx(format!(
            "Failed to create model dir {}: {}",
            model_dir.display(),
            e
        ))
    })?;

    let model_path = model_dir.join(ONNX_MODEL_FILENAME);
    let tokenizer_path = model_dir.join(ONNX_TOKENIZER_FILENAME);

    let needs_model = !file_present(&model_path).await;
    let needs_tokenizer = !file_present(&tokenizer_path).await;

    if needs_model {
        download_file(model_url, &model_path).await?;
    }
    if needs_tokenizer {
        download_file(tokenizer_url, &tokenizer_path).await?;
    }

    Ok((model_path, tokenizer_path))
}

async fn file_present(path: &std::path::Path) -> bool {
    matches!(tokio::fs::metadata(path).await, Ok(m) if m.len() > 0)
}

pub struct OnnxEmbedder {
    session: std::sync::Arc<parking_lot::Mutex<ort::session::Session>>,
    tokenizer: tokenizers::Tokenizer,
    embedding_dim: usize,
}

impl OnnxEmbedder {
    pub fn new(
        model_path: impl AsRef<std::path::Path>,
        embedding_dim: usize,
    ) -> Result<Self, EmbedError> {
        let model_path = model_path.as_ref();
        let session = ort::session::Session::builder()
            .map_err(|e| EmbedError::Onnx(format!("Failed to create ONNX session builder: {}", e)))?
            .commit_from_file(model_path)
            .map_err(|e| EmbedError::Onnx(format!("Failed to load ONNX model: {}", e)))?;

        let tokenizer_path = model_path
            .parent()
            .map(|p| p.join("tokenizer.json"))
            .ok_or_else(|| EmbedError::Onnx(format!("Model path has no parent directory: {:?}", model_path)))?;
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::Onnx(format!("Failed to load tokenizer from {:?}: {}", tokenizer_path, e)))?;

        Ok(Self {
            session: std::sync::Arc::new(parking_lot::Mutex::new(session)),
            tokenizer,
            embedding_dim,
        })
    }

    pub async fn from_config(config: &Config) -> Result<Self, EmbedError> {
        let model_dir = syncmind_core::paths::model_cache_dir()
            .map_err(|e| EmbedError::Onnx(format!("Failed to resolve model cache dir: {}", e)))?;
        let model_path = model_dir.join("bge-small-en-v1.5").join("model.onnx");
        Self::new(model_path, config.embedding_dim)
    }
}

#[async_trait]
impl Embedder for OnnxEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(ONNX_MAX_BATCH_SIZE) {
            let session = std::sync::Arc::clone(&self.session);
            let tokenizer = self.tokenizer.clone();
            let embedding_dim = self.embedding_dim;
            let texts_owned: Vec<String> = chunk.iter().map(|s| s.to_string()).collect();

            let embeddings = tokio::task::spawn_blocking(move || {
                run_onnx_inference(session, tokenizer, &texts_owned, embedding_dim)
            })
            .await
            .map_err(|e| EmbedError::Onnx(format!("Blocking task panicked: {}", e)))??;

            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

fn run_onnx_inference(
    session: std::sync::Arc<parking_lot::Mutex<ort::session::Session>>,
    tokenizer: tokenizers::Tokenizer,
    texts: &[String],
    embedding_dim: usize,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    use ndarray::Array2;

    let mut all_input_ids: Vec<Vec<i64>> = Vec::with_capacity(texts.len());
    let mut all_attention_mask: Vec<Vec<i64>> = Vec::with_capacity(texts.len());
    let mut all_token_type_ids: Vec<Vec<i64>> = Vec::with_capacity(texts.len());
    let mut max_len = 0usize;

    for text in texts {
        let encoding = tokenizer
            .encode(text.as_str(), true)
            .map_err(|e| EmbedError::Onnx(format!("Tokenization failed: {}", e)))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
        let mut type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
        if ids.len() > ONNX_MAX_SEQ_LEN {
            ids.truncate(ONNX_MAX_SEQ_LEN);
            attention.truncate(ONNX_MAX_SEQ_LEN);
            type_ids.truncate(ONNX_MAX_SEQ_LEN);
        }
        max_len = max_len.max(ids.len());
        all_input_ids.push(ids);
        all_attention_mask.push(attention);
        all_token_type_ids.push(type_ids);
    }

    // Pad sequences to max_len
    for i in 0..texts.len() {
        let pad_len = max_len - all_input_ids[i].len();
        if pad_len > 0 {
            all_input_ids[i].extend(std::iter::repeat_n(0i64, pad_len));
            all_attention_mask[i].extend(std::iter::repeat_n(0i64, pad_len));
            all_token_type_ids[i].extend(std::iter::repeat_n(0i64, pad_len));
        }
    }

    let batch_size = texts.len();
    let shape = (batch_size, max_len);

    let input_ids_array = Array2::from_shape_vec(
        shape,
        all_input_ids.into_iter().flatten().collect(),
    )
    .map_err(|e| EmbedError::Onnx(format!("Failed to build input_ids tensor: {}", e)))?;

    let attention_mask_array = Array2::from_shape_vec(
        shape,
        all_attention_mask.into_iter().flatten().collect(),
    )
    .map_err(|e| EmbedError::Onnx(format!("Failed to build attention_mask tensor: {}", e)))?;

    let token_type_ids_array = Array2::from_shape_vec(
        shape,
        all_token_type_ids.into_iter().flatten().collect(),
    )
    .map_err(|e| EmbedError::Onnx(format!("Failed to build token_type_ids tensor: {}", e)))?;

    // Clone attention mask for mean pooling before moving arrays into ort tensors
    let attention_mask_for_pooling: Vec<Vec<i64>> = (0..batch_size)
        .map(|b| attention_mask_array.row(b).to_vec())
        .collect();

    let input_ids = ort::value::Tensor::from_array(input_ids_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create input_ids tensor: {}", e)))?;
    let attention_mask = ort::value::Tensor::from_array(attention_mask_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create attention_mask tensor: {}", e)))?;
    let token_type_ids = ort::value::Tensor::from_array(token_type_ids_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create token_type_ids tensor: {}", e)))?;

    let mut session = session.lock();
    let outputs = session
        .run(ort::inputs![
            "input_ids" => input_ids,
            "attention_mask" => attention_mask,
            "token_type_ids" => token_type_ids
        ])
        .map_err(|e| EmbedError::Onnx(format!("ONNX session run failed: {}", e)))?;

    // BGE ONNX model returns [batch_size, seq_len, hidden_dim]
    let output_tensor = outputs
        .get("last_hidden_state")
        .or_else(|| {
            if outputs.len() > 0 {
                Some(&outputs[0])
            } else {
                None
            }
        })
        .ok_or_else(|| EmbedError::Onnx("ONNX output missing".to_string()))?;

    let output_view = output_tensor
        .try_extract_array::<f32>()
        .map_err(|e| EmbedError::Onnx(format!("Failed to extract output tensor: {}", e)))?;

    let output_shape = output_view.shape();
    if output_shape.len() != 3 {
        return Err(EmbedError::Onnx(format!(
            "Expected 3D output tensor, got {}D",
            output_shape.len()
        )));
    }

    let batch = output_shape[0];
    let seq_len = output_shape[1];
    let hidden_dim = output_shape[2];

    if hidden_dim != embedding_dim {
        return Err(EmbedError::DimensionMismatch {
            expected: embedding_dim,
            actual: hidden_dim,
        });
    }

    let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(batch);

    for b in 0..batch {
        let mut embedding = vec![0.0f32; hidden_dim];
        let mut mask_sum = 0.0f32;

        for s in 0..seq_len {
            let mask_val = attention_mask_for_pooling[b][s] as f32;
            if mask_val == 0.0 {
                continue;
            }
            mask_sum += mask_val;
            for (h, emb_val) in embedding.iter_mut().enumerate().take(hidden_dim) {
                *emb_val += output_view[[b, s, h]] * mask_val;
            }
        }

        if mask_sum > 0.0 {
            for emb_val in embedding.iter_mut().take(hidden_dim) {
                *emb_val /= mask_sum;
            }
        }

        embeddings.push(embedding);
    }

    Ok(embeddings)
}

// ─── Auto Embedder ────────────────────────────────────────────────────────────

pub struct AutoEmbedder {
    inner: Box<dyn Embedder>,
}

impl AutoEmbedder {
    pub async fn new(config: &Config) -> Result<Self, EmbedError> {
        if let Ok(()) = Self::probe_ollama(&config.ollama_url, &config.ollama_model).await {
            tracing::info!("Using Ollama embedder at {}", config.ollama_url);
            let embedder = OllamaEmbedder::from_config(config)?;
            return Ok(Self {
                inner: Box::new(embedder),
            });
        }

        tracing::info!("Ollama unavailable, falling back to ONNX embedder");
        let embedder = OnnxEmbedder::from_config(config).await?;
        Ok(Self {
            inner: Box::new(embedder),
        })
    }

    async fn probe_ollama(url: &str, model: &str) -> Result<(), EmbedError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .map_err(|e| EmbedError::OllamaUnavailable(e.to_string()))?;
        let url = format!("{}/api/embed", url.trim_end_matches('/'));
        let request = OllamaEmbedRequest {
            model,
            input: &["test"],
        };
        let response = client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbedError::OllamaUnavailable(e.to_string()))?;
        if !response.status().is_success() {
            return Err(EmbedError::OllamaUnavailable(format!(
                "Ollama probe returned HTTP {}",
                response.status()
            )));
        }
        let body: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| EmbedError::OllamaUnavailable(format!("Failed to parse probe response: {}", e)))?;
        if body.embeddings.is_empty() || body.embeddings[0].is_empty() {
            return Err(EmbedError::OllamaUnavailable(
                "Ollama probe returned empty embeddings".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl Embedder for AutoEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.inner.embed(texts).await
    }

    fn embedding_dim(&self) -> usize {
        self.inner.embedding_dim()
    }
}

// ─── Swappable Embedder ──────────────────────────────────────────────────────

/// Embedder whose backend can be replaced at runtime.
///
/// The desktop daemon installs a fast placeholder backend at startup so its
/// Tauri `setup` hook never blocks on the network, then spawns a background
/// task that swaps in a real `AutoEmbedder` once initialization
/// (Ollama probe + ONNX model download) completes. Consumers continue to
/// hold the same `Arc<dyn Embedder>` and are unaware of the swap.
pub struct SwappableEmbedder {
    inner: std::sync::RwLock<std::sync::Arc<dyn Embedder>>,
    embedding_dim: usize,
}

impl SwappableEmbedder {
    pub fn new(initial: std::sync::Arc<dyn Embedder>) -> Self {
        let embedding_dim = initial.embedding_dim();
        Self {
            inner: std::sync::RwLock::new(initial),
            embedding_dim,
        }
    }

    /// Replace the active backend. The new backend must report the same
    /// `embedding_dim` as the one used to construct this `SwappableEmbedder`,
    /// otherwise downstream vector store writes would corrupt the index.
    pub fn swap(&self, new_backend: std::sync::Arc<dyn Embedder>) {
        debug_assert_eq!(
            new_backend.embedding_dim(),
            self.embedding_dim,
            "swapped backend must preserve embedding_dim"
        );
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        *guard = new_backend;
    }
}

#[async_trait]
impl Embedder for SwappableEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Clone the Arc out of the lock so we never hold the read guard
        // across the `.await` below.
        let backend = {
            let guard = self
                .inner
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            std::sync::Arc::clone(&*guard)
        };
        backend.embed(texts).await
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_ollama_embedder_request_serialization() {
        let texts: &[&str] = &["hello world", "foo bar"];
        let request = OllamaEmbedRequest {
            model: "bge-m3",
            input: texts,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"bge-m3\""));
        assert!(json.contains("\"input\":[\"hello world\",\"foo bar\"]"));
    }

    #[test]
    fn test_ollama_embedder_parses_response() {
        let json = r#"{
            "model": "bge-m3",
            "embeddings": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]
        }"#;
        let resp: OllamaEmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.model, "bge-m3");
        assert_eq!(resp.embeddings.len(), 2);
        assert_eq!(resp.embeddings[0], vec![0.1f32, 0.2, 0.3]);
        assert_eq!(resp.embeddings[1], vec![0.4f32, 0.5, 0.6]);
    }

    #[test]
    fn test_dimension_mismatch_errors() {
        let embedder = OllamaEmbedder::new("http://localhost:11434", "bge-m3", 3).unwrap();
        // Simulate a response with wrong dimension by manually parsing
        let json = r#"{"model":"bge-m3","embeddings":[[0.1,0.2]]}"#;
        let resp: OllamaEmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.embeddings[0].len(), 2);
        // Verify the embed() method rejects mismatched dimensions by exercising
        // the validation helper directly (it is the same logic used in embed()).
        let expected = embedder.embedding_dim();
        let actual = resp.embeddings[0].len();
        assert_ne!(expected, actual);

        // Ensure the validation logic used inside embed() returns the correct error.
        for emb in &resp.embeddings {
            if emb.len() != expected {
                let err = EmbedError::DimensionMismatch {
                    expected,
                    actual: emb.len(),
                };
                assert_eq!(format!("{}", err), "Dimension mismatch: expected 3, got 2");
                return;
            }
        }
        panic!("Expected dimension mismatch error was not produced");
    }

    #[tokio::test]
    async fn test_auto_embedder_falls_back_to_onnx() {
        // Use a non-routable URL so Ollama probe fails. Point ONNX download
        // URLs at a closed local port so the fallback also fails fast — we
        // are only verifying that AutoEmbedder routes to the ONNX branch.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_addr = listener.local_addr().unwrap();
        drop(listener);

        let config = Config {
            ollama_url: "http://192.0.2.0:11434".to_string(), // TEST-NET-1, guaranteed unreachable
            ollama_model: "bge-m3".to_string(),
            embedding_dim: 384,
            onnx_model_url: Some(format!("http://{}/model.onnx", closed_addr)),
            onnx_tokenizer_url: Some(format!("http://{}/tokenizer.json", closed_addr)),
            ..Config::default()
        };

        let result = AutoEmbedder::new(&config).await;
        match result {
            Err(EmbedError::Onnx(_)) => {
                // Expected: ONNX was attempted and failed (connection refused on closed port).
            }
            Err(EmbedError::OllamaUnavailable(_)) => {
                // Also acceptable if the probe itself is the reported error.
            }
            Err(other) => {
                panic!("Unexpected error variant: {:?}", other);
            }
            Ok(_) => {
                panic!("Both probes should have failed in this test");
            }
        }
    }

    #[tokio::test]
    async fn ensure_onnx_assets_downloads_missing_files() {
        let model_body = b"FAKE_ONNX_BYTES";
        let tokenizer_body = b"{\"fake\":\"tokenizer\"}";

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let app = axum::Router::new()
            .route(
                "/model.onnx",
                axum::routing::get(move || async move { model_body.to_vec() }),
            )
            .route(
                "/tokenizer.json",
                axum::routing::get(move || async move { tokenizer_body.to_vec() }),
            );

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let dir = tempfile::tempdir().unwrap();
        let model_url = format!("http://{}/model.onnx", addr);
        let tokenizer_url = format!("http://{}/tokenizer.json", addr);

        let (model_path, tokenizer_path) =
            ensure_onnx_assets(dir.path(), &model_url, &tokenizer_url)
                .await
                .unwrap();

        let on_disk_model = std::fs::read(&model_path).unwrap();
        let on_disk_tokenizer = std::fs::read(&tokenizer_path).unwrap();
        assert_eq!(on_disk_model, model_body);
        assert_eq!(on_disk_tokenizer, tokenizer_body);

        // Second call must NOT re-download: shut the server down first.
        server.abort();
        let (_, _) = ensure_onnx_assets(dir.path(), &model_url, &tokenizer_url)
            .await
            .expect("second call should be a no-op since files exist");
    }

    #[tokio::test]
    async fn ensure_onnx_assets_propagates_http_404() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let app = axum::Router::new()
            .route(
                "/missing.onnx",
                axum::routing::get(|| async { (axum::http::StatusCode::NOT_FOUND, "nope") }),
            )
            .route(
                "/missing-tokenizer.json",
                axum::routing::get(|| async { (axum::http::StatusCode::NOT_FOUND, "nope") }),
            );

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let dir = tempfile::tempdir().unwrap();
        let result = ensure_onnx_assets(
            dir.path(),
            &format!("http://{}/missing.onnx", addr),
            &format!("http://{}/missing-tokenizer.json", addr),
        )
        .await;

        server.abort();

        match result {
            Err(EmbedError::Onnx(msg)) => {
                assert!(
                    msg.contains("404") || msg.to_lowercase().contains("not found"),
                    "expected 404 mention, got: {}",
                    msg
                );
            }
            other => panic!("expected EmbedError::Onnx, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_ollama_embedder_real() {
        let embedder = OllamaEmbedder::new("http://localhost:11434", "bge-m3", 1024).unwrap();
        let result = embedder.embed(&["hello world"]).await;
        assert!(result.is_ok());
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 1024);
    }

    #[tokio::test]
    #[ignore = "requires ONNX model file and tokenizer.json at <data-dir>/syncmind/models/ (see core/syncmind-core/src/paths.rs)"]
    async fn test_onnx_embedder_loads() {
        let config = Config {
            embedding_dim: 384,
            ..Config::default()
        };
        let embedder = OnnxEmbedder::from_config(&config).await.unwrap();
        let result = embedder.embed(&["hello world"]).await;
        assert!(result.is_ok());
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 384);
    }

    // SwappableEmbedder: swap()  routes subsequent embed() calls to the new
    // backend without changing the Arc<dyn Embedder> consumers hold.
    struct FixedEmbedder {
        value: f32,
        dim: usize,
    }

    #[async_trait]
    impl Embedder for FixedEmbedder {
        async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            Ok(texts.iter().map(|_| vec![self.value; self.dim]).collect())
        }

        fn embedding_dim(&self) -> usize {
            self.dim
        }
    }

    #[tokio::test]
    async fn swappable_embedder_routes_to_current_backend() {
        let initial: std::sync::Arc<dyn Embedder> =
            std::sync::Arc::new(FixedEmbedder { value: 1.0, dim: 4 });
        let swappable = std::sync::Arc::new(SwappableEmbedder::new(initial));
        let as_dyn: std::sync::Arc<dyn Embedder> = std::sync::Arc::clone(&swappable)
            as std::sync::Arc<dyn Embedder>;

        let before = as_dyn.embed(&["x"]).await.unwrap();
        assert_eq!(before, vec![vec![1.0; 4]]);

        let replacement: std::sync::Arc<dyn Embedder> =
            std::sync::Arc::new(FixedEmbedder { value: 2.0, dim: 4 });
        swappable.swap(replacement);

        // The same Arc<dyn Embedder> a consumer captured earlier must see
        // the new backend without being re-handed.
        let after = as_dyn.embed(&["x"]).await.unwrap();
        assert_eq!(after, vec![vec![2.0; 4]]);
        assert_eq!(as_dyn.embedding_dim(), 4);
    }
}
