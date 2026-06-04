//! Pairing flow: initiate, poll for completion, derive sync_key.
//!
//! PRD 004 §US-030 / §US-031. The desktop acts as the initiator — it displays a QR PNG
//! plus a 6-digit short code and polls `GET /v1/pairing/:session_id/status` every second
//! until the responder completes the session or the TTL elapses.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use image::{ImageBuffer, Luma};
use qrcode::QrCode;
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use url::Url;

use crate::spine::client::{PairingStatusResponse, SpineClient};
use crate::spine::crypto;
use crate::spine::identity::{self, Identity};
use crate::spine::{SpineError, SpineErrorCode};

/// 1-second polling interval for the pairing status loop (PRD 004 §US-030).
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// QR PNG side length in pixels.
const QR_PNG_SIDE_PX: u32 = 320;

/// Tolerance for clock skew when validating `expires_at` on inbound payloads.
const EXPIRY_SKEW: chrono::Duration = chrono::Duration::seconds(60);

/// Constant `kind` discriminator for the mobile pairing payload schema (PRD 005 §US-041).
const PAIRING_PAYLOAD_KIND: &str = "syncmind-pairing";

/// Versioned QR payload emitted by the desktop initiator for mobile (or future desktop)
/// scanners (PRD 005 §US-041 / §US-052). Serialized as a UTF-8 JSON object and rendered
/// directly as QR content. `pairing_token` and `device_a_pubkey` are both the desktop's
/// long-term Ed25519 identity pubkey encoded as base64url-no-pad — see [`build_mobile_pairing_payload`]
/// for the encoding choice rationale.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MobilePairingPayload {
    pub v: u8,
    pub kind: String,
    pub session_id: String,
    pub spine_url: String,
    pub ca_fingerprint: Option<String>,
    pub pairing_token: String,
    pub expires_at: String,
    pub device_a_pubkey: String,
    pub device_a_fingerprint: String,
}

impl std::fmt::Debug for MobilePairingPayload {
    /// Redacts `pairing_token` to "<first4>…<last4>" so accidental `dbg!()` calls on a
    /// payload do not leak the token to logs or panic backtraces (tasks.md §7.2).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let token = redact_token(&self.pairing_token);
        f.debug_struct("MobilePairingPayload")
            .field("v", &self.v)
            .field("kind", &self.kind)
            .field("session_id", &self.session_id)
            .field("spine_url", &self.spine_url)
            .field("ca_fingerprint", &self.ca_fingerprint)
            .field("pairing_token", &token)
            .field("expires_at", &self.expires_at)
            .field("device_a_pubkey", &token_like(&self.device_a_pubkey))
            .field("device_a_fingerprint", &self.device_a_fingerprint)
            .finish()
    }
}

fn redact_token(s: &str) -> String {
    if s.len() <= 8 {
        return "<redacted>".into();
    }
    format!("{}…{}", &s[..4], &s[s.len() - 4..])
}

fn token_like(s: &str) -> String {
    redact_token(s)
}

/// Returned to the frontend so it can render a QR + short code countdown.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PairingHandleView {
    pub session_id: String,
    pub short_code: String,
    pub qr_png_base64: String,
    /// Raw JSON string identical to the QR-encoded content. PRD 005 §US-052 — exposed so the
    /// frontend Devices tab can offer "Copy payload" UX in the future without re-running the
    /// initiate flow. Existing frontends that ignore this field continue to function.
    pub qr_payload_json: String,
    pub expires_at: String,
}

/// Successful completion of pairing. The desktop has derived (and cached) the sync_key by
/// the time this is returned.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PairingCompletion {
    pub peer_fingerprint: String,
    pub peer_device_id: Option<String>,
    pub peer_pubkey_raw: [u8; 32],
    /// SHA-256 lower-hex of `sync_key` — purely for UI display so the user can spot-verify
    /// against the peer's identical display. NOT the sync_key itself.
    pub sync_key_fingerprint: String,
}

