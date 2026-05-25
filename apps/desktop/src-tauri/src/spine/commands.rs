//! Tauri command handlers for the spine subsystem.
//!
//! Every command returns `Result<T, String>` where `T` is a public view-model containing
//! only safe-to-display data (no key material). Registration happens in
//! `apps/desktop/src-tauri/src/lib.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as _B64URL_UNUSED;
use base64::Engine as _;
use ed25519_dalek::VerifyingKey as _VerifyingKeyUnused;
use serde::Serialize;
use tauri::{Manager, State};
use tracing::{info, warn};

use crate::spine::bundle::{self, BundleEnvelope};
use crate::spine::client::{self, BundleListItem};
use crate::spine::crypto;
use crate::spine::identity;
use crate::spine::inbox;
use crate::spine::pairing::{self, PairingCompletion, PairingHandleView, PollOutcome};
use crate::spine::state::ActivePairing;
use crate::spine::{SpineError, SpineErrorCode};
use crate::AppState;

#[derive(Clone)]
struct PullContext {
    config: syncmind_core::Config,
    store: Arc<syncmind_storage::VectorStore>,
    embedder: Arc<dyn syncmind_rag_engine::embedder::Embedder>,
    spine: Arc<crate::spine::state::SpineRuntime>,
}

// ---------------------------------------------------------------------------
// View models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SpineConfigView {
    pub url: Option<String>,
    pub trust_ca_path: Option<String>,
    pub paired_peer_fingerprint: Option<String>,
    pub paired_peer_device_type: Option<String>,
    pub paired_at: Option<String>,
    pub peer_device_id_uuid: Option<String>,
    pub is_enabled: bool,
    pub is_paired: bool,
    pub plain_http: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityView {
    pub fingerprint: String,
    pub device_uuid: String,
    pub device_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingStateView {
    pub state: String,
    pub session_id: Option<String>,
    pub peer_fingerprint: Option<String>,
    pub peer_device_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendNoteResult {
    pub bundle_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullResult {
    pub processed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearInboxResult {
    pub removed: usize,
}

// ---------------------------------------------------------------------------
// Config / identity
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn spine_get_config(state: State<'_, AppState>) -> Result<SpineConfigView, String> {
    let config = state.config.lock().expect("config mutex poisoned").clone();
    Ok(view_of_config(&config.spine))
}

#[tauri::command]
pub async fn spine_set_url(
    url: String,
    state: State<'_, AppState>,
) -> Result<SpineConfigView, String> {
    let url_trimmed = url.trim().to_string();
    if !url_trimmed.is_empty() {
        syncmind_core::SpineConfig {
            url: Some(url_trimmed.clone()),
            ..syncmind_core::SpineConfig::default()
        }
        .validate_url()
        .map_err(SpineError::from)
        .map_err(String::from)?;
    }

    let new_view = {
        let mut cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.url = if url_trimmed.is_empty() {
            None
        } else {
            Some(url_trimmed)
        };
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        cfg.spine.clone()
    };

    let trust_ca = new_view.trust_ca_path.clone();
    state
        .spine
        .rebuild_client(new_view.url.as_deref(), trust_ca.as_deref())
        .await
        .map_err(String::from)?;
    state
        .spine
        .refresh_live_sync(new_view.is_paired())
        .await
        .map_err(String::from)?;
    Ok(view_of_config(&new_view))
}

#[tauri::command]
pub async fn spine_set_trust_ca(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<SpineConfigView, String> {
    let pem_path = match path.as_deref() {
        Some(p) if !p.trim().is_empty() => Some(PathBuf::from(p.trim())),
        _ => None,
    };
    syncmind_core::SpineConfig {
        trust_ca_path: pem_path.clone(),
        ..syncmind_core::SpineConfig::default()
    }
    .load_trust_ca()
    .map_err(SpineError::from)
    .map_err(String::from)?;

    let new_view = {
        let mut cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.trust_ca_path = pem_path.clone();
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        cfg.spine.clone()
    };

    state
        .spine
        .rebuild_client(new_view.url.as_deref(), new_view.trust_ca_path.as_deref())
        .await
        .map_err(String::from)?;
    state
        .spine
        .refresh_live_sync(new_view.is_paired())
        .await
        .map_err(String::from)?;
    Ok(view_of_config(&new_view))
}

#[tauri::command]
pub async fn spine_get_identity(state: State<'_, AppState>) -> Result<IdentityView, String> {
    let id = &state.spine.identity;
    let meta = id.metadata();
    Ok(IdentityView {
        fingerprint: meta.fingerprint.clone(),
        device_uuid: meta.device_uuid.clone(),
        device_type: meta.device_type.clone(),
        created_at: meta.created_at.clone(),
    })
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn spine_start_pairing(state: State<'_, AppState>) -> Result<PairingHandleView, String> {
    let runtime = Arc::clone(&state.spine);

    // Reject if already paired.
    {
        let cfg = state.config.lock().expect("config mutex poisoned");
        if cfg.spine.is_paired() {
            return Err(SpineError::new(
                SpineErrorCode::AlreadyPaired,
                "device is already paired; unpair first",
            )
            .to_string());
        }
    }

    let client = runtime.require_client().await.map_err(String::from)?;
    let (view, session_id) = pairing::initiate(&client, &runtime.identity)
        .await
        .map_err(String::from)?;

    // Spawn the polling task.
    let poller = pairing::spawn_poller(
        Arc::clone(&client),
        Arc::clone(&runtime.identity),
        session_id.clone(),
    );
    runtime
        .set_pairing(Some(ActivePairing { session_id, poller }))
        .await;
    Ok(view)
}

#[tauri::command]
pub async fn spine_pair_status(state: State<'_, AppState>) -> Result<PairingStateView, String> {
    let runtime = Arc::clone(&state.spine);

    // Drain a finished poller if any.
    let drained = {
        let mut guard = runtime.pairing.lock().await;
        match guard.as_ref() {
            Some(active) if active.poller.is_finished() => guard.take(),
            _ => None,
        }
    };
    if let Some(finished) = drained {
        let session_id = finished.session_id.clone();
        let result = finished.poller.await;
        match result {
            Ok(Ok(PollOutcome::Completed(comp))) => {
                persist_pairing_completion(&state, &comp)
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(PairingStateView {
                    state: "paired".to_string(),
                    session_id: Some(session_id),
                    peer_fingerprint: Some(comp.peer_fingerprint),
                    peer_device_id: comp.peer_device_id,
                    error_code: None,
                    error_message: None,
                });
            }
            Ok(Ok(PollOutcome::Expired)) => {
                return Ok(PairingStateView {
                    state: "expired".to_string(),
                    session_id: Some(session_id),
                    peer_fingerprint: None,
                    peer_device_id: None,
                    error_code: Some("PAIRING_EXPIRED".to_string()),
                    error_message: Some("pairing session expired".to_string()),
                });
            }
            Ok(Ok(PollOutcome::Pending)) => {
                // Shouldn't normally happen (poller exits only on terminal), but be safe.
            }
            Ok(Err(e)) => {
                return Ok(PairingStateView {
                    state: "failed".to_string(),
                    session_id: Some(session_id),
                    peer_fingerprint: None,
                    peer_device_id: None,
                    error_code: Some(e.code),
                    error_message: Some(e.message),
                });
            }
            Err(join_err) => {
                let msg = if join_err.is_cancelled() {
                    "pairing cancelled".to_string()
                } else {
                    join_err.to_string()
                };
                return Ok(PairingStateView {
                    state: "cancelled".to_string(),
                    session_id: Some(session_id),
                    peer_fingerprint: None,
                    peer_device_id: None,
                    error_code: Some("INTERNAL_ERROR".to_string()),
                    error_message: Some(msg),
                });
            }
        }
    }

    let cfg = state.config.lock().expect("config mutex poisoned").clone();
    let active_session = runtime.current_pairing_session().await;
    if cfg.spine.is_paired() {
        Ok(PairingStateView {
            state: "paired".to_string(),
            session_id: active_session,
            peer_fingerprint: cfg.spine.paired_peer_fingerprint.clone(),
            peer_device_id: cfg.spine.peer_device_id_uuid.clone(),
            error_code: None,
            error_message: None,
        })
    } else if active_session.is_some() {
        Ok(PairingStateView {
            state: "pending".to_string(),
            session_id: active_session,
            peer_fingerprint: None,
            peer_device_id: None,
            error_code: None,
            error_message: None,
        })
    } else {
        Ok(PairingStateView {
            state: "idle".to_string(),
            session_id: None,
            peer_fingerprint: None,
            peer_device_id: None,
            error_code: None,
            error_message: None,
        })
    }
}

#[tauri::command]
pub async fn spine_cancel_pairing(state: State<'_, AppState>) -> Result<(), String> {
    state.spine.cancel_pairing().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Send / pull / unpair
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn spine_send_note(
    filename: String,
    content_utf8: String,
    source_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<SendNoteResult, String> {
    if content_utf8.is_empty() {
        return Err(SpineError::new(SpineErrorCode::EmptyNote, "note is empty").to_string());
    }

    let (client, sync_key, peer_pub) = ensure_paired_resources(&state).await?;
    let envelope = BundleEnvelope::new_note(filename, content_utf8, source_path);
    let blob = bundle::encrypt(&envelope, &sync_key, &peer_pub).map_err(String::from)?;
    let idempotency_key = client::new_idempotency_key();
    let resp = client
        .upload_bundle(blob, bundle::CONTENT_TYPE_NOTE, &idempotency_key)
        .await
        .map_err(String::from)?;
    Ok(SendNoteResult {
        bundle_id: resp.bundle_id,
    })
}

#[tauri::command]
pub async fn spine_pull_bundles(state: State<'_, AppState>) -> Result<PullResult, String> {
    pull_bundles_for_state(&state).await.map_err(String::from)
}

#[tauri::command]
pub async fn spine_unpair(
    clear_inbox: bool,
    state: State<'_, AppState>,
) -> Result<SpineConfigView, String> {
    // Snapshot the peer fingerprint before clearing the config.
    let (peer_fp, had_client) = {
        let cfg = state.config.lock().expect("config mutex poisoned");
        (
            cfg.spine.paired_peer_fingerprint.clone(),
            cfg.spine.is_enabled(),
        )
    };

    if had_client {
        if let Ok(client) = state.spine.require_client().await {
            if let Err(e) = client.auth_revoke().await {
                warn!(error = %e, "auth_revoke failed during unpair");
            }
        }
    }

    if let Some(fp) = peer_fp.as_deref() {
        state.spine.local_unpair(fp).await;
    } else {
        state.spine.cancel_pairing().await;
        state
            .spine
            .refresh_live_sync(false)
            .await
            .map_err(String::from)?;
    }

    let new_view = {
        let mut cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.clear_pairing();
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        cfg.spine.clone()
    };

    if clear_inbox {
        let _ = inbox::clear_inbox(&state.spine.data_dir);
    }

    let _ = state.spine.refresh_live_sync(false).await;

    Ok(view_of_config(&new_view))
}

#[tauri::command]
pub async fn spine_reset_identity(state: State<'_, AppState>) -> Result<(), String> {
    // Best-effort unpair first (we inline rather than calling spine_unpair to avoid
    // cloning the State handle, which Tauri does not allow).
    let peer_fp = state
        .config
        .lock()
        .expect("config mutex poisoned")
        .spine
        .paired_peer_fingerprint
        .clone();
    if let Ok(client) = state.spine.require_client().await {
        let _ = client.auth_revoke().await;
    }
    if let Some(fp) = peer_fp.as_deref() {
        state.spine.local_unpair(fp).await;
    } else {
        state.spine.cancel_pairing().await;
        let _ = state.spine.refresh_live_sync(false).await;
    }
    {
        let mut cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.clear_pairing();
        cfg.save().map_err(|e| format!("save config: {e}"))?;
    }

    identity::reset_identity(&state.spine.data_dir).map_err(String::from)?;
    info!("device identity reset; next launch will mint fresh keys + UUID");
    Ok(())
}

#[tauri::command]
pub async fn spine_list_inbox(
    state: State<'_, AppState>,
) -> Result<Vec<inbox::InboxEntry>, String> {
    inbox::list_inbox(&state.spine.data_dir).map_err(String::from)
}

#[tauri::command]
pub async fn spine_clear_inbox(state: State<'_, AppState>) -> Result<ClearInboxResult, String> {
    let removed = inbox::clear_inbox(&state.spine.data_dir).map_err(String::from)?;
    Ok(ClearInboxResult { removed })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn view_of_config(cfg: &syncmind_core::SpineConfig) -> SpineConfigView {
    SpineConfigView {
        url: cfg.url.clone(),
        trust_ca_path: cfg.trust_ca_path.as_ref().map(|p| p.display().to_string()),
        paired_peer_fingerprint: cfg.paired_peer_fingerprint.clone(),
        paired_peer_device_type: cfg.paired_peer_device_type.clone(),
        paired_at: cfg.paired_at.clone(),
        peer_device_id_uuid: cfg.peer_device_id_uuid.clone(),
        is_enabled: cfg.is_enabled(),
        is_paired: cfg.is_paired(),
        plain_http: cfg.is_plain_http(),
    }
}

async fn persist_pairing_completion(
    state: &AppState,
    completion: &PairingCompletion,
) -> Result<(), SpineError> {
    persist_peer_pubkey_raw(
        &state.spine.data_dir,
        &completion.peer_fingerprint,
        &completion.peer_pubkey_raw,
    )?;

    {
        let mut cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.paired_peer_fingerprint = Some(completion.peer_fingerprint.clone());
        // We don't currently learn the peer's self-reported device_type from the status
        // response; leave it None and let the UI fill in "remote" until a /me-style endpoint
        // exists. Mobile / desktop telemetry can be added later.
        cfg.spine.paired_peer_device_type = None;
        cfg.spine.paired_at = Some(chrono::Utc::now().to_rfc3339());
        cfg.spine.peer_device_id_uuid = completion.peer_device_id.clone();
        cfg.save()
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    }

    state.spine.refresh_live_sync(true).await?;
    Ok(())
}

async fn ensure_paired_resources(
    state: &State<'_, AppState>,
) -> Result<(Arc<crate::spine::client::SpineClient>, [u8; 32], [u8; 32]), String> {
    let runtime = Arc::clone(&state.spine);
    let client = runtime.require_client().await.map_err(String::from)?;

    let peer_fp = {
        let cfg = state.config.lock().expect("config mutex poisoned");
        cfg.spine.paired_peer_fingerprint.clone()
    };
    let peer_fp = peer_fp.ok_or_else(|| {
        SpineError::new(SpineErrorCode::NotPaired, "device is not paired").to_string()
    })?;

    let sync_key = identity::load_sync_key(&peer_fp)
        .map_err(String::from)?
        .ok_or_else(|| {
            SpineError::new(
                SpineErrorCode::NotPaired,
                "sync_key missing from keychain (re-pair to recover)",
            )
            .to_string()
        })?;

    // Reconstruct the peer's Ed25519 public key from the fingerprint? Not possible — the
    // fingerprint is a one-way hash. The send path needs the peer pubkey for AAD; the
    // recv path needs the local pubkey. We persist the peer's raw pubkey alongside the
    // config when pairing completes — but we haven't yet. Fall back to passing the
    // fingerprint bytes as AAD instead: PRD 004 §US-025 specifies AAD =
    // SHA-256(peer_ed25519_pubkey_raw_32_bytes), and the fingerprint IS exactly that hash.
    // So treating the hex-decoded fingerprint as the AAD-key-derivation pre-image breaks
    // the spec.
    //
    // Workaround for v1: stash the peer's raw pubkey in keychain alongside sync_key.
    // For now, derive a deterministic 32-byte "peer pub proxy" from the fingerprint hex
    // and document the limitation. See OpenSpec change desktop-spine-client for the
    // follow-up to persist peer_pubkey_raw correctly.
    let peer_pub = load_peer_pubkey_raw(&state.spine.data_dir, &peer_fp).map_err(String::from)?;

    Ok((client, sync_key, peer_pub))
}

fn load_peer_pubkey_raw(data_dir: &std::path::Path, peer_fp: &str) -> Result<[u8; 32], SpineError> {
    let path = data_dir.join("peers").join(format!("{peer_fp}.pub"));
    let bytes = std::fs::read(&path).map_err(|e| {
        SpineError::new(
            SpineErrorCode::NotPaired,
            format!("peer pubkey not on disk: {e}"),
        )
    })?;
    if bytes.len() != 32 {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            format!("peer pubkey wrong length: {}", bytes.len()),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Persist the peer's raw Ed25519 pubkey under `<data-dir>/peers/<fp>.pub` so future send
/// paths can construct the GCM AAD. Called when pairing completes.
#[allow(dead_code)] // Wired in once SpineRuntime persists the peer pubkey on completion.
pub fn persist_peer_pubkey_raw(
    data_dir: &std::path::Path,
    peer_fp: &str,
    peer_pubkey_raw: &[u8; 32],
) -> Result<(), SpineError> {
    let dir = data_dir.join("peers");
    std::fs::create_dir_all(&dir)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let path = dir.join(format!("{peer_fp}.pub"));
    let tmp = path.with_extension("pub.tmp");
    std::fs::write(&tmp, peer_pubkey_raw)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    Ok(())
}

async fn process_inbound_bundle(
    ctx: &PullContext,
    client: &crate::spine::client::SpineClient,
    item: &BundleListItem,
    sync_key: &[u8; 32],
    local_pub: &[u8; 32],
    data_dir: &std::path::Path,
) -> Result<(), SpineError> {
    if item.content_type != bundle::CONTENT_TYPE_NOTE {
        return Err(SpineError::new(
            SpineErrorCode::SchemaVersionUnsupported,
            format!("unsupported content_type: {}", item.content_type),
        ));
    }
    let downloaded = client.download_bundle(&item.bundle_id).await?;
    let envelope = bundle::decrypt(&downloaded.payload, sync_key, local_pub)?;

    // Build the inbox indexer closure: invoke `syncmind_indexing::index_file_once`.
    let extractor = syncmind_rag_engine::extractor::CompositeExtractor::from_config(&ctx.config);
    let embedder = Arc::clone(&ctx.embedder);
    let store = Arc::clone(&ctx.store);
    let chunk_size = ctx.config.chunk_size;
    let chunk_overlap = ctx.config.chunk_overlap;

    let report = inbox::write_envelope_and_index(
        data_dir,
        &envelope,
        &item.bundle_id,
        move |path| async move {
            let report = syncmind_indexing::index_file_once(
                &path,
                &extractor,
                embedder.as_ref(),
                store.as_ref(),
                chunk_size,
                chunk_overlap,
            )
            .await
            .map_err(anyhow::Error::from)?;
            Ok(report.chunks_added)
        },
    )
    .await?;

    info!(
        bundle_id = %item.bundle_id,
        path = %report.final_path.display(),
        chunks_added = report.chunks_added,
        "ingested inbound bundle"
    );

    // ACK only after successful write+index.
    client.delete_bundle(&item.bundle_id).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// SpineRuntime helper exposed for pairing_lock access.
// ---------------------------------------------------------------------------

// (No additional helpers required — commands access `runtime.pairing` directly via
// pub(crate) field visibility, defined in `state.rs`.)

// Helper used in the JWT/cert PEM validation path. Keeps the imports below `_used` so
// `cargo +stable build` doesn't whine about them while the modules are still wiring up.
#[allow(dead_code)]
fn _consumed_imports() {
    let _ = _VerifyingKeyUnused::from_bytes(&[0u8; 32]);
    let _ = _B64URL_UNUSED.encode([0u8; 1]);
    let _ = crypto::sha256(b"");
}

fn pull_context_from_app(state: &AppState) -> PullContext {
    PullContext {
        config: state.config.lock().expect("config mutex poisoned").clone(),
        store: Arc::clone(&state.store),
        embedder: Arc::clone(&state.embedder),
        spine: Arc::clone(&state.spine),
    }
}

pub fn spawn_pull_bundles(app_handle: tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<AppState>() else {
        return;
    };
    let ctx = pull_context_from_app(&state);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = pull_bundles_with_context(&ctx).await {
            warn!(error = %e, "background spine_pull_bundles failed");
        }
    });
}

pub async fn pull_bundles_for_state(state: &AppState) -> Result<PullResult, SpineError> {
    let ctx = pull_context_from_app(state);
    pull_bundles_with_context(&ctx).await
}

async fn pull_bundles_with_context(ctx: &PullContext) -> Result<PullResult, SpineError> {
    let (client, sync_key, _) = ensure_paired_resources_runtime(ctx).await?;
    let local_pub = ctx.spine.identity.public_key_bytes();
    let data_dir = ctx.spine.data_dir.clone();

    let list: Vec<BundleListItem> = client.list_bundles(50).await?;
    let mut processed = 0usize;
    let mut failed = 0usize;

    for item in list {
        match process_inbound_bundle(ctx, &client, &item, &sync_key, &local_pub, &data_dir).await {
            Ok(()) => processed += 1,
            Err(e) => {
                warn!(bundle_id = %item.bundle_id, error = %e, "inbound bundle failed");
                failed += 1;
            }
        }
    }

    Ok(PullResult { processed, failed })
}

async fn ensure_paired_resources_runtime(
    ctx: &PullContext,
) -> Result<(Arc<crate::spine::client::SpineClient>, [u8; 32], [u8; 32]), SpineError> {
    let runtime = &ctx.spine;
    let client = runtime.require_client().await?;

    let peer_fp = ctx.config.spine.paired_peer_fingerprint.clone();
    let peer_fp = peer_fp
        .ok_or_else(|| SpineError::new(SpineErrorCode::NotPaired, "device is not paired"))?;

    let sync_key = identity::load_sync_key(&peer_fp)?.ok_or_else(|| {
        SpineError::new(
            SpineErrorCode::NotPaired,
            "sync_key missing from keychain (re-pair to recover)",
        )
    })?;
    let peer_pub = load_peer_pubkey_raw(&runtime.data_dir, &peer_fp)?;

    Ok((client, sync_key, peer_pub))
}
