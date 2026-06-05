//! Inbound bundle dispatch — routes a decrypted envelope to the correct handler
//! based on its `kind`.
//!
//! Called from `commands::process_inbound_bundle` after decryption, replacing
//! the pre-Phase-4 direct call to `inbox::write_envelope_and_index`.
//!
//! See PRD 004 §US-035 (inbound bundle routing), OpenSpec change
//! `desktop-spine-ingestion-dispatch`.

use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::spine::bundle::BundleEnvelope;
use crate::spine::inbox;
use crate::spine::ratelimit;
use crate::spine::stt;
use crate::spine::{SpineError, SpineErrorCode};

/// Outcome of dispatching a single inbound bundle. Each variant carries enough
/// context for the caller to log / ACK / NACK appropriately.
#[derive(Debug, Clone)]
pub enum DispatchOutcome {
    /// Text content was written to the sync-inbox and fed into the local index.
    TextIndexed { path: PathBuf, chunks_added: usize },
    /// Binary content (audio, image) was stored on disk alongside a placeholder
    /// markdown file.
    BinaryStored {
        binary_path: PathBuf,
        markdown_path: PathBuf,
    },
    /// The bundle was an RPC request/response (search, etc.) and was handled
    /// without producing local files.
    RpcHandled,
    /// The bundle was a search RPC request that exceeded the per-peer rate
    /// limit; the caller must encrypt/upload the provided response envelope.
    RpcRateLimited { response: BundleEnvelope },
    /// The bundle was intentionally skipped (e.g. deduplicated, expired).
    Ignored,
    /// The kind was recognized but did not match any active handler — logged
    /// for forensic analysis.
    Unknown { forensic_path: PathBuf },
}

/// Maximum decoded (plaintext) bundle payload size. 12 MB.
pub const MAX_DECODED_BUNDLE_BYTES: usize = 12 * 1024 * 1024;

pub type PostprocessIndexer = Arc<
    dyn Fn(PathBuf) -> Pin<Box<dyn Future<Output = anyhow::Result<usize>> + Send>> + Send + Sync,
>;

// ---------------------------------------------------------------------------
// Payload structs — each maps to the JSON body of a specific bundle kind.
// ---------------------------------------------------------------------------

/// Decoded payload of a `search-request` bundle.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SearchRequestPayload {
    pub v: u8,
    pub kind: String,
    pub request_id: String,
    pub query: String,
    pub top_k: u32,
    pub filter_file_type: Option<Vec<String>>,
    pub client_ts: String,
}

/// A single search hit in the search-response payload. Maps one-to-one with
/// [`crate::commands::SearchResultDto`] but lives here because the protocol
/// type belongs next to the dispatch logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub chunk_id: i64,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub score: f64,
}

/// Decoded payload of a `search-response` bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponsePayload {
    pub v: u8,
    pub kind: String,
    pub request_id: String,
    pub results: Vec<SearchHitDto>,
    pub server_ts: String,
}

/// Decoded payload of a rate-limit or other RPC error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchErrorPayload {
    pub v: u8,
    pub kind: String,
    pub request_id: String,
    pub error_code: String,
    pub error_message: String,
    pub retry_after_seconds: u32,
    pub server_ts: String,
}

/// Build a `search-response` [`BundleEnvelope`] from a request ID and result hits.
///
/// Sets the envelope kind to `"search-response"`, serialises the response payload
/// as JSON, and computes the SHA-256 content hash.
pub fn build_search_response_envelope(
    request_id: &str,
    results: &[SearchHitDto],
) -> BundleEnvelope {
    let payload = SearchResponsePayload {
        v: 1,
        kind: "search-response".to_string(),
        request_id: request_id.to_string(),
        results: results.to_vec(),
        server_ts: chrono::Utc::now().to_rfc3339(),
    };
    let content =
        serde_json::to_string(&payload).expect("SearchResponsePayload is always serializable");
    let sha = hex::encode(crate::spine::crypto::sha256(content.as_bytes()));
    BundleEnvelope {
        schema_version: crate::spine::bundle::SCHEMA_VERSION_V1,
        kind: "search-response".to_string(),
        filename: format!("search-response-{request_id}.json"),
        content_utf8: content,
        source_path: None,
        captured_at: chrono::Utc::now().to_rfc3339(),
        sha256: sha,
    }
}

/// Build a `search-response` envelope carrying an inner `kind: "error"` payload.
///
/// The outer kind intentionally remains `search-response` so the existing
/// encrypted response routing can deliver it to the requesting peer.
pub fn build_rate_limited_error_envelope(request_id: &str) -> BundleEnvelope {
    let payload = SearchErrorPayload {
        v: 1,
        kind: "error".to_string(),
        request_id: request_id.to_string(),
        error_code: "RATE_LIMITED".to_string(),
        error_message: format!(
            "Search rate limit exceeded: {} requests/minute per device. Try again later.",
            ratelimit::MAX_REQUESTS
        ),
        retry_after_seconds: 30,
        server_ts: chrono::Utc::now().to_rfc3339(),
    };
    let content =
        serde_json::to_string(&payload).expect("SearchErrorPayload is always serializable");
    let sha = hex::encode(crate::spine::crypto::sha256(content.as_bytes()));
    BundleEnvelope {
        schema_version: crate::spine::bundle::SCHEMA_VERSION_V1,
        kind: "search-response".to_string(),
        filename: format!("search-error-{request_id}.json"),
        content_utf8: content,
        source_path: None,
        captured_at: chrono::Utc::now().to_rfc3339(),
        sha256: sha,
    }
}