/// Progress callback used by responder pairing to report user-visible steps.
pub type PairingProgressCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// Complete pairing as the responder using a gateway short code.
///
/// `on_progress` is an optional callback that receives step labels so the
/// caller can emit frontend events.
pub async fn complete_with_short_code(
    client: &SpineClient,
    identity: &Identity,
    short_code: &str,
    on_progress: Option<PairingProgressCallback>,
) -> Result<PairingCompletion, SpineError> {
    if let Some(ref cb) = on_progress {
        cb("contacting_server");
    }

    let normalized = normalize_short_code(short_code)?;
    let resp = client
        .pairing_complete_short_code(
            &normalized,
            &identity.public_key_bytes(),
            identity.device_type(),
        )
        .await?;
    if resp.status != "completed" {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            format!("unexpected pairing completion status: {}", resp.status),
        ));
    }
    let session_id = resp.session_id.as_deref().ok_or_else(|| {
        SpineError::new(
            SpineErrorCode::Internal,
            "short-code completion response missing session_id",
        )
    })?;
    let initiator_b64 = resp.initiator_pubkey.as_deref().ok_or_else(|| {
        SpineError::new(
            SpineErrorCode::Internal,
            "short-code completion response missing initiator_pubkey",
        )
    })?;

    completion_from_peer_pubkey(
        identity,
        session_id,
        initiator_b64,
        resp.initiator_id,
        on_progress.as_ref(),
    )
    .await
}

/// Outcome of a single `poll_once` iteration.
#[derive(Debug)]
pub enum PollOutcome {
    /// Still waiting for the responder.
    Pending,
    /// Responder completed; sync_key has been derived and cached.
    Completed(PairingCompletion),
    /// Session expired or was cancelled server-side.
    Expired,
}

/// Initiate a pairing session and produce the data the UI needs.
///
/// PRD 005 §US-052: the QR content is now a versioned JSON object so mobile scanners can
/// resolve the spine URL, optional self-signed CA fingerprint, and the initiator's identity
/// pubkey from a single scan. The frontend additionally receives the raw JSON string via
/// `PairingHandleView::qr_payload_json` for future "copy payload" UX.
pub async fn initiate(
    client: &SpineClient,
    identity: &Identity,
    config: &syncmind_core::SpineConfig,
) -> Result<(PairingHandleView, String), SpineError> {
    let resp = client.pairing_initiate(identity.device_type()).await?;
    let payload = build_mobile_pairing_payload(config, identity, &resp)?;
    let qr_payload_json = serde_json::to_string(&payload).map_err(|e| {
        SpineError::new(
            SpineErrorCode::Internal,
            format!("serialize pairing payload: {e}"),
        )
    })?;
    let qr_png_base64 = render_qr_png_base64(&qr_payload_json)?;
    let view = PairingHandleView {
        session_id: resp.session_id.clone(),
        short_code: resp.short_code,
        qr_png_base64,
        qr_payload_json,
        expires_at: resp.expires_at,
    };
    Ok((view, resp.session_id))
}

/// Construct the v1 mobile pairing payload from the server response and local config.
///
/// Field sources:
///
/// - `session_id` — Spine `pairing_initiate` response UUID. Mobile uses this as the
///   only `/v1/pairing/complete` locator; `pairing_token` remains opaque legacy material.
/// - `spine_url` — `config.url` (required; absence returns `SPINE_NOT_CONFIGURED`).
/// - `ca_fingerprint` — `sha256:<hex>` over the first PEM CERTIFICATE block at
///   `config.trust_ca_path` if set and readable; otherwise `None`.
/// - `pairing_token` — the `?pk=` query value lifted from `resp.qr_payload`. The Spine
///   uses the initiator pubkey itself as the implicit session credential, so re-publishing
///   it under a token-shaped field keeps the schema future-proof (D2 in design.md) without
///   requiring server changes.
/// - `device_a_pubkey` — base64url-no-pad encoding of the same 32 raw Ed25519 bytes. Same
///   encoding as `pairing_token` so mobile can `decode_b64url` once and reuse the result
///   for both fields.
/// - `device_a_fingerprint` — `sha256:<hex>` of the raw Ed25519 pubkey bytes (matches
///   [`identity::fingerprint_hex`]).
fn build_mobile_pairing_payload(
    config: &syncmind_core::SpineConfig,
    identity: &Identity,
    resp: &crate::spine::client::InitiateResponse,
) -> Result<MobilePairingPayload, SpineError> {
    let spine_url = config
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            SpineError::new(
                SpineErrorCode::SpineNotConfigured,
                "spine.url must be set before initiating pairing",
            )
        })?
        .to_string();

    let ca_fingerprint = compute_ca_fingerprint(config);

    let pairing_token = extract_pk_from_qr_payload(&resp.qr_payload)?;

    let pubkey_bytes = identity.public_key_bytes();
    let device_a_pubkey = B64URL.encode(pubkey_bytes);
    let device_a_fingerprint = format!("sha256:{}", hex::encode(Sha256::digest(pubkey_bytes)));

    Ok(MobilePairingPayload {
        v: 1,
        kind: PAIRING_PAYLOAD_KIND.to_string(),
        session_id: resp.session_id.clone(),
        spine_url,
        ca_fingerprint,
        pairing_token,
        expires_at: resp.expires_at.clone(),
        device_a_pubkey,
        device_a_fingerprint,
    })
}

