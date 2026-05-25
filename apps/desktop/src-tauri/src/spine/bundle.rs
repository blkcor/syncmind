//! Versioned plaintext envelope and AES-GCM seal/unseal.
//!
//! Wire format (server-visible bytes):
//!
//! ```text
//! bundle_blob = nonce (12 bytes) ‖ ciphertext_and_tag (N + 16 bytes)
//! ```
//!
//! Plaintext (inside the ciphertext) is a UTF-8 JSON envelope:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "kind": "note",
//!   "filename": "...",
//!   "content_utf8": "...",
//!   "source_path": "...",   // optional
//!   "captured_at": "RFC3339",
//!   "sha256": "lower-hex"
//! }
//! ```
//!
//! AAD = `SHA-256(peer_ed25519_pubkey_raw_32_bytes)` — 32 bytes. The sender uses the
//! receiver's public key; the receiver uses its own.
//!
//! See PRD 004 §US-033 / §US-034 and `specs/desktop-spine-client/spec.md`.

use serde::{Deserialize, Serialize};

use crate::spine::crypto;
use crate::spine::{SpineError, SpineErrorCode};

/// The only currently-supported schema_version. Bumping this is a protocol-level change.
pub const SCHEMA_VERSION_V1: u32 = 1;

/// Kind discriminant inside the envelope. v1 only supports `note`.
pub const KIND_NOTE: &str = "note";

/// Wire content-type sent in `X-Syncmind-Content-Type` for v1 note bundles.
pub const CONTENT_TYPE_NOTE: &str = "application/syncmind.note+json";

/// Plaintext bundle envelope. Serialized as JSON before encryption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEnvelope {
    pub schema_version: u32,
    pub kind: String,
    pub filename: String,
    pub content_utf8: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_path: Option<String>,
    /// RFC3339 UTC timestamp.
    pub captured_at: String,
    /// Lower-hex SHA-256 of `content_utf8.as_bytes()` (NOT of the whole envelope).
    pub sha256: String,
}

impl BundleEnvelope {
    /// Construct a v1 note envelope. `captured_at` is filled with `chrono::Utc::now()` and
    /// `sha256` with the SHA-256 of the content bytes.
    pub fn new_note(
        filename: impl Into<String>,
        content_utf8: impl Into<String>,
        source_path: Option<String>,
    ) -> Self {
        let content = content_utf8.into();
        let sha = hex::encode(crypto::sha256(content.as_bytes()));
        Self {
            schema_version: SCHEMA_VERSION_V1,
            kind: KIND_NOTE.to_string(),
            filename: filename.into(),
            content_utf8: content,
            source_path,
            captured_at: chrono::Utc::now().to_rfc3339(),
            sha256: sha,
        }
    }

    /// Verify schema_version, kind, and content-hash invariants. Returns Ok if the envelope
    /// is a valid v1 note whose `sha256` matches the content.
    pub fn validate(&self) -> Result<(), SpineError> {
        if self.schema_version != SCHEMA_VERSION_V1 {
            return Err(SpineError::new(
                SpineErrorCode::SchemaVersionUnsupported,
                format!("unsupported schema_version: {}", self.schema_version),
            ));
        }
        if self.kind != KIND_NOTE {
            return Err(SpineError::new(
                SpineErrorCode::SchemaVersionUnsupported,
                format!("unsupported kind: {}", self.kind),
            ));
        }
        let computed = hex::encode(crypto::sha256(self.content_utf8.as_bytes()));
        if computed != self.sha256 {
            return Err(SpineError::new(
                SpineErrorCode::EnvelopeIntegrityFailed,
                "content_utf8 SHA-256 does not match envelope.sha256",
            ));
        }
        Ok(())
    }
}

/// Compute the AAD for AES-GCM. The sender supplies the **receiver's** Ed25519 raw public key
/// (32 bytes); the receiver supplies its **own** raw public key.
pub fn aad_for_peer(peer_ed25519_pubkey_raw: &[u8; 32]) -> [u8; 32] {
    crypto::sha256(peer_ed25519_pubkey_raw)
}

/// Seal `envelope` into a `bundle_blob = nonce(12) ‖ ciphertext_and_tag`.
///
/// `sync_key` is the 32-byte HKDF output from pairing.
/// `peer_ed25519_pubkey_raw` is the receiver's Ed25519 public key (32 bytes).
pub fn encrypt(
    envelope: &BundleEnvelope,
    sync_key: &[u8; 32],
    peer_ed25519_pubkey_raw: &[u8; 32],
) -> Result<Vec<u8>, SpineError> {
    let plaintext = serde_json::to_vec(envelope)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let nonce = crypto::random_nonce_12();
    let aad = aad_for_peer(peer_ed25519_pubkey_raw);
    let ct_and_tag = crypto::aes_256_gcm_encrypt(sync_key, &nonce, &aad, &plaintext)?;

    let mut blob = Vec::with_capacity(nonce.len() + ct_and_tag.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct_and_tag);
    Ok(blob)
}

