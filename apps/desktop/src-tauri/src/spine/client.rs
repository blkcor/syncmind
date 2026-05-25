//! HTTP client for the Spine sync gateway.
//!
//! Builds a `reqwest::Client` with rustls and (optionally) a user-supplied PEM trust anchor
//! per PRD 004 §US-020 / §Decisions §9. Wraps every endpoint with Bearer JWT injection,
//! per-bundle `Idempotency-Key`, and exponential-backoff retry on 429/5xx (PRD 004 §US-024,
//! §US-025).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Certificate, Client, ClientBuilder, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use url::Url;

use crate::spine::crypto::{self, MintedJwt};
use crate::spine::identity::Identity;
use crate::spine::{SpineError, SpineErrorCode};

const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";
const X_CONTENT_TYPE_HEADER: &str = "X-Syncmind-Content-Type";
const X_PAYLOAD_HASH_HEADER: &str = "X-Syncmind-Payload-Hash";
const MAX_RETRIES: usize = 5;
const RETRY_BASE_MS: u64 = 1000;

/// Holds the current minted JWT and serializes refreshes across concurrent callers.
pub struct JwtHolder {
    inner: RwLock<Option<MintedJwt>>,
}

impl JwtHolder {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Return the current token if one is held and not in the refresh window. Otherwise
    /// mint a fresh one using the supplied identity.
    pub async fn current_or_mint(&self, id: &Identity) -> Result<MintedJwt, SpineError> {
        let now = chrono::Utc::now().timestamp();
        {
            let guard = self.inner.read().await;
            if let Some(jwt) = guard.as_ref() {
                if !jwt.needs_refresh(now) {
                    return Ok(jwt.clone());
                }
            }
        }
        let minted = id.with_signing_key(|sk| crypto::mint_jwt(sk, id.device_uuid()))?;
        *self.inner.write().await = Some(minted.clone());
        Ok(minted)
    }

    /// Force a refresh (called after a 401).
    pub async fn refresh(&self, id: &Identity) -> Result<MintedJwt, SpineError> {
        let minted = id.with_signing_key(|sk| crypto::mint_jwt(sk, id.device_uuid()))?;
        *self.inner.write().await = Some(minted.clone());
        Ok(minted)
    }

    /// Drop the current token; the next `current_or_mint` mints a fresh one.
    pub async fn clear(&self) {
        *self.inner.write().await = None;
    }

    /// Snapshot the current token (useful for revocation).
    pub async fn peek(&self) -> Option<MintedJwt> {
        self.inner.read().await.clone()
    }
}