/// Read the first CERTIFICATE block from `config.trust_ca_path` and return a
/// `sha256:<lower-hex>` fingerprint of its DER bytes. Returns `None` when no path is
/// configured. Logs (but does not propagate) read or parse failures — the payload still
/// emits with `ca_fingerprint: null` so the mobile scanner falls back to system trust.
fn compute_ca_fingerprint(config: &syncmind_core::SpineConfig) -> Option<String> {
    let path = config.trust_ca_path.as_ref()?;
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "trust_ca_path unreadable while building pairing payload; emitting null ca_fingerprint"
            );
            return None;
        }
    };
    let mut reader = std::io::Cursor::new(&bytes);
    let first = rustls_pemfile::certs(&mut reader).next();
    match first {
        Some(Ok(cert)) => Some(format!(
            "sha256:{}",
            hex::encode(Sha256::digest(cert.as_ref()))
        )),
        Some(Err(e)) => {
            warn!(
                path = %path.display(),
                error = %e,
                "trust_ca_path PEM parse failed while building pairing payload"
            );
            None
        }
        None => {
            warn!(
                path = %path.display(),
                "trust_ca_path contained no CERTIFICATE blocks"
            );
            None
        }
    }
}

/// Pull the `pk` query value from a `spine://pair/{session}?pk={b64url}` URI. The Spine
/// server is the only authority on this format, so anything else is a protocol violation
/// and surfaces as `INTERNAL_ERROR` (the user can't act on it).
fn extract_pk_from_qr_payload(uri: &str) -> Result<String, SpineError> {
    let url = Url::parse(uri).map_err(|e| {
        SpineError::new(
            SpineErrorCode::Internal,
            format!("server qr_payload not a URL: {e}"),
        )
    })?;
    if url.scheme() != "spine" {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            format!(
                "server qr_payload scheme: expected spine, got {}",
                url.scheme()
            ),
        ));
    }
    let pk = url
        .query_pairs()
        .find(|(k, _)| k == "pk")
        .map(|(_, v)| v.into_owned())
        .ok_or_else(|| {
            SpineError::new(
                SpineErrorCode::Internal,
                "server qr_payload missing ?pk= query param",
            )
        })?;
    if pk.is_empty() {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            "server qr_payload had empty pk=",
        ));
    }
    Ok(pk)
}

/// Parse an inbound pairing payload string. Accepts two formats:
///
/// 1. **v1 JSON** — produced by [`build_mobile_pairing_payload`]. Validates `v == 1`,
///    `kind == "syncmind-pairing"`, and `expires_at` within ±60 s of the local clock.
/// 2. **Legacy `spine://pair/{session}?pk={b64url}` URI** — the format the Spine server
///    historically published as `qr_payload`. Kept reachable for future desktop-side
///    scanning UI; currently exercised only by unit tests.
///
/// Any other shape returns `BAD_REQUEST` so callers can surface a precise UI message.
#[allow(dead_code)]
pub(crate) fn parse_mobile_pairing_payload(
    input: &str,
) -> Result<MobilePairingPayload, SpineError> {
    parse_mobile_pairing_payload_at(input, Utc::now())
}

/// `now` is injected so unit tests can pin the clock when asserting expiry behavior.
#[allow(dead_code)]
pub(crate) fn parse_mobile_pairing_payload_at(
    input: &str,
    now: DateTime<Utc>,
) -> Result<MobilePairingPayload, SpineError> {
    if let Ok(payload) = serde_json::from_str::<MobilePairingPayload>(input) {
        return validate_payload(payload, now);
    }
    if input.starts_with("spine://pair/") {
        return parse_legacy_uri(input);
    }
    Err(SpineError::new(
        SpineErrorCode::BadRequest,
        "pairing payload is neither v1 JSON nor a spine://pair/ URI",
    ))
}