/// Unseal a `bundle_blob` produced by `encrypt`. Performs every integrity check listed in
/// the inbound-integrity requirement of the spec (AES-GCM tag, schema_version, content hash).
///
/// `local_ed25519_pubkey_raw` is the receiver's OWN Ed25519 public key (matches the sender's
/// AAD choice).
pub fn decrypt(
    bundle_blob: &[u8],
    sync_key: &[u8; 32],
    local_ed25519_pubkey_raw: &[u8; 32],
) -> Result<BundleEnvelope, SpineError> {
    if bundle_blob.len() < 12 + 16 {
        return Err(SpineError::new(
            SpineErrorCode::EnvelopeIntegrityFailed,
            "bundle_blob shorter than nonce+tag",
        ));
    }
    let (nonce_bytes, ct_and_tag) = bundle_blob.split_at(12);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(nonce_bytes);
    let aad = aad_for_peer(local_ed25519_pubkey_raw);

    let plaintext = crypto::aes_256_gcm_decrypt(sync_key, &nonce, &aad, ct_and_tag)?;
    let envelope: BundleEnvelope = serde_json::from_slice(&plaintext).map_err(|e| {
        SpineError::new(
            SpineErrorCode::EnvelopeIntegrityFailed,
            format!("envelope JSON parse failed: {e}"),
        )
    })?;
    envelope.validate()?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_pubkey(seed: u8) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[0] = seed;
        k
    }

    #[test]
    fn new_note_fills_sha256_and_timestamp() {
        let env = BundleEnvelope::new_note("a.md", "hello", None);
        assert_eq!(env.schema_version, SCHEMA_VERSION_V1);
        assert_eq!(env.kind, KIND_NOTE);
        assert_eq!(env.content_utf8, "hello");
        assert!(!env.captured_at.is_empty());
        assert_eq!(env.sha256, hex::encode(crypto::sha256(b"hello")));
        assert!(env.validate().is_ok());
    }

    #[test]
    fn encrypt_decrypt_roundtrip_between_two_peers() {
        let sync_key = [0x9Au8; 32];
        let sender_local_pubkey = fixed_pubkey(0xAA);
        let receiver_local_pubkey = fixed_pubkey(0xBB);

        let envelope = BundleEnvelope::new_note("hello.md", "the quick brown fox", None);

        // Sender encrypts using the RECEIVER's public key as AAD-key.
        let blob = encrypt(&envelope, &sync_key, &receiver_local_pubkey).unwrap();
        assert!(blob.len() > 12 + 16);

        // Receiver decrypts using its OWN public key as AAD-key.
        let decoded = decrypt(&blob, &sync_key, &receiver_local_pubkey).unwrap();
        assert_eq!(decoded, envelope);

        // Wrong AAD (e.g. sender's own pubkey) MUST fail.
        let err = decrypt(&blob, &sync_key, &sender_local_pubkey).unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[test]
    fn decrypt_rejects_short_blob() {
        let err = decrypt(&[1, 2, 3], &[0u8; 32], &[0u8; 32]).unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[test]
    fn validate_rejects_unsupported_schema_version() {
        let mut env = BundleEnvelope::new_note("a.md", "x", None);
        env.schema_version = 2;
        let err = env.validate().unwrap_err();
        assert_eq!(err.code, "SCHEMA_VERSION_UNSUPPORTED");
    }

    #[test]
    fn validate_rejects_content_hash_mismatch() {
        let mut env = BundleEnvelope::new_note("a.md", "x", None);
        env.content_utf8 = "tampered".to_string();
        let err = env.validate().unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[test]
    fn tag_tamper_round_trip_fails_with_integrity_error() {
        let sync_key = [0x9Au8; 32];
        let receiver_local_pubkey = fixed_pubkey(0xBB);
        let envelope = BundleEnvelope::new_note("a.md", "hi", None);
        let mut blob = encrypt(&envelope, &sync_key, &receiver_local_pubkey).unwrap();
        // Flip a bit in the last byte (within GCM tag).
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        let err = decrypt(&blob, &sync_key, &receiver_local_pubkey).unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }
}
