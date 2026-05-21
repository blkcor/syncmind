use crate::error::EmbedError;
use async_trait::async_trait;
use std::path::Path;

const RERANKER_MAX_SEQ_LEN: usize = 512;
const RERANKER_MAX_BATCH_SIZE: usize = 32;
const MEMORY_BUDGET_MB: u64 = 600;

/// A reranker scores query-passage pairs to refine an initial retrieval result set.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Score each passage relative to the query.
    ///
    /// Returns a vector of scores in `[0.0, 1.0]` where higher means more relevant.
    /// The output order matches the input `passages` order.
    async fn rerank(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>, EmbedError>;
}

/// Local ONNX cross-encoder reranker.
///
/// Expects a model directory containing:
/// - `<model_name>.onnx`
/// - `tokenizer.json`
///
/// The model is loaded with a memory-budget check: if the estimated working
/// set (2x file size) exceeds 150 MB the load is rejected.
pub struct OnnxReranker {
    session: std::sync::Arc<parking_lot::Mutex<ort::session::Session>>,
    tokenizer: tokenizers::Tokenizer,
}

impl OnnxReranker {
    pub fn new(model_path: impl AsRef<Path>) -> Result<Self, EmbedError> {
        let model_path = model_path.as_ref();
        check_memory_budget(model_path)?;

        let session = ort::session::Session::builder()
            .map_err(|e| {
                EmbedError::Onnx(format!("Failed to create ONNX session builder: {}", e))
            })?
            .commit_from_file(model_path)
            .map_err(|e| EmbedError::Onnx(format!("Failed to load ONNX model: {}", e)))?;

        let tokenizer_path = model_path
            .parent()
            .map(|p| p.join("tokenizer.json"))
            .ok_or_else(|| {
                EmbedError::Onnx(format!("Model path has no parent directory: {:?}", model_path))
            })?;
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::Onnx(format!("Failed to load tokenizer from {:?}: {}", tokenizer_path, e)))?;

        Ok(Self {
            session: std::sync::Arc::new(parking_lot::Mutex::new(session)),
            tokenizer,
        })
    }
}

fn check_memory_budget(model_path: &Path) -> Result<(), EmbedError> {
    let metadata = std::fs::metadata(model_path)?;
    let size_mb = metadata.len() / (1024 * 1024);
    let estimated_mb = size_mb.saturating_mul(2);
    if estimated_mb > MEMORY_BUDGET_MB {
        return Err(EmbedError::Onnx(format!(
            "Reranker model memory estimate ({} MB) exceeds budget ({} MB)",
            estimated_mb, MEMORY_BUDGET_MB
        )));
    }
    Ok(())
}

#[async_trait]
impl Reranker for OnnxReranker {
    async fn rerank(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>, EmbedError> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_scores: Vec<f32> = Vec::with_capacity(passages.len());

        for chunk in passages.chunks(RERANKER_MAX_BATCH_SIZE) {
            let session = std::sync::Arc::clone(&self.session);
            let tokenizer = self.tokenizer.clone();
            let query = query.to_string();
            let passages_owned: Vec<String> = chunk.iter().map(|s| s.to_string()).collect();

            let scores = tokio::task::spawn_blocking(move || {
                run_reranker_inference(session, tokenizer, &query, &passages_owned)
            })
            .await
            .map_err(|e| EmbedError::Onnx(format!("Blocking task panicked: {}", e)))??;

            all_scores.extend(scores);
        }

        Ok(all_scores)
    }
}