fn validate_payload(
    payload: MobilePairingPayload,
    now: DateTime<Utc>,
) -> Result<MobilePairingPayload, SpineError> {
    if payload.v != 1 {
        return Err(SpineError::new(
            SpineErrorCode::SchemaVersionUnsupported,
            format!(
                "unsupported pairing payload version {}; expected 1",
                payload.v
            ),
        ));
    }
    if payload.kind != PAIRING_PAYLOAD_KIND {
        return Err(SpineError::new(
            SpineErrorCode::BadRequest,
            format!(
                "unexpected kind '{}'; expected '{PAIRING_PAYLOAD_KIND}'",
                payload.kind
            ),
        ));
    }
    let expires_at = DateTime::parse_from_rfc3339(&payload.expires_at)
        .map_err(|e| {
            SpineError::new(
                SpineErrorCode::BadRequest,
                format!("expires_at is not RFC 3339: {e}"),
            )
        })?
        .with_timezone(&Utc);
    if now > expires_at + EXPIRY_SKEW {
        return Err(SpineError::new(
            SpineErrorCode::PairingExpired,
            "pairing payload expires_at is in the past",
        ));
    }
    Ok(payload)
}

fn parse_legacy_uri(input: &str) -> Result<MobilePairingPayload, SpineError> {
    let url = Url::parse(input).map_err(|_| {
        SpineError::new(
            SpineErrorCode::BadRequest,
            "legacy pairing URI failed to parse",
        )
    })?;
    if url.scheme() != "spine" {
        return Err(SpineError::new(
            SpineErrorCode::BadRequest,
            "legacy pairing URI scheme must be spine",
        ));
    }
    if url.host_str() != Some("pair") {
        return Err(SpineError::new(
            SpineErrorCode::BadRequest,
            "legacy pairing URI host must be 'pair'",
        ));
    }
    let session_id = url.path().trim_start_matches('/');
    if session_id.is_empty() {
        return Err(SpineError::new(
            SpineErrorCode::BadRequest,
            "legacy pairing URI missing session_id path segment",
        ));
    }
    let mut pk_value: Option<String> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "pk" => pk_value = Some(v.into_owned()),
            other => {
                return Err(SpineError::new(
                    SpineErrorCode::BadRequest,
                    format!("legacy pairing URI has unexpected query key '{other}'"),
                ));
            }
        }
    }
    let pairing_token = pk_value.ok_or_else(|| {
        SpineError::new(
            SpineErrorCode::BadRequest,
            "legacy pairing URI missing ?pk= query",
        )
    })?;

    // Future desktop-side scanning will populate the remaining fields from the local
    // SpineConfig. For unit-test reach-ability we synthesize placeholders that satisfy
    // the type contract without claiming to know the emitter's spine URL or fingerprint.
    let expires_at = (Utc::now() + chrono::Duration::minutes(5))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    Ok(MobilePairingPayload {
        v: 1,
        kind: PAIRING_PAYLOAD_KIND.to_string(),
        session_id: session_id.to_string(),
        spine_url: String::new(),
        ca_fingerprint: None,
        pairing_token: pairing_token.clone(),
        expires_at,
        device_a_pubkey: pairing_token,
        device_a_fingerprint: String::new(),
    })
}

/// Perform one round of polling. The caller is responsible for the cadence.
pub async fn poll_once(
    client: &SpineClient,
    identity: &Identity,
    session_id: &str,
) -> Result<PollOutcome, SpineError> {
    let status = client.pairing_status(session_id).await?;
    classify_status(&status, identity, session_id).await
}

async fn classify_status(
    status: &PairingStatusResponse,
    identity: &Identity,
    session_id: &str,
) -> Result<PollOutcome, SpineError> {
    match status.status.as_str() {
        "pending" => Ok(PollOutcome::Pending),
        "expired" | "cancelled" => Ok(PollOutcome::Expired),
        "completed" => {
            let responder_b64 = status.responder_pubkey.as_deref().ok_or_else(|| {
                SpineError::new(
                    SpineErrorCode::Internal,
                    "status=completed but server returned no responder_pubkey",
                )
            })?;
            completion_from_peer_pubkey(
                identity,
                session_id,
                responder_b64,
                status.paired_device_id.clone(),
                None, // poll path: no progress callback
            )
            .await
            .map(PollOutcome::Completed)
        }
        other => Err(SpineError::new(
            SpineErrorCode::Internal,
            format!("unknown pairing status: {other}"),
        )),
    }
}

