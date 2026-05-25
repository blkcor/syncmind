//! HTTP client for the Spine sync gateway.
//!
//! Builds a `reqwest::Client` with rustls and (optionally) a user-supplied PEM trust anchor
//! per PRD 004 §US-028 / §Decisions §9. Wraps every endpoint with Bearer JWT injection,
//! per-bundle `Idempotency-Key`, and exponential-backoff retry on 429/5xx (PRD 004 §US-032,
//! §US-033).

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_code: Option<&'a str>,
    pub device_uuid: &'a str,
    pub responder_pubkey: String,
    pub device_type: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompleteResponse {
    pub status: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub initiator_id: Option<String>,
    pub responder_id: Option<String>,
    #[serde(default)]
    pub initiator_pubkey: Option<String>,
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
    max_retries: usize,
    retry_base_ms: u64,
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
            max_retries: MAX_RETRIES,
            retry_base_ms: RETRY_BASE_MS,
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
        self.pairing_complete_with_locator(
            Some(session_id),
            None,
            responder_pubkey_raw,
            device_type,
        )
        .await
    }

    pub async fn pairing_complete_short_code(
        &self,
        short_code: &str,
        responder_pubkey_raw: &[u8; 32],
        device_type: &str,
    ) -> Result<CompleteResponse, SpineError> {
        self.pairing_complete_with_locator(
            None,
            Some(short_code),
            responder_pubkey_raw,
            device_type,
        )
        .await
    }

    async fn pairing_complete_with_locator(
        &self,
        session_id: Option<&str>,
        short_code: Option<&str>,
        responder_pubkey_raw: &[u8; 32],
        device_type: &str,
    ) -> Result<CompleteResponse, SpineError> {
        let body = CompleteRequest {
            session_id,
            short_code,
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
        // Verify transport hash (PRD 004 §US-034 step 1 of the inbound integrity check).
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
            HeaderName::from_bytes(b"x-syncmind-content-type").expect("valid header name"),
            HeaderValue::from_str(content_type)
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?,
        );
        extra.insert(
            HeaderName::from_bytes(b"Idempotency-Key").expect("valid header name"),
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
                    if attempt < self.max_retries {
                        let backoff = Duration::from_millis(self.retry_base_ms << attempt);
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
                && attempt < self.max_retries
            {
                let backoff = Duration::from_millis(self.retry_base_ms << attempt);
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
    use crate::spine::identity::{fingerprint_hex, DeviceMetadata};
    use ed25519_dalek::SigningKey;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn idempotency_header() -> reqwest::header::HeaderName {
        reqwest::header::HeaderName::from_bytes(b"Idempotency-Key").unwrap()
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn test_identity() -> Identity {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let vk = signing_key.verifying_key();
        Identity::from_parts(
            signing_key,
            DeviceMetadata {
                fingerprint: fingerprint_hex(&vk.to_bytes()),
                device_type: "desktop".to_string(),
                device_uuid: Uuid::new_v4().to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
    }

    /// Build a SpineClient wired to a live wiremock server with fast retry for tests.
    fn test_client(base_url: String) -> (SpineClient, Arc<Identity>, Arc<JwtHolder>) {
        let identity = Arc::new(test_identity());
        let jwt = Arc::new(JwtHolder::new());
        let mut client = SpineClient::new(&base_url, None, identity.clone(), jwt.clone()).unwrap();
        // Fast retry for tests: at most 1 retry, 1 ms base backoff.
        client.max_retries = 1;
        client.retry_base_ms = 1;
        (client, identity, jwt)
    }

    /// Snapshot received requests from the wiremock server.
    async fn received_requests(server: &MockServer) -> Vec<wiremock::Request> {
        server.received_requests().await.unwrap().clone()
    }

    // -----------------------------------------------------------------------
    // Unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn new_idempotency_key_is_uuid_v4() {
        let key = new_idempotency_key();
        let parsed = uuid::Uuid::parse_str(&key).unwrap();
        assert_eq!(parsed.get_version_num(), 4);
    }

    // -----------------------------------------------------------------------
    // Happy-path integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_bundles_parses_empty_list() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!([]))),
            )
            .await;

        let bundles = client.list_bundles(10).await.unwrap();
        assert!(bundles.is_empty());
    }

    #[tokio::test]
    async fn list_bundles_parses_multiple_items() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                        {
                            "bundle_id": "b-1",
                            "from_device": "d-a",
                            "payload_size": 42,
                            "content_type": "application/x-syncmind-note",
                            "created_at": "2026-01-01T00:00:00Z",
                            "payload_hash": "a".repeat(64),
                        },
                        {
                            "bundle_id": "b-2",
                            "from_device": "d-b",
                            "payload_size": 7,
                            "content_type": "application/x-syncmind-note",
                            "created_at": "2026-01-02T00:00:00Z",
                            "payload_hash": "b".repeat(64),
                        },
                    ]))),
            )
            .await;

        let bundles = client.list_bundles(10).await.unwrap();
        assert_eq!(bundles.len(), 2);
        assert_eq!(bundles[0].bundle_id, "b-1");
        assert_eq!(bundles[1].bundle_id, "b-2");
    }

    #[tokio::test]
    async fn upload_bundle_sends_idempotency_key_and_succeeds() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());
        let id_key = "my-idempotency-key-42";

        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/sync/bundle"))
                    .and(header(idempotency_header(), id_key.to_string()))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_json(json!({"bundle_id": "b-new"})),
                    ),
            )
            .await;

        let resp = client
            .upload_bundle(b"hello".to_vec(), "application/x-syncmind-note", id_key)
            .await
            .unwrap();
        assert_eq!(resp.bundle_id, "b-new");
    }

    #[tokio::test]
    async fn pairing_initiate_returns_session() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/pairing/initiate"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "session_id": "s-1",
                        "qr_payload": "qr-data",
                        "short_code": "123456",
                        "expires_at": "2026-12-31T23:59:59Z",
                    }))),
            )
            .await;

        let resp = client.pairing_initiate("desktop").await.unwrap();
        assert_eq!(resp.session_id, "s-1");
        assert_eq!(resp.short_code, "123456");
    }

    #[tokio::test]
    async fn delete_bundle_success_on_200() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("DELETE"))
                    .and(path("/v1/sync/bundles/b-1"))
                    .respond_with(ResponseTemplate::new(200)),
            )
            .await;

        assert!(client.delete_bundle("b-1").await.is_ok());
    }

    #[tokio::test]
    async fn delete_bundle_ok_on_204() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("DELETE"))
                    .and(path("/v1/sync/bundles/b-1"))
                    .respond_with(ResponseTemplate::new(204)),
            )
            .await;

        assert!(client.delete_bundle("b-1").await.is_ok());
    }

    #[tokio::test]
    async fn auth_revoke_best_effort_200_or_401() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        // 200 is success
        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/auth/revoke"))
                    .respond_with(ResponseTemplate::new(200)),
            )
            .await;
        assert!(client.auth_revoke().await.is_ok());
    }

    // -----------------------------------------------------------------------
    // 401 → JWT refresh → retry once
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn auth_401_refreshes_jwt_and_retries_once_then_succeeds() {
        let server = MockServer::start().await;
        let (client, identity, jwt) = test_client(server.uri());

        // Snapshot the JWT the client will use on its first request.
        let first_jwt = jwt.current_or_mint(&identity).await.unwrap();

        // Request bearing the *original* JWT → 401.
        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .and(header(
                        "Authorization",
                        format!("Bearer {}", first_jwt.token),
                    ))
                    .respond_with(ResponseTemplate::new(401)),
            )
            .await;

        // Any other request to the same endpoint → 200 (catches the retry).
        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!([]))),
            )
            .await;

        let result = client.list_bundles(10).await;
        assert!(
            result.is_ok(),
            "expected Ok after JWT refresh, got {result:?}"
        );
    }

    #[tokio::test]
    async fn auth_401_twice_returns_auth_invalid_error() {
        let server = MockServer::start().await;
        let (client, identity, jwt) = test_client(server.uri());

        let first_jwt = jwt.current_or_mint(&identity).await.unwrap();

        // Original JWT → 401.
        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .and(header(
                        "Authorization",
                        format!("Bearer {}", first_jwt.token),
                    ))
                    .respond_with(ResponseTemplate::new(401)),
            )
            .await;

        // Refreshed JWT → also 401.
        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(401)),
            )
            .await;

        let err = client.list_bundles(10).await.unwrap_err();
        assert_eq!(err.code, "AUTH_INVALID");
    }

    // -----------------------------------------------------------------------
    // 5xx / retry exhaustion + idempotency-key reuse
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn server_error_retries_and_exhausts() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(503)),
            )
            .await;

        let err = client.list_bundles(10).await.unwrap_err();
        assert_eq!(err.code, "SPINE_UNREACHABLE");

        // With max_retries=1 we expect: initial request + 1 retry = 2 total.
        let reqs = received_requests(&server).await;
        assert_eq!(reqs.len(), 2, "expected 1 initial + 1 retry");
    }

    #[tokio::test]
    async fn idempotency_key_reused_across_retries() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());
        let id_key = "reused-key-across-retries";

        // Always return 503 to trigger retry.
        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/sync/bundle"))
                    .respond_with(ResponseTemplate::new(503)),
            )
            .await;

        let _ = client
            .upload_bundle(b"payload".to_vec(), "application/x-syncmind-note", id_key)
            .await;

        let reqs = received_requests(&server).await;
        assert!(
            reqs.len() >= 2,
            "expected at least 2 attempts, got {}",
            reqs.len()
        );

        let idempotency_header = idempotency_header();

        // Every request must carry the same Idempotency-Key.
        for (i, req) in reqs.iter().enumerate() {
            let header_val = req
                .headers
                .get(&idempotency_header)
                .map(|v| v.to_str().unwrap().to_string());
            assert_eq!(
                header_val.as_deref(),
                Some(id_key),
                "request {i} had wrong Idempotency-Key"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Error code mapping
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn http_404_maps_to_internal_error() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(404)),
            )
            .await;

        let err = client.list_bundles(10).await.unwrap_err();
        assert_eq!(err.code, "INTERNAL_ERROR");
    }

    #[tokio::test]
    async fn http_429_retries_then_fails() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles"))
                    .and(query_param("limit", "10"))
                    .respond_with(ResponseTemplate::new(429)),
            )
            .await;

        let err = client.list_bundles(10).await.unwrap_err();
        // 429 is retried, then exhausted → SPINE_UNREACHABLE.
        assert_eq!(err.code, "SPINE_UNREACHABLE");

        let reqs = received_requests(&server).await;
        assert_eq!(reqs.len(), 2, "expected 1 initial + 1 retry for 429");
    }

    #[tokio::test]
    async fn pairing_status_parses_completed() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/pairing/s-1/status"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "status": "completed",
                        "expires_at": "2026-12-31T23:59:59Z",
                    }))),
            )
            .await;

        let status = client.pairing_status("s-1").await.unwrap();
        assert_eq!(status.status, "completed");
    }

    #[tokio::test]
    async fn pairing_complete_sends_device_uuid() {
        let server = MockServer::start().await;
        let (client, identity, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/pairing/complete"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "status": "completed",
                        "initiator_id": null,
                        "responder_id": null,
                    }))),
            )
            .await;

        let resp = client
            .pairing_complete("s-1", &[0xabu8; 32], "desktop")
            .await
            .unwrap();
        assert_eq!(resp.status, "completed");

        // Verify the request body included the device UUID.
        let reqs = received_requests(&server).await;
        assert_eq!(reqs.len(), 1);
        let body: serde_json::Value =
            serde_json::from_slice(&reqs[0].body).expect("valid JSON body");
        assert_eq!(
            body["device_uuid"].as_str().unwrap(),
            identity.device_uuid()
        );
        assert_eq!(body["session_id"].as_str().unwrap(), "s-1");
        assert!(body.get("short_code").is_none());
    }

    #[tokio::test]
    async fn pairing_complete_short_code_sends_short_code() {
        let server = MockServer::start().await;
        let (client, identity, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("POST"))
                    .and(path("/v1/pairing/complete"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "status": "completed",
                        "session_id": "s-1",
                        "initiator_id": "i-1",
                        "responder_id": "r-1",
                        "initiator_pubkey": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x11u8; 32]),
                    }))),
            )
            .await;

        let resp = client
            .pairing_complete_short_code("123-456", &[0xabu8; 32], "desktop")
            .await
            .unwrap();
        assert_eq!(resp.status, "completed");
        assert_eq!(resp.session_id.as_deref(), Some("s-1"));
        assert_eq!(resp.initiator_id.as_deref(), Some("i-1"));
        assert!(resp.initiator_pubkey.is_some());

        let reqs = received_requests(&server).await;
        assert_eq!(reqs.len(), 1);
        let body: serde_json::Value =
            serde_json::from_slice(&reqs[0].body).expect("valid JSON body");
        assert_eq!(
            body["device_uuid"].as_str().unwrap(),
            identity.device_uuid()
        );
        assert_eq!(body["short_code"].as_str().unwrap(), "123-456");
        assert!(body.get("session_id").is_none());
    }

    #[tokio::test]
    async fn download_bundle_verifies_transport_hash() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        let payload = b"hello-download";
        let hash = hex::encode(crypto::sha256(payload));

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles/b-dl"))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .set_body_bytes(payload.to_vec())
                            .insert_header("X-Syncmind-Content-Type", "application/x-syncmind-note")
                            .insert_header("X-Syncmind-Payload-Hash", hash.as_str()),
                    ),
            )
            .await;

        let bundle = client.download_bundle("b-dl").await.unwrap();
        assert_eq!(bundle.payload, payload);
        assert_eq!(bundle.content_type, "application/x-syncmind-note");
    }

    #[tokio::test]
    async fn download_bundle_hash_mismatch_errors() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles/b-bad"))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .set_body_bytes(b"real-payload".to_vec())
                            .insert_header("X-Syncmind-Content-Type", "text/plain")
                            .insert_header(
                                "X-Syncmind-Payload-Hash",
                                hex::encode(crypto::sha256(b"different")),
                            ),
                    ),
            )
            .await;

        let err = client.download_bundle("b-bad").await.unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[tokio::test]
    async fn download_bundle_missing_hash_header_errors() {
        let server = MockServer::start().await;
        let (client, _, _) = test_client(server.uri());

        server
            .register(
                Mock::given(method("GET"))
                    .and(path("/v1/sync/bundles/b-nohash"))
                    .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data".to_vec())),
            )
            .await;

        let err = client.download_bundle("b-nohash").await.unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }
}
