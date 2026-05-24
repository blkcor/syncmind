//! Spine client — desktop side of the Phase 3 sync protocol.
//!
//! Implements the behaviors specified in PRD 004 (`docs/prd/004-desktop-spine-client.md`)
//! and the OpenSpec change `openspec/changes/desktop-spine-client/`. The submodules form
//! a layered stack:
//!
//! - [`identity`] — Ed25519 device identity stored in the OS keychain.
//! - [`crypto`]   — HKDF, AES-GCM, Ed25519↔X25519, EdDSA JWT primitives.
//! - [`bundle`]   — Versioned plaintext envelope, encrypt/decrypt with peer-fingerprint AAD.
//! - `client`     — HTTPS client (TODO).
//! - `pairing`    — Pairing initiate/poll + sync_key derivation (TODO).
//! - `ws`         — WebSocket notifications + polling fallback (TODO).
//! - `inbox`      — sync-inbox materialization (TODO).
//! - `commands`   — Tauri command wrappers (TODO).
//!
//! All secrets stay inside the Rust backend; nothing under `apps/desktop/src/` ever
//! receives raw key material via IPC. See `FR-21` in PRD 004.

pub mod bundle;
pub mod client;
pub mod commands;
pub mod crypto;
pub mod identity;
pub mod inbox;
pub mod pairing;
pub mod state;

use std::fmt;

/// String error codes shared between the spine subsystem and the Solid frontend.
///
/// Per PRD 004 §FR-27, codes must align with the server's string conventions
/// (`AUTH_INVALID`, `RATE_LIMITED`, …) and add client-only codes for situations
/// that never reach the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpineErrorCode {
    SpineNotConfigured,
    SpineUnreachable,
    AlreadyPaired,
    NotPaired,
    EmptyNote,
    BundleTooLarge,
    KeychainUnavailable,
    KeychainFingerprintMismatch,
    InvalidUrl,
    TrustCaNotReadable,
    TrustCaInvalidPem,
    PairingExpired,
    AuthInvalid,
    SchemaVersionUnsupported,
    EnvelopeIntegrityFailed,
    Internal,
}

impl SpineErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpineErrorCode::SpineNotConfigured => "SPINE_NOT_CONFIGURED",
            SpineErrorCode::SpineUnreachable => "SPINE_UNREACHABLE",
            SpineErrorCode::AlreadyPaired => "ALREADY_PAIRED",
            SpineErrorCode::NotPaired => "NOT_PAIRED",
            SpineErrorCode::EmptyNote => "EMPTY_NOTE",
            SpineErrorCode::BundleTooLarge => "BUNDLE_TOO_LARGE",
            SpineErrorCode::KeychainUnavailable => "KEYCHAIN_UNAVAILABLE",
            SpineErrorCode::KeychainFingerprintMismatch => "KEYCHAIN_FINGERPRINT_MISMATCH",
            SpineErrorCode::InvalidUrl => "INVALID_URL",
            SpineErrorCode::TrustCaNotReadable => "TRUST_CA_NOT_READABLE",
            SpineErrorCode::TrustCaInvalidPem => "TRUST_CA_INVALID_PEM",
            SpineErrorCode::PairingExpired => "PAIRING_EXPIRED",
            SpineErrorCode::AuthInvalid => "AUTH_INVALID",
            SpineErrorCode::SchemaVersionUnsupported => "SCHEMA_VERSION_UNSUPPORTED",
            SpineErrorCode::EnvelopeIntegrityFailed => "ENVELOPE_INTEGRITY_FAILED",
            SpineErrorCode::Internal => "INTERNAL_ERROR",
        }
    }
}

impl fmt::Display for SpineErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Public-facing error returned by every spine API call. Serializable so it can flow back
/// through Tauri commands; carries only a code + message, never key material.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpineError {
    pub code: String,
    pub message: String,
}

impl SpineError {
    pub fn new(code: SpineErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SpineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SpineError {}

impl From<SpineError> for String {
    fn from(e: SpineError) -> Self {
        e.to_string()
    }
}

/// Convert an `anyhow::Error` into a generic internal SpineError. Use sparingly — prefer
/// constructing a specific error variant at the call site so the frontend can map it.
impl From<anyhow::Error> for SpineError {
    fn from(e: anyhow::Error) -> Self {
        SpineError::new(SpineErrorCode::Internal, e.to_string())
    }
}