async fn completion_from_peer_pubkey(
    identity: &Identity,
    session_id: &str,
    peer_pubkey_b64: &str,
    peer_device_id: Option<String>,
    on_progress: Option<&PairingProgressCallback>,
) -> Result<PairingCompletion, SpineError> {
    let peer_bytes = decode_pubkey(peer_pubkey_b64, "peer_pubkey")?;
    let peer_vk = VerifyingKey::from_bytes(&peer_bytes)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let peer_fp = identity::fingerprint_hex(&peer_bytes);

    if let Some(ref cb) = on_progress {
        cb("deriving_keys");
    }

    let sync_key =
        identity.with_signing_key(|sk| crypto::derive_sync_key(sk, &peer_vk, session_id));
    let sync_key_fp = hex::encode(crypto::sha256(&sync_key));

    if let Some(ref cb) = on_progress {
        cb("saving_keychain");
    }

    identity::store_sync_key(&peer_fp, &sync_key)?;
    info!(
        peer_fingerprint = %peer_fp,
        "pairing completed, sync_key derived and cached"
    );

    Ok(PairingCompletion {
        peer_fingerprint: peer_fp,
        peer_device_id,
        peer_pubkey_raw: peer_bytes,
        sync_key_fingerprint: sync_key_fp,
    })
}

fn decode_pubkey(encoded: &str, field: &str) -> Result<[u8; 32], SpineError> {
    let raw = B64URL
        .decode(encoded)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    if raw.len() != 32 {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            format!("{field} is not 32 bytes"),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

pub fn normalize_short_code(input: &str) -> Result<String, SpineError> {
    let digits = input.trim().replace('-', "");
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SpineError::new(
            SpineErrorCode::InvalidShortCode,
            "short code must contain 6 digits",
        ));
    }
    Ok(format!("{}-{}", &digits[..3], &digits[3..]))
}

/// Spawn a polling loop on the supplied tokio runtime. Returns a future that the caller
/// can `.await` (or push into a JoinSet) and an abort handle for cancellation.
///
/// The loop terminates on completion, expiry, the TTL elapsing, or `abort()` being called.
pub fn spawn_poller(
    client: Arc<SpineClient>,
    identity: Arc<Identity>,
    session_id: String,
) -> tokio::task::JoinHandle<Result<PollOutcome, SpineError>> {
    tokio::spawn(async move {
        loop {
            match poll_once(&client, &identity, &session_id).await {
                Ok(PollOutcome::Pending) => {
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
                other => return other,
            }
        }
    })
}

/// Render a QR encoding of `payload` to a base64-encoded PNG. The resulting `data:` URL can
/// be set as `<img src=...>` directly.
///
/// PRD 005 §US-052: payload size increased from ~50 chars (legacy URI) to ~350 chars (v1
/// JSON), so the ECC level is dropped from `M` (15% recovery, denser modules) to `L` (7%
/// recovery) to keep the QR scannable at the existing 320 px image dimension. Module size
/// is recalculated so the rendered side is always at least `QR_PNG_SIDE_PX`.
fn render_qr_png_base64(payload: &str) -> Result<String, SpineError> {
    let code = QrCode::with_error_correction_level(payload, qrcode::EcLevel::L)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("qr encode: {e}")))?;
    let modules = code.width() as u32;
    let scale = QR_PNG_SIDE_PX.div_ceil(modules).max(1);
    let side = modules * scale;
    let mut img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(side, side);
    let colors = code.to_colors();
    for y in 0..modules {
        for x in 0..modules {
            let dark = colors[(y * modules + x) as usize] == qrcode::Color::Dark;
            let pixel = if dark { 0u8 } else { 255u8 };
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel(x * scale + dx, y * scale + dy, Luma([pixel]));
                }
            }
        }
    }

    let mut png_bytes = Vec::new();
    image::write_buffer_with_format(
        &mut std::io::Cursor::new(&mut png_bytes),
        img.as_raw(),
        side,
        side,
        image::ColorType::L8,
        image::ImageFormat::Png,
    )
    .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("png encode: {e}")))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Tear down a pairing: revoke JWT (best-effort), wipe sync_key, clear paired_* in config.
