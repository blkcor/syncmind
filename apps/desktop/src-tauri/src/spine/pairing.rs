//! Pairing flow: initiate, poll for completion, derive sync_key.
//!
//! PRD 004 §US-030 / §US-031. The desktop acts as the initiator — it displays a QR PNG
//! plus a 6-digit short code and polls `GET /v1/pairing/:session_id/status` every second
//! until the responder completes the session or the TTL elapses.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use image::{ImageBuffer, Luma};
use qrcode::QrCode;
use tracing::{info, warn};

use crate::spine::client::{PairingStatusResponse, SpineClient};
use crate::spine::crypto;
use crate::spine::identity::{self, Identity};
use crate::spine::{SpineError, SpineErrorCode};

/// 1-second polling interval for the pairing status loop (PRD 004 §US-030).
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// QR PNG side length in pixels.
const QR_PNG_SIDE_PX: u32 = 320;

/// Returned to the frontend so it can render a QR + short code countdown.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PairingHandleView {
    pub session_id: String,
    pub short_code: String,
    pub qr_png_base64: String,
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
pub async fn initiate(
    client: &SpineClient,
    identity: &Identity,
) -> Result<(PairingHandleView, String), SpineError> {
    let resp = client.pairing_initiate(identity.device_type()).await?;
    let qr_png_base64 = render_qr_png_base64(&resp.qr_payload)?;
    let view = PairingHandleView {
        session_id: resp.session_id.clone(),
        short_code: resp.short_code,
        qr_png_base64,
        expires_at: resp.expires_at,
    };
    Ok((view, resp.session_id))
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
fn render_qr_png_base64(payload: &str) -> Result<String, SpineError> {
    let code = QrCode::with_error_correction_level(payload, qrcode::EcLevel::M)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, format!("qr encode: {e}")))?;
    let modules = code.width() as u32;
    let scale = (QR_PNG_SIDE_PX / modules).max(1);
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