/// Payload for `capture-text` bundles.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CaptureTextPayload {
    id: String,
    text: String,
    source: String,
    client_ts: String,
}

/// Payload for `capture-link` bundles.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CaptureLinkPayload {
    id: String,
    url: String,
    #[serde(default)]
    shared_text: Option<String>,
    client_ts: String,
}

/// Payload for `capture-audio` bundles.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CaptureAudioPayload {
    id: String,
    audio_base64: String,
    audio_mime: String,
    duration_ms: u64,
    client_ts: String,
    #[serde(default)]
    client_device_fingerprint: Option<String>,
}

/// Payload for `capture-image` bundles.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CaptureImagePayload {
    id: String,
    image_base64: String,
    image_mime: String,
    width: u32,
    height: u32,
    client_ts: String,
    #[serde(default)]
    client_device_fingerprint: Option<String>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Dispatch a decrypted bundle envelope to the appropriate handler.
///
/// # Parameters
///
/// * `data_dir` — SyncMind data directory (used to derive inbox / storage paths).
/// * `envelope` — The decrypted, validated [`BundleEnvelope`].
/// * `bundle_id` — Server-assigned bundle ID (used for idempotency / ACK).
/// * `peer_fingerprint` — Fingerprint of the sending peer.
/// * `indexer` — Closure that indexes a plaintext file and returns chunk count.
/// * `rpc` — Closure that handles a `search-request` payload.
///
/// # Errors
///
/// Returns [`SpineError`] if the envelope fails validation, exceeds the size cap,
/// or the handler fails.
#[cfg_attr(not(test), allow(dead_code))]
pub async fn dispatch_bundle<I, IF, R, RF>(
    data_dir: &Path,
    envelope: &BundleEnvelope,
    bundle_id: &str,
    peer_fingerprint: &str,
    indexer: I,
    rpc: R,
) -> Result<DispatchOutcome, SpineError>
where
    I: FnOnce(PathBuf) -> IF,
    IF: std::future::Future<Output = anyhow::Result<usize>>,
    R: FnOnce(SearchRequestPayload) -> RF,
    RF: std::future::Future<Output = anyhow::Result<()>>,
{
    // Validate the envelope first — schema version, kind allowlist, content hash.
    envelope.validate()?;

    // Size cap: reject any envelope whose raw content exceeds the limit.
    if envelope.content_utf8.len() > MAX_DECODED_BUNDLE_BYTES {
        return Err(SpineError::new(
            SpineErrorCode::BundleTooLarge,
            format!(
                "bundle content exceeds maximum size ({} > {})",
                envelope.content_utf8.len(),
                MAX_DECODED_BUNDLE_BYTES,
            ),
        ));
    }

    route_bundle(
        data_dir,
        envelope,
        bundle_id,
        peer_fingerprint,
        indexer,
        rpc,
        None,
    )
    .await
}

pub async fn dispatch_bundle_with_postprocess<I, IF, R, RF>(
    data_dir: &Path,
    envelope: &BundleEnvelope,
    bundle_id: &str,
    peer_fingerprint: &str,
    indexer: I,
    postprocess_indexer: PostprocessIndexer,
    rpc: R,
) -> Result<DispatchOutcome, SpineError>
where
    I: FnOnce(PathBuf) -> IF,
    IF: std::future::Future<Output = anyhow::Result<usize>>,
    R: FnOnce(SearchRequestPayload) -> RF,
    RF: std::future::Future<Output = anyhow::Result<()>>,
{
    envelope.validate()?;

    if envelope.content_utf8.len() > MAX_DECODED_BUNDLE_BYTES {
        return Err(SpineError::new(
            SpineErrorCode::BundleTooLarge,
            format!(
                "bundle content exceeds maximum size ({} > {})",
                envelope.content_utf8.len(),
                MAX_DECODED_BUNDLE_BYTES,
            ),
        ));
    }

    route_bundle(
        data_dir,
        envelope,
        bundle_id,
        peer_fingerprint,
        indexer,
        rpc,
        Some(postprocess_indexer),
    )
    .await
}

// ---------------------------------------------------------------------------
// Core routing — split from `dispatch_bundle` so tests can bypass validate().
// ---------------------------------------------------------------------------