#[allow(dead_code)] // Reserved for callers that want a direct unpair primitive without going via the Tauri command layer.
pub async fn unpair(
    client: &SpineClient,
    peer_fingerprint: &str,
    config: &mut syncmind_core::Config,
) -> Result<(), SpineError> {
    // Best-effort revoke; failure does not block the rest of unpair (PRD 004 §US-038).
    if let Err(e) = client.auth_revoke().await {
        warn!(error = %e, "auth_revoke failed during unpair (continuing)");
    }
    identity::wipe_sync_key(peer_fingerprint)?;
    config.spine.clear_pairing();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> MobilePairingPayload {
        MobilePairingPayload {
            v: 1,
            kind: PAIRING_PAYLOAD_KIND.to_string(),
            session_id: "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa".to_string(),
            spine_url: "https://spine.example.com:8443".to_string(),
            ca_fingerprint: None,
            pairing_token: "abc123def456".to_string(),
            expires_at: (Utc::now() + chrono::Duration::minutes(5))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            device_a_pubkey: B64URL.encode([0xAB; 32]),
            device_a_fingerprint: format!("sha256:{}", hex::encode(Sha256::digest([0xAB; 32]))),
        }
    }

    #[test]
    fn payload_roundtrip_json() {
        let p = sample_payload();
        let json = serde_json::to_string(&p).unwrap();
        let back: MobilePairingPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.v, p.v);
        assert_eq!(back.kind, p.kind);
        assert_eq!(back.session_id, p.session_id);
        assert_eq!(back.spine_url, p.spine_url);
        assert_eq!(back.ca_fingerprint, p.ca_fingerprint);
        assert_eq!(back.pairing_token, p.pairing_token);
        assert_eq!(back.expires_at, p.expires_at);
        assert_eq!(back.device_a_pubkey, p.device_a_pubkey);
        assert_eq!(back.device_a_fingerprint, p.device_a_fingerprint);
    }

    #[test]
    fn mobile_pairing_payload_carries_initiate_session_id() {
        let identity = Identity::from_parts(
            ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]),
            identity::DeviceMetadata {
                fingerprint: "sha256:test".to_string(),
                device_type: "desktop".to_string(),
                device_uuid: "11111111-1111-4111-8111-111111111111".to_string(),
                created_at: Utc::now().to_rfc3339(),
            },
        );
        let config = syncmind_core::SpineConfig {
            url: Some("https://spine.example.com:8443".to_string()),
            ..Default::default()
        };
        let response = crate::spine::client::InitiateResponse {
            session_id: "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa".to_string(),
            qr_payload: "spine://pair/aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa?pk=BBBB".to_string(),
            short_code: "123-456".to_string(),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
        };

        let payload = build_mobile_pairing_payload(&config, &identity, &response).unwrap();
        let json = serde_json::to_value(&payload).unwrap();

        assert_eq!(payload.session_id, response.session_id);
        assert_eq!(
            json["session_id"].as_str(),
            Some(response.session_id.as_str())
        );
    }

    #[test]
    fn parse_accepts_valid_v1_json() {
        let json = serde_json::to_string(&sample_payload()).unwrap();
        let parsed = parse_mobile_pairing_payload(&json).unwrap();
        assert_eq!(parsed.v, 1);
        assert_eq!(parsed.kind, PAIRING_PAYLOAD_KIND);
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let mut p = sample_payload();
        p.v = 2;
        let json = serde_json::to_string(&p).unwrap();
        let err = parse_mobile_pairing_payload(&json).unwrap_err();
        assert_eq!(err.code, "SCHEMA_VERSION_UNSUPPORTED");
        assert!(err.message.contains('2'));
    }

    #[test]
    fn parse_rejects_unknown_kind() {
        let mut p = sample_payload();
        p.kind = "syncmind-foo".into();
        let json = serde_json::to_string(&p).unwrap();
        let err = parse_mobile_pairing_payload(&json).unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[test]
    fn parse_accepts_legacy_uri() {
        let uri = "spine://pair/session-abc?pk=AAAA";
        let parsed = parse_mobile_pairing_payload(uri).unwrap();
        assert_eq!(parsed.pairing_token, "AAAA");
    }

    #[test]
    fn parse_rejects_uri_with_extra_query() {
        let uri = "spine://pair/session-abc?pk=AAAA&extra=1";
        let err = parse_mobile_pairing_payload(uri).unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
        assert!(err.message.contains("extra"));
    }

    #[test]
    fn parse_rejects_uri_without_pk() {
        let uri = "spine://pair/session-abc";
        let err = parse_mobile_pairing_payload(uri).unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[test]
    fn parse_rejects_uri_without_session() {
        let uri = "spine://pair/?pk=AAAA";
        let err = parse_mobile_pairing_payload(uri).unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[test]
    fn parse_rejects_unrelated_input() {
        let err = parse_mobile_pairing_payload("hello world").unwrap_err();
        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[test]
    fn parse_rejects_expired_payload() {
        let mut p = sample_payload();
        p.expires_at = (Utc::now() - chrono::Duration::seconds(120))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let json = serde_json::to_string(&p).unwrap();
        let err = parse_mobile_pairing_payload(&json).unwrap_err();
        assert_eq!(err.code, "PAIRING_EXPIRED");
    }

    #[test]
    fn parse_accepts_payload_within_clock_skew() {
        // 30 s in the past — inside the ±60 s tolerance, must still parse.
        let mut p = sample_payload();
        p.expires_at = (Utc::now() - chrono::Duration::seconds(30))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let json = serde_json::to_string(&p).unwrap();
        assert!(parse_mobile_pairing_payload(&json).is_ok());
    }

    #[test]
    fn fingerprint_matches_sha256_of_decoded_pubkey() {
        let p = sample_payload();
        let decoded = B64URL.decode(p.device_a_pubkey.as_bytes()).unwrap();
        let expected = format!("sha256:{}", hex::encode(Sha256::digest(&decoded)));
        assert_eq!(p.device_a_fingerprint, expected);
        assert!(p.device_a_fingerprint.starts_with("sha256:"));
        assert!(p
            .device_a_fingerprint
            .chars()
            .skip(7)
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn null_ca_fingerprint_serializes_as_json_null() {
        let p = sample_payload();
        assert!(p.ca_fingerprint.is_none());
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains("\"ca_fingerprint\":null"),
            "expected literal null in JSON, got: {json}"
        );
    }

    #[test]
    fn extract_pk_from_qr_payload_happy_path() {
        let pk = extract_pk_from_qr_payload("spine://pair/sess-1?pk=ZZZZ").unwrap();
        assert_eq!(pk, "ZZZZ");
    }

    #[test]
    fn extract_pk_from_qr_payload_rejects_missing_pk() {
        let err = extract_pk_from_qr_payload("spine://pair/sess-1").unwrap_err();
        assert_eq!(err.code, "INTERNAL_ERROR");
    }

    #[test]
    fn redact_token_trims_long_token() {
        assert_eq!(redact_token("ABCDEFGHIJ"), "ABCD…GHIJ");
    }

    #[test]
    fn redact_token_redacts_short_token() {
        assert_eq!(redact_token("ABC"), "<redacted>");
    }

    #[test]
    fn debug_impl_redacts_pairing_token() {
        let p = sample_payload();
        let dbg = format!("{:?}", p);
        assert!(
            !dbg.contains("abc123def456"),
            "raw token leaked in Debug output: {dbg}"
        );
    }

    #[test]
    fn render_qr_png_base64_produces_data_url() {
        let url = render_qr_png_base64("spine://pair/abc?pk=xyz").unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        // PNG magic after the data URL prefix.
        let b64 = url.trim_start_matches("data:image/png;base64,");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        assert!(
            bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
            "expected PNG magic"
        );
    }

    #[test]
    fn render_qr_png_handles_full_json_payload() {
        // ~300-char payload — must still encode without error at ECC Low.
        let p = sample_payload();
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.len() > 200,
            "sample payload unexpectedly short: {}",
            json.len()
        );
        let url = render_qr_png_base64(&json).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn normalize_short_code_accepts_hyphenated_and_digits_only() {
        assert_eq!(normalize_short_code("123-456").unwrap(), "123-456");
        assert_eq!(normalize_short_code("123456").unwrap(), "123-456");
        assert_eq!(normalize_short_code(" 123456 ").unwrap(), "123-456");
    }

    #[test]
    fn normalize_short_code_rejects_invalid_input() {
        assert_eq!(
            normalize_short_code("12345").unwrap_err().code,
            "INVALID_SHORT_CODE"
        );
        assert_eq!(
            normalize_short_code("abc-def").unwrap_err().code,
            "INVALID_SHORT_CODE"
        );
    }
}