impl Default for JwtHolder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Request / response shapes (mirror server pairing.go and sync.go)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct InitiateRequest<'a> {
    pub device_uuid: &'a str,
    pub initiator_pubkey: String,
    pub device_type: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitiateResponse {
    pub session_id: String,
    pub qr_payload: String,
    pub short_code: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompleteRequest<'a> {
    pub session_id: &'a str,
    pub device_uuid: &'a str,
    pub responder_pubkey: String,
    pub device_type: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompleteResponse {
    pub status: String,
    pub initiator_id: Option<String>,
    pub responder_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairingStatusResponse {
    pub status: String,
    pub expires_at: String,
    #[serde(default)]
    pub paired_device_id: Option<String>,
    /// Base64url (no padding) of the responder's raw 32-byte Ed25519 public key, present
    /// only when `status == "completed"`. The initiator uses this to derive `sync_key`.
    #[serde(default)]
    pub responder_pubkey: Option<String>,
    /// Base64url (no padding) of the initiator's raw 32-byte Ed25519 public key, present
    /// only when `status == "completed"`. The responder learns this from the QR payload
    /// directly but the value is mirrored back here for inspectability.
    #[serde(default)]
    pub initiator_pubkey: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleListItem {
    pub bundle_id: String,
    pub from_device: String,
    pub payload_size: u64,
    pub content_type: String,
    pub created_at: String,
    pub payload_hash: String,
}

#[derive(Debug, Clone)]
pub struct DownloadedBundle {
    pub payload: Vec<u8>,
    pub content_type: String,
    pub payload_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadBundleResponse {
    pub bundle_id: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct SpineClient {
    http: Client,
    base_url: Url,
    identity: Arc<Identity>,
    jwt: Arc<JwtHolder>,
}

impl SpineClient {
    /// Construct a new client. `base_url` is the user-supplied Spine URL; `trust_ca_path`
    /// optionally adds a self-signed CA to the rustls root store. `danger_accept_invalid_certs`
    /// is NEVER set — verification still applies.
    pub fn new(
        base_url: &str,
        trust_ca_path: Option<&Path>,
        identity: Arc<Identity>,
        jwt: Arc<JwtHolder>,
    ) -> Result<Self, SpineError> {
        let cfg = syncmind_core::SpineConfig {
            url: Some(base_url.to_string()),
            trust_ca_path: trust_ca_path.map(Path::to_path_buf),
            ..syncmind_core::SpineConfig::default()
        };
        let parsed = cfg.validate_url().map_err(SpineError::from)?;

        let mut builder = ClientBuilder::new()
            .use_rustls_tls()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60));

        for der in cfg.load_trust_ca().map_err(SpineError::from)? {
            let cert = Certificate::from_der(der.as_ref())
                .map_err(|e| SpineError::new(SpineErrorCode::TrustCaInvalidPem, e.to_string()))?;
            builder = builder.add_root_certificate(cert);
        }

        let http = builder
            .build()
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;

        Ok(Self {
            http,
            base_url: parsed,
            identity,
            jwt,
        })
    }

    fn url(&self, path: &str) -> Result<Url, SpineError> {
        self.base_url
            .join(path)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
    }

    /// WebSocket URL: same host as base_url, scheme upgraded to ws:// or wss://.
    pub fn websocket_url(&self, path: &str) -> Result<String, SpineError> {
        let mut u = self
            .base_url
            .join(path)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        let new_scheme = match u.scheme() {
            "https" => "wss",
            "http" => "ws",
            other => {
                return Err(SpineError::new(
                    SpineErrorCode::InvalidUrl,
                    format!("unsupported scheme for websocket: {other}"),
                ))
            }
        };
        u.set_scheme(new_scheme)
            .map_err(|()| SpineError::new(SpineErrorCode::Internal, "set_scheme failed"))?;
        Ok(u.to_string())
    }

    // -----------------------------------------------------------------------
    // Pairing
    // -----------------------------------------------------------------------

    pub async fn pairing_initiate(
        &self,
        device_type: &str,
    ) -> Result<InitiateResponse, SpineError> {
        let pubkey_b64 = B64URL.encode(self.identity.public_key_bytes());
        let body = InitiateRequest {
            device_uuid: self.identity.device_uuid(),
            initiator_pubkey: pubkey_b64,
            device_type,
        };
        let url = self.url("v1/pairing/initiate")?;
        let resp = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(unreachable_to_spine_err)?;
        json_or_error(resp).await
    }

    pub async fn pairing_complete(
        &self,
        session_id: &str,
        responder_pubkey_raw: &[u8; 32],
        device_type: &str,
    ) -> Result<CompleteResponse, SpineError> {
        let body = CompleteRequest {
            session_id,
            device_uuid: self.identity.device_uuid(),
            responder_pubkey: B64URL.encode(responder_pubkey_raw),
            device_type,
        };
        let url = self.url("v1/pairing/complete")?;
        let resp = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(unreachable_to_spine_err)?;
        json_or_error(resp).await
    }

    pub async fn pairing_status(
        &self,
        session_id: &str,
    ) -> Result<PairingStatusResponse, SpineError> {
        let url = self.url(&format!("v1/pairing/{session_id}/status"))?;
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(unreachable_to_spine_err)?;
        json_or_error(resp).await
    }

    // -----------------------------------------------------------------------
    // Bundles
    // -----------------------------------------------------------------------

    pub async fn list_bundles(&self, limit: u32) -> Result<Vec<BundleListItem>, SpineError> {
        let url = self.url(&format!("v1/sync/bundles?limit={limit}"))?;
        let resp = self
            .send_authenticated(Method::GET, url, None, None)
            .await?;
        json_or_error(resp).await
    }

    pub async fn download_bundle(&self, bundle_id: &str) -> Result<DownloadedBundle, SpineError> {
        let url = self.url(&format!("v1/sync/bundles/{bundle_id}"))?;
        let resp = self
            .send_authenticated(Method::GET, url, None, None)
            .await?;
        if !resp.status().is_success() {
            return Err(http_status_to_spine_err(&resp));
        }
        let content_type = header_string(resp.headers(), X_CONTENT_TYPE_HEADER)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let payload_hash =
            header_string(resp.headers(), X_PAYLOAD_HASH_HEADER).ok_or_else(|| {
                SpineError::new(
                    SpineErrorCode::EnvelopeIntegrityFailed,
                    "missing X-Syncmind-Payload-Hash header on bundle download",
                )
            })?;
        let payload = resp
            .bytes()
            .await
            .map_err(unreachable_to_spine_err)?
            .to_vec();
        // Verify transport hash (PRD 004 §US-026 step 1 of the inbound integrity check).
        let computed = hex::encode(crypto::sha256(&payload));
        if computed != payload_hash {
            return Err(SpineError::new(
                SpineErrorCode::EnvelopeIntegrityFailed,
                "downloaded bundle SHA-256 does not match X-Syncmind-Payload-Hash",
            ));
        }
        Ok(DownloadedBundle {
            payload,
            content_type,
            payload_hash,
        })
    }

    pub async fn upload_bundle(
        &self,
        blob: Vec<u8>,
        content_type: &str,
        idempotency_key: &str,
    ) -> Result<UploadBundleResponse, SpineError> {
        let url = self.url("v1/sync/bundle")?;
        let mut extra = HeaderMap::new();
        extra.insert(
            HeaderName::from_static("x-syncmind-content-type"),
            HeaderValue::from_str(content_type)
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?,
        );
        extra.insert(
            HeaderName::from_static(IDEMPOTENCY_KEY_HEADER),
            HeaderValue::from_str(idempotency_key)
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?,
        );
        extra.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );

        let resp = self
            .send_authenticated(Method::POST, url, Some(blob), Some(extra))
            .await?;
        json_or_error(resp).await
    }

    pub async fn delete_bundle(&self, bundle_id: &str) -> Result<(), SpineError> {
        let url = self.url(&format!("v1/sync/bundles/{bundle_id}"))?;
        let resp = self
            .send_authenticated(Method::DELETE, url, None, None)
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(http_status_to_spine_err(&resp))
        }
    }

    pub async fn auth_revoke(&self) -> Result<(), SpineError> {
        let url = self.url("v1/auth/revoke")?;
        let resp = self
            .send_authenticated(Method::POST, url, None, None)
            .await?;
        // 204 or 200 both acceptable; 401 means the JWT was already invalid (effectively
        // revoked). We don't propagate 401 here because best-effort.
        let status = resp.status();
        if status.is_success() || status == StatusCode::UNAUTHORIZED {
            Ok(())
        } else {
            Err(http_status_to_spine_err(&resp))
        }
    }

    // -----------------------------------------------------------------------
    // Internal request runner: JWT injection + 401 refresh + 429/5xx retry
    // -----------------------------------------------------------------------

    async fn send_authenticated(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        extra_headers: Option<HeaderMap>,
    ) -> Result<Response, SpineError> {
        let mut attempt: usize = 0;
        let mut refreshed = false;

        loop {
            let jwt = self.jwt.current_or_mint(&self.identity).await?;
            let mut req = self.http.request(method.clone(), url.clone()).header(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", jwt.token))
                    .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?,
            );
            if let Some(extra) = extra_headers.as_ref() {
                req = req.headers(extra.clone());
            }
            if let Some(b) = body.clone() {
                req = req.body(b);
            }
            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    // Network-layer failure → retry the same way as 5xx.
                    if attempt < MAX_RETRIES {
                        let backoff = Duration::from_millis(RETRY_BASE_MS << attempt);
                        debug!(?backoff, attempt, error = %e, "spine request transport error; backing off");
                        tokio::time::sleep(backoff).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(SpineError::new(
                        SpineErrorCode::SpineUnreachable,
                        e.to_string(),
                    ));
                }
            };

            let status = response.status();

            if status == StatusCode::UNAUTHORIZED && !refreshed {
                warn!("spine returned 401; refreshing JWT once");
                self.jwt.refresh(&self.identity).await?;
                refreshed = true;
                continue;
            }

            // 429 / 5xx → backoff + retry.
            if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && attempt < MAX_RETRIES
            {
                let backoff = Duration::from_millis(RETRY_BASE_MS << attempt);
                debug!(?backoff, attempt, %status, "spine returned retryable status; backing off");
                tokio::time::sleep(backoff).await;
                attempt += 1;
                continue;
            }

            return Ok(response);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unreachable_to_spine_err(e: reqwest::Error) -> SpineError {
    SpineError::new(SpineErrorCode::SpineUnreachable, e.to_string())
}

async fn json_or_error<T: for<'de> Deserialize<'de>>(resp: Response) -> Result<T, SpineError> {
    let status = resp.status();
    if !status.is_success() {
        return Err(http_status_to_spine_err_owned(resp).await);
    }
    resp.json::<T>()
        .await
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

fn http_status_to_spine_err(resp: &Response) -> SpineError {
    let status = resp.status();
    let code = match status {
        StatusCode::UNAUTHORIZED => SpineErrorCode::AuthInvalid,
        StatusCode::NOT_FOUND => SpineErrorCode::Internal,
        StatusCode::CONFLICT => SpineErrorCode::AlreadyPaired,
        StatusCode::GONE => SpineErrorCode::PairingExpired,
        StatusCode::PAYLOAD_TOO_LARGE => SpineErrorCode::BundleTooLarge,
        StatusCode::TOO_MANY_REQUESTS => SpineErrorCode::SpineUnreachable,
        _ if status.is_server_error() => SpineErrorCode::SpineUnreachable,
        _ => SpineErrorCode::Internal,
    };
    SpineError::new(code, format!("spine returned HTTP {status}"))
}

async fn http_status_to_spine_err_owned(resp: Response) -> SpineError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let code = match status {
        StatusCode::UNAUTHORIZED => SpineErrorCode::AuthInvalid,
        StatusCode::NOT_FOUND => SpineErrorCode::Internal,
        StatusCode::CONFLICT => SpineErrorCode::AlreadyPaired,
        StatusCode::GONE => SpineErrorCode::PairingExpired,
        StatusCode::PAYLOAD_TOO_LARGE => SpineErrorCode::BundleTooLarge,
        StatusCode::TOO_MANY_REQUESTS => SpineErrorCode::SpineUnreachable,
        _ if status.is_server_error() => SpineErrorCode::SpineUnreachable,
        _ => SpineErrorCode::Internal,
    };
    SpineError::new(code, format!("spine returned HTTP {status}: {body}"))
}

fn header_string(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok().map(String::from))
}

/// Convenience: generate a fresh UUIDv4 for use as an `Idempotency-Key`.
pub fn new_idempotency_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_idempotency_key_is_uuid_v4() {
        let key = new_idempotency_key();
        let parsed = uuid::Uuid::parse_str(&key).unwrap();
        assert_eq!(parsed.get_version_num(), 4);
    }
}
