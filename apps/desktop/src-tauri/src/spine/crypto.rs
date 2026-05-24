//! Cryptographic primitives used by the spine client.
//!
//! All algorithms in this module are normative — they MUST produce identical bytes on every
//! client implementation so that paired devices interoperate. See PRD 003 §Impl Note 1.1 and
//! 1.2 (the dalek conversion contract and client-supplied UUIDs) and PRD 004 §US-023, §US-025,
//! §US-026.
//!
//! No function here performs I/O. Functions are deterministic except those that explicitly
//! source randomness (`random_nonce`, `mint_jwt` via the `jti` UUID generator).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::spine::{SpineError, SpineErrorCode};

/// HKDF-SHA256 with the documented (`ikm`, `salt`, `info`) → 32-byte output schedule
/// used to derive `sync_key` from the shared X25519 secret.
pub fn hkdf_sha256_32(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out)
        .expect("HKDF-SHA256 expand of 32 bytes never fails");
    out
}

/// SHA-256 of the input, returning the 32-byte digest.
pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().into()
}

/// Generate a 12-byte AES-GCM nonce from the OS CSPRNG.
pub fn random_nonce_12() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Encrypt `plaintext` with AES-256-GCM, producing `ciphertext || tag`.
///
/// `aad` is bound into the GCM tag — paired sender and receiver must agree on it. See
/// PRD 004 §US-025: AAD is `SHA-256(peer_ed25519_pubkey_raw_32_bytes)`.
pub fn aes_256_gcm_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, SpineError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| SpineError::new(SpineErrorCode::Internal, "AES-256-GCM encryption failed"))
}

/// Decrypt `ciphertext_and_tag` with AES-256-GCM. Returns the plaintext on success; on any
/// failure (tag mismatch, AAD mismatch, key mismatch) returns
/// `EnvelopeIntegrityFailed`.
pub fn aes_256_gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext_and_tag: &[u8],
) -> Result<Vec<u8>, SpineError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext_and_tag,
                aad,
            },
        )
        .map_err(|_| {
            SpineError::new(
                SpineErrorCode::EnvelopeIntegrityFailed,
                "AES-256-GCM decryption failed (tag, AAD, or key mismatch)",
            )
        })
}

/// Convert an Ed25519 signing key to its Curve25519 (X25519) scalar form.
///
/// Uses [`SigningKey::to_scalar_bytes`], which returns `clamp(SHA-512(seed)[..32])` — the
/// standard derivation defined by RFC 8032 §5.1.5 and used by libsodium's
/// `crypto_sign_ed25519_sk_to_curve25519`.
pub fn ed25519_signing_key_to_x25519_scalar(sk: &SigningKey) -> [u8; 32] {
    sk.to_scalar_bytes()
}

/// Convert an Ed25519 verifying (public) key to its Curve25519 (X25519) Montgomery form.
///
/// Wraps [`VerifyingKey::to_montgomery`] for symmetry with the private-side conversion.
pub fn ed25519_verifying_key_to_x25519_pubkey(vk: &VerifyingKey) -> [u8; 32] {
    vk.to_montgomery().to_bytes()
}

/// Derive the 32-byte `sync_key` shared between two paired devices.
///
/// Given the local Ed25519 signing key, the peer's Ed25519 verifying key, and the pairing
/// session ID (as a UTF-8 string), returns `HKDF-SHA256(x25519(...), session_id, "syncmind-v1")`.
/// This is the canonical derivation specified in PRD 003 §Impl Note 1.1.
pub fn derive_sync_key(
    local_sk: &SigningKey,
    peer_vk: &VerifyingKey,
    session_id: &str,
) -> [u8; 32] {
    let local_scalar = ed25519_signing_key_to_x25519_scalar(local_sk);
    let peer_point = ed25519_verifying_key_to_x25519_pubkey(peer_vk);
    let shared = x25519_dalek::x25519(local_scalar, peer_point);
    hkdf_sha256_32(&shared, session_id.as_bytes(), b"syncmind-v1")
}

/// Sign `msg` with the supplied signing key (thin wrapper for callers that hold the key
/// behind a wrapper type and don't want to expose `Signer`).
pub fn sign(sk: &SigningKey, msg: &[u8]) -> Signature {
    sk.sign(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn fixed_signing_key(seed: u8) -> SigningKey {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        SigningKey::from_bytes(&bytes)
    }

    #[test]
    fn hkdf_golden_vector_rfc5869_case_1_truncated() {
        // RFC 5869 Test Case 1, truncated to our 32-byte output.
        // IKM: 0b * 22, salt: 0x000102...0c (13 bytes), info: 0xf0f1f2f3f4f5f6f7f8f9
        let ikm = [0x0bu8; 22];
        let salt = (0u8..=12).collect::<Vec<u8>>();
        let info: [u8; 10] = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9];
        let okm = hkdf_sha256_32(&ikm, &salt, &info);
        let expected = hex::decode(
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf",
        )
        .unwrap();
        assert_eq!(okm.to_vec(), expected);
    }

    #[test]
    fn aes_gcm_roundtrip() {
        let key = [0x42u8; 32];
        let nonce = [0x01u8; 12];
        let aad = b"peer-fingerprint-32-bytes-aad-here";
        let plaintext = b"hello world";

        let ct = aes_256_gcm_encrypt(&key, &nonce, aad, plaintext).unwrap();
        let pt = aes_256_gcm_decrypt(&key, &nonce, aad, &ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_aad_mismatch_fails() {
        let key = [0x42u8; 32];
        let nonce = [0x01u8; 12];
        let ct = aes_256_gcm_encrypt(&key, &nonce, b"aad-a", b"hi").unwrap();
        let err = aes_256_gcm_decrypt(&key, &nonce, b"aad-b", &ct).unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[test]
    fn aes_gcm_tag_tamper_fails() {
        let key = [0x42u8; 32];
        let nonce = [0x01u8; 12];
        let mut ct = aes_256_gcm_encrypt(&key, &nonce, b"aad", b"hi").unwrap();
        // Flip the last byte (part of the GCM tag).
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        let err = aes_256_gcm_decrypt(&key, &nonce, b"aad", &ct).unwrap_err();
        assert_eq!(err.code, "ENVELOPE_INTEGRITY_FAILED");
    }

    #[test]
    fn sync_key_derivation_is_symmetric() {
        // A and B independently derive the same sync_key from each other's keys.
        let sk_a = fixed_signing_key(0xA1);
        let sk_b = fixed_signing_key(0xB2);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();

        let key_from_a = derive_sync_key(&sk_a, &vk_b, "session-uuid-xyz");
        let key_from_b = derive_sync_key(&sk_b, &vk_a, "session-uuid-xyz");
        assert_eq!(key_from_a, key_from_b);

        // Same inputs → deterministic; different session_id → different key.
        let key_other_session = derive_sync_key(&sk_a, &vk_b, "different-session");
        assert_ne!(key_from_a, key_other_session);
    }

    #[test]
    fn random_nonce_is_nondegenerate() {
        let a = random_nonce_12();
        let b = random_nonce_12();
        // Probabilistically distinct.
        assert_ne!(a, b);
    }

    #[test]
    fn sha256_known_vector() {
        // SHA-256("abc") known answer.
        let got = sha256(b"abc");
        let expected =
            hex::decode("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap();
        assert_eq!(got.to_vec(), expected);
    }
}