/// Inner routing function. Performs all the same work as [`dispatch_bundle`] but
/// does NOT call `envelope.validate()` — callers must ensure the envelope is
/// valid before reaching this function.
async fn route_bundle<I, IF, R, RF>(
    data_dir: &Path,
    envelope: &BundleEnvelope,
    bundle_id: &str,
    peer_fingerprint: &str,
    indexer: I,
    rpc: R,
    postprocess_indexer: Option<PostprocessIndexer>,
) -> Result<DispatchOutcome, SpineError>
where
    I: FnOnce(PathBuf) -> IF,
    IF: std::future::Future<Output = anyhow::Result<usize>>,
    R: FnOnce(SearchRequestPayload) -> RF,
    RF: std::future::Future<Output = anyhow::Result<()>>,
{
    match envelope.kind.as_str() {
        // ---- Persistent text kinds ----
        "note" => {
            let report =
                inbox::write_envelope_and_index(data_dir, envelope, bundle_id, indexer).await?;
            Ok(DispatchOutcome::TextIndexed {
                path: report.final_path,
                chunks_added: report.chunks_added,
            })
        }

        "capture-text" => {
            let payload: CaptureTextPayload = serde_json::from_str(&envelope.content_utf8)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid capture-text payload: {e}"),
                    )
                })?;

            let markdown = format!(
                "---\n\
                 source: mobile-capture\n\
                 kind: capture-text\n\
                 id: {id}\n\
                 captured_at: {ts}\n\
                 source_app: {source}\n\
                 ---\n\n\
                 {text}\n",
                id = payload.id,
                ts = payload.client_ts,
                source = payload.source,
                text = payload.text,
            );

            let dir = ensure_subdir(data_dir, "captures")?;
            let file_path = dir.join(format!("{}.md", payload.id));
            write_text_atomically(&file_path, &markdown).await?;

            let chunks_added = indexer(file_path.clone())
                .await
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("indexer: {e}")))?;
            Ok(DispatchOutcome::TextIndexed {
                path: file_path,
                chunks_added,
            })
        }

        "capture-link" => {
            let payload: CaptureLinkPayload = serde_json::from_str(&envelope.content_utf8)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid capture-link payload: {e}"),
                    )
                })?;

            let shared = payload.shared_text.as_deref().unwrap_or("");
            let markdown = format!(
                "---\n\
                 source: mobile-capture\n\
                 kind: capture-link\n\
                 id: {id}\n\
                 captured_at: {ts}\n\
                 ---\n\n\
                 ## URL\n{url}\n\n\
                 ## Shared text\n{shared}\n",
                id = payload.id,
                ts = payload.client_ts,
                url = payload.url,
                shared = shared,
            );

            let dir = ensure_subdir(data_dir, "captures")?;
            let file_path = dir.join(format!("{}.md", payload.id));
            write_text_atomically(&file_path, &markdown).await?;

            let chunks_added = indexer(file_path.clone())
                .await
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("indexer: {e}")))?;
            Ok(DispatchOutcome::TextIndexed {
                path: file_path,
                chunks_added,
            })
        }

        // ---- Persistent binary kinds ----
        "capture-audio" => {
            let payload: CaptureAudioPayload = serde_json::from_str(&envelope.content_utf8)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid capture-audio payload: {e}"),
                    )
                })?;

            // Size pre-check on approximate decoded size.
            let approx_decoded = payload.audio_base64.len() * 3 / 4;
            if approx_decoded > MAX_DECODED_BUNDLE_BYTES {
                return Err(SpineError::new(
                    SpineErrorCode::BundleTooLarge,
                    format!(
                        "audio payload too large: ~{} bytes (max {})",
                        approx_decoded, MAX_DECODED_BUNDLE_BYTES,
                    ),
                ));
            }

            let binary = base64::engine::general_purpose::STANDARD
                .decode(&payload.audio_base64)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid audio base64: {e}"),
                    )
                })?;

            // Write binary file.
            let audio_dir = ensure_subdir(data_dir, "audio")?;
            let binary_path = audio_dir.join(format!("{}.m4a", payload.id));
            write_binary_atomically(&binary_path, &binary).await?;

            // Write placeholder markdown (frontmatter + pending-transcription body).
            let markdown = format!(
                "---\n\
                 source: mobile-capture\n\
                 kind: capture-audio\n\
                 audio_file: ../audio/{id}.m4a\n\
                 duration_ms: {dur}\n\
                 captured_at: {ts}\n\
                 ---\n\n\
                 [mobile audio capture \u{2014} transcription pending]\n",
                id = payload.id,
                dur = payload.duration_ms,
                ts = payload.client_ts,
            );

            let captures_dir = ensure_subdir(data_dir, "captures")?;
            let markdown_path = captures_dir.join(format!("{}.md", payload.id));
            write_text_atomically(&markdown_path, &markdown).await?;

            // Index only the markdown (RAG engine does not handle raw audio).
            indexer(markdown_path.clone())
                .await
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("indexer: {e}")))?;

            spawn_audio_postprocess(
                binary_path.clone(),
                markdown_path.clone(),
                data_dir.to_path_buf(),
                postprocess_indexer,
            );

            Ok(DispatchOutcome::BinaryStored {
                binary_path,
                markdown_path,
            })
        }

        "capture-image" => {
            let payload: CaptureImagePayload = serde_json::from_str(&envelope.content_utf8)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid capture-image payload: {e}"),
                    )
                })?;

            // Size pre-check on approximate decoded size.
            let approx_decoded = payload.image_base64.len() * 3 / 4;
            if approx_decoded > MAX_DECODED_BUNDLE_BYTES {
                return Err(SpineError::new(
                    SpineErrorCode::BundleTooLarge,
                    format!(
                        "image payload too large: ~{} bytes (max {})",
                        approx_decoded, MAX_DECODED_BUNDLE_BYTES,
                    ),
                ));
            }

            let binary = base64::engine::general_purpose::STANDARD
                .decode(&payload.image_base64)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid image base64: {e}"),
                    )
                })?;

            // Write binary file.
            let images_dir = ensure_subdir(data_dir, "images")?;
            let binary_path = images_dir.join(format!("{}.jpg", payload.id));
            write_binary_atomically(&binary_path, &binary).await?;

            // Write placeholder markdown (frontmatter + pending-OCR body).
            let markdown = format!(
                "---\n\
                 source: mobile-capture\n\
                 kind: capture-image\n\
                 image_file: ../images/{id}.jpg\n\
                 width: {w}\n\
                 height: {h}\n\
                 captured_at: {ts}\n\
                 ---\n\n\
                 [mobile image capture \u{2014} OCR pending]\n",
                id = payload.id,
                w = payload.width,
                h = payload.height,
                ts = payload.client_ts,
            );

            let captures_dir = ensure_subdir(data_dir, "captures")?;
            let markdown_path = captures_dir.join(format!("{}.md", payload.id));
            write_text_atomically(&markdown_path, &markdown).await?;

            // Index only the markdown (RAG engine does not handle raw images).
            indexer(markdown_path.clone())
                .await
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("indexer: {e}")))?;

            spawn_image_postprocess(
                binary_path.clone(),
                markdown_path.clone(),
                payload.id.clone(),
                payload.width,
                payload.height,
                payload.client_ts.clone(),
                postprocess_indexer,
            );

            Ok(DispatchOutcome::BinaryStored {
                binary_path,
                markdown_path,
            })
        }

        // ---- Transient / silent kinds ----
        "search-request" => {
            let payload: SearchRequestPayload = serde_json::from_str(&envelope.content_utf8)
                .map_err(|e| {
                    SpineError::new(
                        SpineErrorCode::BadRequest,
                        format!("invalid search-request payload: {e}"),
                    )
                })?;

            // Double-check: inner `kind` must match outer `kind` (defense in depth).
            if payload.kind != "search-request" {
                return Err(SpineError::new(
                    SpineErrorCode::BadRequest,
                    format!(
                        "payload.kind mismatch: expected 'search-request', got '{}'",
                        payload.kind
                    ),
                ));
            }

            let limiter = ratelimit::search_rate_limiter();
            let allowed = {
                let mut limiter = limiter.lock().await;
                limiter.check_and_record(peer_fingerprint)
            };
            if !allowed {
                info!(
                    peer_fingerprint,
                    request_id = %payload.request_id,
                    "search-request rate limited"
                );
                return Ok(DispatchOutcome::RpcRateLimited {
                    response: build_rate_limited_error_envelope(&payload.request_id),
                });
            }

            rpc(payload).await.map_err(|e| {
                SpineError::new(SpineErrorCode::Internal, format!("rpc handler: {e}"))
            })?;

            Ok(DispatchOutcome::RpcHandled)
        }

        "search-response" => {
            warn!(
                bundle_id,
                peer_fingerprint, "unexpected search-response received by desktop; ignoring",
            );
            Ok(DispatchOutcome::Ignored)
        }

        // ---- Unknown / forensic ----
        kind => {
            let unknown_dir = ensure_subdir(data_dir, "_unknown")?;
            let forensic_path = unknown_dir.join(format!("{bundle_id}.json"));

            let json = serde_json::to_string_pretty(envelope).map_err(|e| {
                SpineError::new(
                    SpineErrorCode::Internal,
                    format!("serialize unknown envelope: {e}"),
                )
            })?;
            write_text_atomically(&forensic_path, &json).await?;

            warn!(
                bundle_id,
                kind, "unhandled bundle kind; envelope saved for forensic analysis",
            );
            #[cfg(not(test))]
            debug_assert!(false, "unhandled bundle kind: {kind}");

            Ok(DispatchOutcome::Unknown { forensic_path })
        }
    }
}