fn run_reranker_inference(
    session: std::sync::Arc<parking_lot::Mutex<ort::session::Session>>,
    tokenizer: tokenizers::Tokenizer,
    query: &str,
    passages: &[String],
) -> Result<Vec<f32>, EmbedError> {
    use ndarray::Array2;

    let mut all_input_ids: Vec<Vec<i64>> = Vec::with_capacity(passages.len());
    let mut all_attention_mask: Vec<Vec<i64>> = Vec::with_capacity(passages.len());
    let mut all_token_type_ids: Vec<Vec<i64>> = Vec::with_capacity(passages.len());
    let mut max_len = 0usize;

    for passage in passages {
        let encoding = tokenizer
            .encode(
                tokenizers::EncodeInput::Dual(query.into(), passage.as_str().into()),
                true,
            )
            .map_err(|e| EmbedError::Onnx(format!("Tokenization failed: {}", e)))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let mut type_ids: Vec<i64> = encoding
            .get_type_ids()
            .iter()
            .map(|&t| t as i64)
            .collect();
        if ids.len() > RERANKER_MAX_SEQ_LEN {
            ids.truncate(RERANKER_MAX_SEQ_LEN);
            attention.truncate(RERANKER_MAX_SEQ_LEN);
            type_ids.truncate(RERANKER_MAX_SEQ_LEN);
        }
        max_len = max_len.max(ids.len());
        all_input_ids.push(ids);
        all_attention_mask.push(attention);
        all_token_type_ids.push(type_ids);
    }

    // Pad sequences to max_len within the batch.
    for i in 0..passages.len() {
        let pad_len = max_len - all_input_ids[i].len();
        if pad_len > 0 {
            all_input_ids[i].extend(std::iter::repeat_n(0i64, pad_len));
            all_attention_mask[i].extend(std::iter::repeat_n(0i64, pad_len));
            all_token_type_ids[i].extend(std::iter::repeat_n(0i64, pad_len));
        }
    }

    let batch_size = passages.len();
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

    let input_ids = ort::value::Tensor::from_array(input_ids_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create input_ids tensor: {}", e)))?;
    let attention_mask = ort::value::Tensor::from_array(attention_mask_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create attention_mask tensor: {}", e)))?;
    let token_type_ids = ort::value::Tensor::from_array(token_type_ids_array)
        .map_err(|e| EmbedError::Onnx(format!("Failed to create token_type_ids tensor: {}", e)))?;

    let mut session = session.lock();

    // Some ONNX exports (e.g. Xenova/transformers.js) omit token_type_ids.
    // Inspect the model inputs to decide what to feed.
    let has_token_type_ids = session.inputs().iter().any(|i| i.name() == "token_type_ids");

    let outputs = if has_token_type_ids {
        session
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "token_type_ids" => token_type_ids
            ])
            .map_err(|e| EmbedError::Onnx(format!("ONNX session run failed: {}", e)))?
    } else {
        session
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask
            ])
            .map_err(|e| EmbedError::Onnx(format!("ONNX session run failed: {}", e)))?
    };

    let output_tensor = outputs
        .get("logits")
        .or_else(|| outputs.get("output"))
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
    let batch = output_shape.first().copied().unwrap_or(1);

    let mut scores = Vec::with_capacity(batch);

    match output_shape.len() {
        1 => {
            for b in 0..batch {
                scores.push(output_view[b]);
            }
        }
        2 => {
            let second_dim = output_shape[1];
            for b in 0..batch {
                if second_dim == 1 {
                    scores.push(output_view[[b, 0]]);
                } else {
                    // Multi-class: take the positive-class logit (last column).
                    scores.push(output_view[[b, second_dim - 1]]);
                }
            }
        }
        _ => {
            return Err(EmbedError::Onnx(format!(
                "Unexpected output shape: {:?}",
                output_shape
            )));
        }
    }

    // If scores look like raw logits (outside [0,1]), apply sigmoid.
    let need_sigmoid = scores.iter().any(|&s| !(0.0..=1.0).contains(&s));
    if need_sigmoid {
        for s in &mut scores {
            *s = 1.0 / (1.0 + (-*s).exp());
        }
    }

    for s in &mut scores {
        *s = s.clamp(0.0, 1.0);
    }

    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_memory_budget_allows_small_file() {
        // Any existing small file will do; we just need to exercise the path.
        let path = std::env::temp_dir().join("syncmind_test_small.txt");
        std::fs::write(&path, b"tiny").unwrap();
        assert!(check_memory_budget(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_onnx_reranker_e2e() {
        let model_dir = syncmind_core::model_cache_dir().unwrap();
        let model_path = model_dir.join("bge-reranker-base").join("model.onnx");
        if !model_path.exists() {
            eprintln!("Skipping E2E reranker test: model not found at {:?}", model_path);
            return;
        }

        let reranker = OnnxReranker::new(&model_path).unwrap();

        let query = "What is machine learning?";
        let passages = &[
            "Machine learning is a subset of artificial intelligence.",
            "The capital of France is Paris.",
            "Machine learning algorithms learn patterns from data.",
            "Rust is a systems programming language.",
        ];

        let scores = reranker.rerank(query, passages).await.unwrap();
        assert_eq!(scores.len(), passages.len());

        // All scores must be in [0, 1].
        for score in &scores {
            assert!((0.0..=1.0).contains(score), "score {} out of range", score);
        }

        // The most relevant passages should outrank the irrelevant ones.
        // Passages 0 and 2 are about machine learning; 1 and 3 are not.
        assert!(
            scores[0] > scores[1],
            "ML passage should outrank France passage: {} vs {}",
            scores[0],
            scores[1]
        );
        assert!(
            scores[2] > scores[3],
            "ML passage should outrank Rust passage: {} vs {}",
            scores[2],
            scores[3]
        );

        eprintln!("Reranker scores: {:?}", scores);
    }
}
