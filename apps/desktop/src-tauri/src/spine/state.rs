//! Runtime orchestrator for the spine subsystem.
//!
//! `SpineRuntime` is stored in `AppState` and lives for the lifetime of the desktop
//! application. It owns the device identity, the JWT cache, and the (rebuildable)
//! `SpineClient`, and tracks the in-progress pairing handle.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::spine::client::{JwtHolder, SpineClient};
use crate::spine::identity::{self, Identity};
use crate::spine::{SpineError, SpineErrorCode};

/// In-progress pairing handle (None when no pairing is active).
pub struct ActivePairing {
    pub session_id: String,
    pub poller: tokio::task::JoinHandle<Result<crate::spine::pairing::PollOutcome, SpineError>>,
}

pub struct SpineRuntime {
    /// `<data-dir>/` from `syncmind_core::paths::local_data_dir`.
    pub data_dir: PathBuf,
    pub identity: Arc<Identity>,
    pub jwt: Arc<JwtHolder>,
    /// `None` until the user configures a Spine URL. Replaced when URL or trust-CA change.
    client: RwLock<Option<Arc<SpineClient>>>,
    /// In-progress pairing handle. `pub(crate)` so the command layer can drain a finished
    /// poller without going through an extra accessor.
    pub(crate) pairing: Mutex<Option<ActivePairing>>,
    ws_worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    on_new_bundle: Arc<dyn Fn() + Send + Sync>,
    status_sink: Arc<dyn Fn(crate::spine::ws::WsStatus) + Send + Sync>,
}

impl SpineRuntime {
    /// Construct from a freshly-resolved identity. The HTTP client is built lazily on the
    /// first call that needs it.
    pub fn new(
        data_dir: PathBuf,
        identity: Identity,
        on_new_bundle: Arc<dyn Fn() + Send + Sync>,
        status_sink: Arc<dyn Fn(crate::spine::ws::WsStatus) + Send + Sync>,
    ) -> Self {
        Self {
            data_dir,
            identity: Arc::new(identity),
            jwt: Arc::new(JwtHolder::new()),
            client: RwLock::new(None),
            pairing: Mutex::new(None),
            ws_worker: Mutex::new(None),
            on_new_bundle,
            status_sink,
        }
    }

    /// Rebuild the HTTP client from the current `SpineConfig`. Pass `None` for the URL to
    /// tear the client down (e.g. after the user clears the field).
    pub async fn rebuild_client(
        &self,
        url: Option<&str>,
        trust_ca_path: Option<&std::path::Path>,
    ) -> Result<(), SpineError> {
        match url {
            Some(u) if !u.is_empty() => {
                let client = SpineClient::new(
                    u,
                    trust_ca_path,
                    Arc::clone(&self.identity),
                    Arc::clone(&self.jwt),
                )?;
                *self.client.write().await = Some(Arc::new(client));
                info!("spine client rebuilt for url {u}");
            }
            _ => {
                *self.client.write().await = None;
                info!("spine client cleared (no URL configured)");
            }
        }
        // Drop any cached JWT — it's tied to the previous client's auth round-trip identity.
        self.jwt.clear().await;
        Ok(())
    }

    pub async fn refresh_live_sync(&self, is_paired: bool) -> Result<(), SpineError> {
        let next_client = if is_paired {
            self.client.read().await.clone()
        } else {
            None
        };

        let mut guard = self.ws_worker.lock().await;
        if let Some(prev) = guard.take() {
            prev.abort();
        }

        match next_client {
            Some(client) => {
                *guard = Some(crate::spine::ws::spawn_loop(
                    client,
                    Arc::clone(&self.identity),
                    Arc::clone(&self.jwt),
                    Arc::clone(&self.on_new_bundle),
                    Arc::clone(&self.status_sink),
                ));
            }
            None => {
                (self.status_sink)(crate::spine::ws::WsStatus::Disabled);
            }
        }

        Ok(())
    }

    /// Borrow the current client, returning `SPINE_NOT_CONFIGURED` if none has been built.
    pub async fn require_client(&self) -> Result<Arc<SpineClient>, SpineError> {
        match self.client.read().await.clone() {
            Some(c) => Ok(c),
            None => Err(SpineError::new(
                SpineErrorCode::SpineNotConfigured,
                "configure spine.url before using sync features",
            )),
        }
    }

    /// Set or clear the active pairing handle. Aborts any previous poller.
    pub async fn set_pairing(&self, next: Option<ActivePairing>) {
        let mut guard = self.pairing.lock().await;
        if let Some(prev) = guard.take() {
            prev.poller.abort();
        }
        *guard = next;
    }

    pub async fn current_pairing_session(&self) -> Option<String> {
        self.pairing
            .lock()
            .await
            .as_ref()
            .map(|p| p.session_id.clone())
    }

    /// Cancel any in-progress pairing (no-op if none).
    pub async fn cancel_pairing(&self) {
        if let Some(prev) = self.pairing.lock().await.take() {
            prev.poller.abort();
        }
    }

    /// Wipe local pairing state. Used by `spine_unpair`.
    pub async fn local_unpair(&self, peer_fingerprint: &str) {
        self.cancel_pairing().await;
        if let Some(prev) = self.ws_worker.lock().await.take() {
            prev.abort();
        }
        if let Err(e) = identity::wipe_sync_key(peer_fingerprint) {
            warn!(error = %e, peer = %peer_fingerprint, "failed to wipe sync_key");
        }
        self.jwt.clear().await;
        (self.status_sink)(crate::spine::ws::WsStatus::Disabled);
    }
}