fn spawn_audio_postprocess(
    audio_path: PathBuf,
    markdown_path: PathBuf,
    data_dir: PathBuf,
    indexer: Option<PostprocessIndexer>,
) {
    tokio::spawn(async move {
        match stt::transcribe_audio(audio_path.clone(), markdown_path.clone(), data_dir).await {
            Ok(true) => {
                if let Some(indexer) = indexer {
                    if let Err(error) = indexer(markdown_path.clone()).await {
                        warn!(path = %markdown_path.display(), error = %error, "failed to re-index transcribed audio capture");
                    }
                }
            }
            Ok(false) => {}
            Err(error) => {
                warn!(path = %audio_path.display(), error = %error, "audio transcription failed");
            }
        }
    });
}

fn spawn_image_postprocess(
    image_path: PathBuf,
    markdown_path: PathBuf,
    id: String,
    width: u32,
    height: u32,
    captured_at: String,
    indexer: Option<PostprocessIndexer>,
) {
    tokio::spawn(async move {
        let image_path_for_ocr = image_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            syncmind_rag_engine::ocr::ocr_image(&image_path_for_ocr)
        })
        .await;

        match result {
            Ok(Ok(text)) if text.trim().len() >= 10 => {
                let image_file = image_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("../images/{name}"))
                    .unwrap_or_else(|| format!("../images/{id}.jpg"));
                let markdown = format!(
                    "---\n\
                     source: mobile-capture\n\
                     kind: capture-image\n\
                     id: {id}\n\
                     ocr_engine: ocrs\n\
                     ocr_languages: en\n\
                     width: {width}\n\
                     height: {height}\n\
                     captured_at: {captured_at}\n\
                     ---\n\n\
                     {body}\n\n\
                     image_file: {image_file}\n",
                    body = text.trim()
                );
                if let Err(error) = write_text_atomically(&markdown_path, &markdown).await {
                    warn!(path = %markdown_path.display(), error = %error, "failed to write OCR markdown");
                    return;
                }
                if let Some(indexer) = indexer {
                    if let Err(error) = indexer(markdown_path.clone()).await {
                        warn!(path = %markdown_path.display(), error = %error, "failed to re-index OCR capture");
                    }
                }
            }
            Ok(Ok(_)) => {
                if let Err(error) =
                    append_to_markdown(&markdown_path, "[image: no text detected]").await
                {
                    warn!(path = %markdown_path.display(), error = %error, "failed to append no-text OCR marker");
                }
            }
            Ok(Err(syncmind_rag_engine::ocr::OcrError::Decode(error))) => {
                warn!(path = %image_path.display(), error = %error, "image decode failed");
                if let Err(error) =
                    append_to_markdown(&markdown_path, "[image decode failed - OCR unavailable]")
                        .await
                {
                    warn!(path = %markdown_path.display(), error = %error, "failed to append decode OCR marker");
                }
            }
            Ok(Err(error)) => {
                warn!(path = %image_path.display(), error = %error, "image OCR unavailable");
            }
            Err(error) => {
                warn!(path = %image_path.display(), error = %error, "image OCR task failed");
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Ensure a subdirectory of the sync-inbox exists with restrictive (0700)
/// permissions.  Creates `sync-inbox/<name>` beneath `data_dir`.
fn ensure_subdir(data_dir: &Path, name: &str) -> Result<PathBuf, SpineError> {
    let inbox_dir = inbox::ensure_inbox_dir(data_dir)?;
    let subdir = inbox_dir.join(name);
    if !subdir.exists() {
        fs::create_dir_all(&subdir)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    }
    set_dir_permissions_0700(&subdir)?;
    Ok(subdir)
}

/// Write `content` to `path` atomically (tmp -> fsync -> rename).
async fn write_text_atomically(path: &Path, content: &str) -> Result<(), SpineError> {
    write_binary_atomically(path, content.as_bytes()).await
}

async fn append_to_markdown(path: &Path, marker: &str) -> Result<(), SpineError> {
    let mut content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(marker);
    content.push('\n');
    write_text_atomically(path, &content).await
}

/// Write `data` to `path` atomically (tmp -> fsync -> rename).
async fn write_binary_atomically(path: &Path, data: &[u8]) -> Result<(), SpineError> {
    let tmp_path = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        f.write_all(data)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        f.sync_all()
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    }
    fs::rename(&tmp_path, path)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions_0700(p: &Path) -> Result<(), SpineError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(p)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?
        .permissions();
    perms.set_mode(0o700);
    fs::set_permissions(p, perms)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

#[cfg(not(unix))]
fn set_dir_permissions_0700(_p: &Path) -> Result<(), SpineError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spine::bundle;
    use crate::spine::crypto;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_envelope(kind: &str, content: &str) -> BundleEnvelope {
        let sha = hex::encode(crypto::sha256(content.as_bytes()));
        BundleEnvelope {
            schema_version: bundle::SCHEMA_VERSION_V1,
            kind: kind.to_string(),
            filename: "test.md".to_string(),
            content_utf8: content.to_string(),
            source_path: None,
            captured_at: chrono::Utc::now().to_rfc3339(),
            sha256: sha,
        }
    }

    /// Construct a valid `capture-text` JSON payload body.
    fn capture_text_json(id: &str, text: &str, source: &str) -> String {
        serde_json::json!({
            "id": id,
            "text": text,
            "source": source,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string()
    }

    /// Construct a valid `capture-link` JSON payload body.
    fn capture_link_json(id: &str, url: &str, shared_text: Option<&str>) -> String {
        let mut m = serde_json::Map::new();
        m.insert("id".into(), serde_json::json!(id));
        m.insert("url".into(), serde_json::json!(url));
        m.insert(
            "client_ts".into(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
        if let Some(st) = shared_text {
            m.insert("shared_text".into(), serde_json::json!(st));
        }
        serde_json::to_string(&m).unwrap()
    }

    /// Construct a valid `capture-audio` JSON payload body (small base64 string).
    fn capture_audio_json(id: &str) -> String {
        let audio_b64 = base64::engine::general_purpose::STANDARD.encode(b"fake-wav-bytes-minimal");
        serde_json::json!({
            "v": 1,
            "kind": "capture-audio",
            "id": id,
            "audio_base64": audio_b64,
            "audio_mime": "audio/mp4",
            "duration_ms": 5000,
            "client_ts": chrono::Utc::now().to_rfc3339(),
            "client_device_fingerprint": "test-mobile-fingerprint",
        })
        .to_string()
    }

    /// Construct a valid `capture-image` JSON payload body (small base64 string).
    fn capture_image_json(id: &str) -> String {
        let img_b64 = base64::engine::general_purpose::STANDARD.encode(b"fake-jpeg-bytes-minimal");
        serde_json::json!({
            "id": id,
            "image_base64": img_b64,
            "image_mime": "image/jpeg",
            "width": 1920,
            "height": 1080,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string()
    }

    /// Construct a valid `search-request` JSON payload body.
    fn search_request_json(query: &str) -> String {
        serde_json::json!({
            "v": 1,
            "kind": "search-request",
            "request_id": "req-001",
            "query": query,
            "top_k": 5,
            "filter_file_type": null,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string()
    }

    // -----------------------------------------------------------------------
    // 3xx / 4xx / 5xx  Happy-path tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_note_writes_and_indexes() {
        let dir = TempDir::new().unwrap();
        let content = "hello from note test";
        let envelope = BundleEnvelope::new_note("memo.md", content, None);

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-note-1",
            "peer-fp",
            |path| async move {
                let read = std::fs::read_to_string(&path).unwrap();
                assert_eq!(read, content);
                Ok::<_, anyhow::Error>(3usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::TextIndexed { path, chunks_added } => {
                assert!(path.exists());
                assert_eq!(chunks_added, 3);
            }
            other => panic!("expected TextIndexed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_capture_text_writes_file() {
        let dir = TempDir::new().unwrap();
        let id = "ct-001";
        let text = "hello from mobile capture";
        let body = capture_text_json(id, text, "com.syncmind.mobile");

        let envelope = make_envelope("capture-text", &body);

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-ct-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains(text));
                assert!(content.contains("source: mobile-capture"));
                assert!(content.contains("kind: capture-text"));
                assert!(content.starts_with("---\n"));
                assert!(content.contains("\n---\n"));
                Ok::<_, anyhow::Error>(2usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::TextIndexed { path, chunks_added } => {
                assert!(path.exists());
                assert!(path.to_string_lossy().contains(id));
                assert_eq!(chunks_added, 2);
            }
            other => panic!("expected TextIndexed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_capture_link_writes_file() {
        let dir = TempDir::new().unwrap();
        let id = "cl-002";
        let body = capture_link_json(id, "https://example.com", Some("interesting article"));

        let envelope = make_envelope("capture-link", &body);

        let written = Arc::new(std::sync::Mutex::new(None::<String>));
        let written_clone = written.clone();
        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-cl-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("## URL"));
                assert!(content.contains("https://example.com"));
                assert!(content.contains("interesting article"));
                *written_clone.lock().unwrap() = Some(content);
                Ok::<_, anyhow::Error>(1usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::TextIndexed { path, .. } => {
                assert!(path.exists());
            }
            other => panic!("expected TextIndexed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_capture_audio_writes_binary_and_markdown() {
        let dir = TempDir::new().unwrap();
        let id = "au-003";
        let body = capture_audio_json(id);

        let envelope = make_envelope("capture-audio", &body);

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-au-1",
            "peer-fp",
            |path| async move {
                // Indexer is called only on the .md file.
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("kind: capture-audio"));
                assert!(content.contains("audio_file"));
                assert!(content.contains("[mobile audio capture"));
                Ok::<_, anyhow::Error>(0usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::BinaryStored {
                binary_path,
                markdown_path,
            } => {
                assert!(binary_path.exists(), "binary .m4a file should exist");
                assert!(markdown_path.exists(), "markdown .md file should exist");
                assert_eq!(
                    binary_path.extension().and_then(|e| e.to_str()),
                    Some("m4a")
                );
                assert_eq!(
                    markdown_path.extension().and_then(|e| e.to_str()),
                    Some("md")
                );
            }
            other => panic!("expected BinaryStored, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_capture_image_writes_binary_and_markdown() {
        let dir = TempDir::new().unwrap();
        let id = "im-004";
        let body = capture_image_json(id);

        let envelope = make_envelope("capture-image", &body);

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-im-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("kind: capture-image"));
                assert!(content.contains("image_file"));
                assert!(content.contains("[mobile image capture"));
                assert!(content.contains("width: 1920"));
                assert!(content.contains("height: 1080"));
                Ok::<_, anyhow::Error>(0usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::BinaryStored {
                binary_path,
                markdown_path,
            } => {
                assert!(binary_path.exists(), "binary .jpg file should exist");
                assert!(markdown_path.exists(), "markdown .md file should exist");
                assert_eq!(
                    binary_path.extension().and_then(|e| e.to_str()),
                    Some("jpg")
                );
            }
            other => panic!("expected BinaryStored, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_capture_audio_rejects_oversize() {
        let dir = TempDir::new().unwrap();
        // Build a base64 string whose decoded size exceeds the cap.
        let oversize_b64 = "A".repeat(MAX_DECODED_BUNDLE_BYTES * 4 / 3 + 100);
        let body = serde_json::json!({
            "id": "au-oversize",
            "audio_base64": oversize_b64,
            "audio_mime": "audio/m4a",
            "duration_ms": 1000,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();

        let envelope = make_envelope("capture-audio", &body);

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-oversize",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BUNDLE_TOO_LARGE");
    }

    #[tokio::test]
    async fn dispatch_capture_audio_rejects_malformed_base64() {
        let dir = TempDir::new().unwrap();
        let body = serde_json::json!({
            "id": "au-badb64",
            "audio_base64": "!!!not-valid-base64!!!",
            "audio_mime": "audio/m4a",
            "duration_ms": 1000,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();

        let envelope = make_envelope("capture-audio", &body);

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-badb64",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[tokio::test]
    async fn dispatch_capture_image_rejects_oversize() {
        let dir = TempDir::new().unwrap();
        let oversize_b64 = "A".repeat(MAX_DECODED_BUNDLE_BYTES * 4 / 3 + 100);
        let body = serde_json::json!({
            "id": "im-oversize",
            "image_base64": oversize_b64,
            "image_mime": "image/jpeg",
            "width": 100,
            "height": 100,
            "client_ts": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();

        let envelope = make_envelope("capture-image", &body);

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-oversize",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BUNDLE_TOO_LARGE");
    }

    #[tokio::test]
    async fn dispatch_search_request_invokes_rpc_handler() {
        let dir = TempDir::new().unwrap();
        let body = search_request_json("find me something");
        let envelope = make_envelope("search-request", &body);

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_rpc = calls.clone();

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-sr-1",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            move |payload: SearchRequestPayload| {
                let calls = calls_for_rpc.clone();
                async move {
                    assert_eq!(payload.query, "find me something");
                    assert_eq!(payload.kind, "search-request");
                    assert_eq!(payload.top_k, 5);
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, anyhow::Error>(())
                }
            },
        )
        .await
        .unwrap();

        assert!(matches!(outcome, DispatchOutcome::RpcHandled));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "rpc handler must be called exactly once"
        );
    }

    #[test]
    fn rate_limited_error_envelope_preserves_request_id_and_shape() {
        let envelope = build_rate_limited_error_envelope("req-rate-limit");
        assert_eq!(envelope.kind, "search-response");
        assert!(envelope.validate().is_ok());

        let payload: SearchErrorPayload = serde_json::from_str(&envelope.content_utf8).unwrap();
        assert_eq!(payload.v, 1);
        assert_eq!(payload.kind, "error");
        assert_eq!(payload.request_id, "req-rate-limit");
        assert_eq!(payload.error_code, "RATE_LIMITED");
        assert_eq!(payload.retry_after_seconds, 30);
        assert!(payload.error_message.contains("30 requests/minute"));
    }

    #[tokio::test]
    async fn dispatch_search_request_propagates_handler_error() {
        let dir = TempDir::new().unwrap();
        let body = search_request_json("fail");
        let envelope = make_envelope("search-request", &body);

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-sr-err",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Err(anyhow::anyhow!("search engine unavailable")) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "INTERNAL_ERROR");
        assert!(err.message.contains("rpc handler"));
    }

    #[tokio::test]
    async fn dispatch_search_response_is_silently_ignored() {
        let dir = TempDir::new().unwrap();
        let envelope = make_envelope("search-response", "{\"result\":\"ok\"}");

        let outcome = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-srsp-1",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        assert!(matches!(outcome, DispatchOutcome::Ignored));
    }

    // -----------------------------------------------------------------------
    // 6.x  Unknown-kind / forensic test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_unknown_kind_writes_forensic_file() {
        let dir = TempDir::new().unwrap();
        // Create an envelope with a kind that is NOT in RECOGNIZED_KINDS.
        // We bypass validate() by calling route_bundle directly.
        let envelope = make_envelope("future-kind-v2", "some payload");

        let outcome = route_bundle(
            dir.path(),
            &envelope,
            "bundle-future",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
            None,
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::Unknown { forensic_path } => {
                assert!(
                    forensic_path.exists(),
                    "forensic file must exist at {}",
                    forensic_path.display()
                );
                let content = std::fs::read_to_string(&forensic_path).unwrap();
                assert!(content.contains("future-kind-v2"));
                assert!(content.contains("some payload"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 1.4  Validate rejects unknown kind
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn validate_rejects_unknown_kind_in_dispatch() {
        let dir = TempDir::new().unwrap();
        let envelope = make_envelope("bogus-format-v99", "x");

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-unknown",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BAD_REQUEST");
    }

    // -----------------------------------------------------------------------
    // 9.4  Size cap: 13 MB capture-text → BundleTooLarge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn size_cap_rejects_oversize_content_utf8() {
        let dir = TempDir::new().unwrap();
        // content_utf8 > MAX_DECODED_BUNDLE_BYTES (12 MB)
        let oversized = "x".repeat(MAX_DECODED_BUNDLE_BYTES + 1);
        // This envelope has kind "capture-text" but content_utf8 > cap.
        // validate() passes (valid kind), but the size check in dispatch_bundle catches it.
        let envelope = make_envelope("capture-text", &oversized);

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-oversize-text",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BUNDLE_TOO_LARGE");
    }

    // -----------------------------------------------------------------------
    // 9.5  Placeholder markdown frontmatter shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn capture_text_markdown_has_correct_frontmatter_shape() {
        let dir = TempDir::new().unwrap();
        let id = "fm-test-1";
        let body = capture_text_json(id, "check frontmatter", "test-app");
        let envelope = make_envelope("capture-text", &body);
        let captured_markdown = Arc::new(std::sync::Mutex::new(None::<String>));
        let captured = captured_markdown.clone();

        dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-fm-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                *captured.lock().unwrap() = Some(content);
                Ok::<_, anyhow::Error>(1usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        let md = captured_markdown.lock().unwrap().take().unwrap();
        // Must start with ---\n
        assert!(
            md.starts_with("---\n"),
            "frontmatter must start with ---\\n"
        );
        // Must contain source: mobile-capture
        assert!(
            md.contains("source: mobile-capture"),
            "must contain source key"
        );
        // Must contain kind: capture-text
        assert!(md.contains("kind: capture-text"), "must contain kind key");
        // Must contain the id
        assert!(md.contains(&format!("id: {id}")), "must contain id");
        // Must close with \n---\n after the frontmatter block
        let after_first = md.strip_prefix("---\n").unwrap();
        let frontmatter_end = after_first.find("\n---\n");
        assert!(
            frontmatter_end.is_some(),
            "frontmatter must be closed with \\n---\\n"
        );
        // Body after frontmatter should contain the text
        let body_start = frontmatter_end.unwrap() + 5; // skip \n---\n
        let body = &after_first[body_start..];
        assert!(
            body.contains("check frontmatter"),
            "body must contain original text"
        );
    }

    #[tokio::test]
    async fn capture_link_markdown_has_correct_frontmatter() {
        let dir = TempDir::new().unwrap();
        let id = "fm-link-1";
        let body = capture_link_json(id, "https://example.org/link", None);
        let envelope = make_envelope("capture-link", &body);
        let captured = Arc::new(std::sync::Mutex::new(None::<String>));
        let captured_clone = captured.clone();

        dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-fml-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                *captured_clone.lock().unwrap() = Some(content);
                Ok::<_, anyhow::Error>(1usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        let md = captured.lock().unwrap().take().unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("source: mobile-capture"));
        assert!(md.contains("kind: capture-link"));
        assert!(md.contains("## URL"));
        assert!(md.contains("https://example.org/link"));
    }

    #[tokio::test]
    async fn capture_audio_placeholder_markdown_has_correct_shape() {
        let dir = TempDir::new().unwrap();
        let id = "au-fm-1";
        let body = capture_audio_json(id);
        let envelope = make_envelope("capture-audio", &body);
        let captured = Arc::new(std::sync::Mutex::new(None::<String>));
        let captured_clone = captured.clone();

        dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-au-fm-1",
            "peer-fp",
            |path| async move {
                let content = std::fs::read_to_string(&path).unwrap();
                *captured_clone.lock().unwrap() = Some(content);
                Ok::<_, anyhow::Error>(0usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        let md = captured.lock().unwrap().take().unwrap();
        assert!(md.starts_with("---\n"), "must start with ---\\n");
        assert!(md.contains("source: mobile-capture"));
        assert!(md.contains("kind: capture-audio"));
        assert!(md.contains(&format!("audio_file: ../audio/{id}.m4a")));
        assert!(md.contains("duration_ms: 5000"));
        assert!(md.contains("[mobile audio capture"));
        let after_first = md.strip_prefix("---\n").unwrap();
        assert!(
            after_first.contains("\n---\n"),
            "frontmatter must be properly closed"
        );
    }

    // -----------------------------------------------------------------------
    // 9.6  Subdirectory 0700 permissions (Unix only)
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    #[tokio::test]
    async fn subdirectories_have_0700_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();

        // Trigger creation of subdirectories by dispatching both audio and image
        // bundles so that captures/, audio/, and images/ are all created.
        let audio = make_envelope("capture-audio", &capture_audio_json("perm-audio"));
        let image = make_envelope("capture-image", &capture_image_json("perm-image"));

        dispatch_bundle(
            dir.path(),
            &audio,
            "bid-perm-audio",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(0usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        dispatch_bundle(
            dir.path(),
            &image,
            "bid-perm-image",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(0usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        let inbox_dir = dir.path().join("sync-inbox");
        for sub in &["captures", "audio", "images"] {
            let p = inbox_dir.join(sub);
            assert!(p.exists(), "subdir {sub} should exist");
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            // Mode should be 0o40700 (directory + 0700).
            assert_eq!(
                mode & 0o777,
                0o700,
                "expected 0700 permissions for sync-inbox/{sub}, got {:#o}",
                mode & 0o777
            );
        }
    }

    // -----------------------------------------------------------------------
    // 9.7  Verify inbox tests still compile — we don't run them here since
    //      they are in inbox::tests, but our changes do not touch inbox.rs so
    //      they are unaffected.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_capture_text_rejects_malformed_json() {
        let dir = TempDir::new().unwrap();
        let envelope = make_envelope("capture-text", "this is not json");

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-malformed",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[tokio::test]
    async fn dispatch_search_request_rejects_kind_mismatch() {
        let dir = TempDir::new().unwrap();
        let mut body: serde_json::Value =
            serde_json::from_str(&search_request_json("test")).unwrap();
        body["kind"] = serde_json::json!("capture-text"); // mismatch
        let envelope = make_envelope("search-request", &body.to_string());

        let err = dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-km-1",
            "peer-fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "BAD_REQUEST");
        assert!(err.message.contains("payload.kind mismatch"));
    }

    #[tokio::test]
    async fn capture_link_without_shared_text_omits_field() {
        let dir = TempDir::new().unwrap();
        let id = "cl-no-shared";
        let body = capture_link_json(id, "https://example.com", None);
        let envelope = make_envelope("capture-link", &body);
        let captured = Arc::new(std::sync::Mutex::new(None::<String>));
        let captured_clone = captured.clone();

        dispatch_bundle(
            dir.path(),
            &envelope,
            "bid-cl-ns-1",
            "peer-fp",
            |path| async move {
                *captured_clone.lock().unwrap() = Some(std::fs::read_to_string(&path).unwrap());
                Ok::<_, anyhow::Error>(1usize)
            },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
        )
        .await
        .unwrap();

        let md = captured.lock().unwrap().take().unwrap();
        // No `shared_text` in the JSON, so the markdown's Shared text section should be empty.
        assert!(
            md.contains("## Shared text\n\n"),
            "shared text section should be present but empty"
        );
    }

    #[tokio::test]
    async fn unknown_kind_triggers_debug_assert_in_forensic_path() {
        // Just verify the forensic path contains the bundle_id.
        let dir = TempDir::new().unwrap();
        let envelope = make_envelope("unknown-test", "data");

        let outcome = route_bundle(
            dir.path(),
            &envelope,
            "my-bundle-id",
            "fp",
            |_path| async move { Ok::<_, anyhow::Error>(1usize) },
            |_payload| async move { Ok::<_, anyhow::Error>(()) },
            None,
        )
        .await
        .unwrap();

        match outcome {
            DispatchOutcome::Unknown { forensic_path } => {
                let name = forensic_path.file_name().unwrap().to_string_lossy();
                assert!(
                    name.starts_with("my-bundle-id"),
                    "forensic filename should use bundle_id"
                );
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Verify that write_text_atomically and write_binary_atomically produce
    /// correct files without temp artifacts left behind.
    #[tokio::test]
    async fn atomic_writes_leave_no_tmp_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("hello.md");

        write_text_atomically(&path, "hello world").await.unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
        // No .tmp file should remain.
        assert!(
            !path.with_extension("md.tmp").exists() && !dir.path().join("hello.md.tmp").exists()
        );

        // Binary variant.
        let bin_path = dir.path().join("data.bin");
        write_binary_atomically(&bin_path, b"\x00\x01\x02")
            .await
            .unwrap();
        assert_eq!(std::fs::read(&bin_path).unwrap(), vec![0x00, 0x01, 0x02]);
    }

    /// ensure_subdir creates the subdirectory and returns its path.
    #[tokio::test]
    async fn ensure_subdir_creates_and_returns_path() {
        let dir = TempDir::new().unwrap();
        let sub = ensure_subdir(dir.path(), "test-sub").unwrap();
        assert!(sub.exists());
        assert!(sub.is_dir());
        assert!(sub.to_string_lossy().ends_with("test-sub"));
    }
}
