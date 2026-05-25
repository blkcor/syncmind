//! Ed25519 device identity for the spine client.
//!
//! Implements PRD 004 §US-029. Responsibilities:
//!
//! - Generate or load an Ed25519 signing key at first launch.
//! - Persist the private key in the OS keychain (`service="syncmind"`, `account="device-identity"`).
//!   Fall back to 0600 files under `<data-dir>/keys/` on Linux when libsecret is unavailable,
//!   and in macOS dev builds where repeated Tauri rebuilds invalidate Keychain ACL trust.
//! - Mint a UUIDv4 used as the device's `sub` claim in every JWT (and as `devices.id` on the
//!   Spine, per PRD 002 §Impl Note 1.2).
//! - Cache the public fingerprint + UUID in `<data-dir>/device.json` (NEVER includes the
//!   private key) so other modules can read it without reaching into the keychain.
//! - Provide `sign(msg)` and key-export helpers but NEVER expose `SigningKey` itself across
//!   any API boundary that might surface in IPC.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::spine::crypto;
use crate::spine::{SpineError, SpineErrorCode};

const KEYCHAIN_SERVICE: &str = "syncmind";
const KEYCHAIN_ACCOUNT_IDENTITY: &str = "device-identity";
const KEYCHAIN_SYNC_KEY_ACCOUNT_PREFIX: &str = "sync-key:";
const DEVICE_JSON: &str = "device.json";
const LINUX_FALLBACK_DIR: &str = "keys";
const LINUX_FALLBACK_FILE: &str = "device.ed25519";
const FALLBACK_SYNC_KEY_PREFIX: &str = "sync-key-";
const FALLBACK_SYNC_KEY_SUFFIX: &str = ".b64";

/// Public metadata cached in `<data-dir>/device.json`. Contains nothing secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceMetadata {
    pub fingerprint: String,
    pub device_type: String,
    pub device_uuid: String,
    pub created_at: String,
}

/// A loaded device identity. The signing key is kept private to this module so that no
/// downstream caller can accidentally serialize it into an IPC response (FR-21).
pub struct Identity {
    signing_key: SigningKey,
    metadata: DeviceMetadata,
}

impl Identity {
    /// Wrap an existing signing key + metadata. Tests use this; in production prefer
    /// `ensure(...)`.
    pub fn from_parts(signing_key: SigningKey, metadata: DeviceMetadata) -> Self {
        Self {
            signing_key,
            metadata,
        }
    }

    pub fn fingerprint(&self) -> &str {
        &self.metadata.fingerprint
    }

    pub fn device_uuid(&self) -> &str {
        &self.metadata.device_uuid
    }

    pub fn device_type(&self) -> &str {
        &self.metadata.device_type
    }

    pub fn metadata(&self) -> &DeviceMetadata {
        &self.metadata
    }

    /// The Ed25519 public key (raw 32 bytes). Safe to share publicly.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Sign `msg` with the device's Ed25519 private key.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        self.signing_key.sign(msg)
    }

    /// Lend the underlying [`SigningKey`] to a callback. Use this for operations that
    /// genuinely need the key (sync_key derivation, JWT signing) without exposing it
    /// permanently.
    pub fn with_signing_key<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&SigningKey) -> R,
    {
        f(&self.signing_key)
    }
}

/// Resolved storage backend for an Ed25519 device key.
#[derive(Debug, Clone, Copy)]
pub enum KeyStorage {
    /// macOS Keychain / Windows Credential Manager / libsecret-backed Linux daemon.
    Keychain,
    /// `<data-dir>/keys/device.ed25519` — only used as a Linux fallback.
    FilesystemFallback,
}

/// Locate or create the device identity. On first run this generates a fresh Ed25519
/// keypair, a fresh UUIDv4, and writes both halves of the world view atomically.
///
/// `data_dir` should be `syncmind_core::paths::local_data_dir()?` in production.
pub fn ensure_identity(data_dir: &Path, device_type: &str) -> Result<Identity, SpineError> {
    let device_json = data_dir.join(DEVICE_JSON);

    let (backend, stored) = load_stored_identity(data_dir)?;

    if let Some(signing_key) = stored {
        let computed_fp = fingerprint_hex(&signing_key.verifying_key().to_bytes());
        let mut metadata = read_metadata(&device_json).unwrap_or_else(|_| DeviceMetadata {
            fingerprint: computed_fp.clone(),
            device_type: device_type.to_string(),
            device_uuid: Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        });

        if metadata.fingerprint != computed_fp {
            return Err(SpineError::new(
                SpineErrorCode::KeychainFingerprintMismatch,
                format!(
                    "device.json fingerprint {} does not match the key loaded from {:?}",
                    &metadata.fingerprint[..16.min(metadata.fingerprint.len())],
                    backend
                ),
            ));
        }
        // device.json may have been deleted while the key persists; rewrite it.
        if !device_json.exists() {
            metadata.fingerprint = computed_fp;
            write_metadata(&device_json, &metadata)?;
        }
        return Ok(Identity::from_parts(signing_key, metadata));
    }

    // Brand-new install: mint everything.
    let signing_key = SigningKey::generate(&mut OsRng);
    let fp = fingerprint_hex(&signing_key.verifying_key().to_bytes());

    match backend {
        KeyStorage::Keychain => persist_to_keychain(&signing_key)?,
        KeyStorage::FilesystemFallback => persist_to_fallback(data_dir, &signing_key)?,
    }

    let metadata = DeviceMetadata {
        fingerprint: fp.clone(),
        device_type: device_type.to_string(),
        device_uuid: Uuid::new_v4().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    write_metadata(&device_json, &metadata)?;
    info!(
        fingerprint = %fp,
        device_uuid = %metadata.device_uuid,
        backend = ?backend,
        "minted new device identity"
    );

    Ok(Identity::from_parts(signing_key, metadata))
}

/// Wipe the device identity entirely. Used by `spine_reset_identity` (PRD 004 §US-038).
/// Best-effort: failures on individual backends are logged but not propagated unless every
/// destructive step failed.
pub fn reset_identity(data_dir: &Path) -> Result<(), SpineError> {
    let mut any_succeeded = false;

    if !prefer_filesystem_secret_store() {
        match keyring_entry(KEYCHAIN_ACCOUNT_IDENTITY) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) => any_succeeded = true,
                Err(keyring::Error::NoEntry) => any_succeeded = true,
                Err(e) => warn!(error = %e, "failed to delete keychain identity entry"),
            },
            Err(e) => warn!(error = %e, "keyring unavailable while resetting identity"),
        }
    }

    let fallback = fallback_path(data_dir);
    if fallback.exists() {
        match fs::remove_file(&fallback) {
            Ok(()) => any_succeeded = true,
            Err(e) => {
                warn!(error = %e, path = %fallback.display(), "failed to remove fallback key file")
            }
        }
    } else {
        any_succeeded = true;
    }

    let dj = data_dir.join(DEVICE_JSON);
    if dj.exists() {
        match fs::remove_file(&dj) {
            Ok(()) => any_succeeded = true,
            Err(e) => warn!(error = %e, path = %dj.display(), "failed to remove device.json"),
        }
    } else {
        any_succeeded = true;
    }

    if any_succeeded {
        Ok(())
    } else {
        Err(SpineError::new(
            SpineErrorCode::Internal,
            "failed to clear any identity storage backend",
        ))
    }
}

/// Cache a freshly-derived sync_key per peer fingerprint. Used by the pairing flow.
pub fn store_sync_key(peer_fingerprint: &str, key: &[u8; 32]) -> Result<(), SpineError> {
    if prefer_filesystem_secret_store() {
        return persist_sync_key_to_fallback(peer_fingerprint, key);
    }

    let account = format!("{KEYCHAIN_SYNC_KEY_ACCOUNT_PREFIX}{peer_fingerprint}");
    let entry = keyring_entry(&account)?;
    match entry.set_password(&B64.encode(key)) {
        Ok(()) => Ok(()),
        Err(e) => {
            warn!(
                error = %e,
                "keyring unavailable while storing sync_key; falling back to filesystem storage"
            );
            persist_sync_key_to_fallback(peer_fingerprint, key)
        }
    }
}

/// Load a previously cached sync_key. Returns `Ok(None)` if no entry exists.
pub fn load_sync_key(peer_fingerprint: &str) -> Result<Option<[u8; 32]>, SpineError> {
    if prefer_filesystem_secret_store() {
        if let Some(sync_key) = try_load_sync_key_from_fallback(peer_fingerprint)? {
            return Ok(Some(sync_key));
        }
        if migrate_from_keychain() {
            if let Some(sync_key) = try_load_sync_key_from_keychain(peer_fingerprint)? {
                persist_sync_key_to_fallback(peer_fingerprint, &sync_key)?;
                info!(
                    peer_fingerprint = %peer_fingerprint,
                    "migrated sync_key from keychain to filesystem secret store"
                );
                return Ok(Some(sync_key));
            }
        }
        return Ok(None);
    }

    let account = format!("{KEYCHAIN_SYNC_KEY_ACCOUNT_PREFIX}{peer_fingerprint}");
    let entry = keyring_entry(&account)?;
    match entry.get_password() {
        Ok(b64) => decode_sync_key_b64(&b64).map(Some),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            warn!(
                error = %e,
                "keyring unavailable while loading sync_key; falling back to filesystem storage"
            );
            try_load_sync_key_from_fallback(peer_fingerprint)
        }
    }
}

/// Wipe every cached sync_key associated with this device. Called by `spine_unpair`.
///
/// Keychain APIs don't expose an enumerate-by-prefix operation, so callers must pass the
/// peer fingerprint explicitly. For a "wipe-all" semantic at app level, supply the last
/// known peer fingerprint from `Config.spine.paired_peer_fingerprint`.
pub fn wipe_sync_key(peer_fingerprint: &str) -> Result<(), SpineError> {
    if prefer_filesystem_secret_store() {
        return wipe_sync_key_from_fallback(peer_fingerprint);
    }

    let account = format!("{KEYCHAIN_SYNC_KEY_ACCOUNT_PREFIX}{peer_fingerprint}");
    match keyring_entry(&account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            let _ = wipe_sync_key_from_fallback(peer_fingerprint);
            Ok(())
        }
        Err(e) => {
            warn!(
                error = %e,
                "keyring unavailable while wiping sync_key; falling back to filesystem storage"
            );
            wipe_sync_key_from_fallback(peer_fingerprint)
        }
    }
}

/// Compute the SHA-256 hex fingerprint of an Ed25519 public key (64 chars, lower hex).
pub fn fingerprint_hex(pubkey: &[u8]) -> String {
    hex::encode(crypto::sha256(pubkey))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn keyring_entry(account: &str) -> Result<keyring::Entry, SpineError> {
    keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(keyring_to_spine_err)
}

fn keyring_to_spine_err(e: keyring::Error) -> SpineError {
    SpineError::new(SpineErrorCode::KeychainUnavailable, e.to_string())
}

fn load_stored_identity(data_dir: &Path) -> Result<(KeyStorage, Option<SigningKey>), SpineError> {
    if prefer_filesystem_secret_store() {
        debug!("using filesystem secret store for spine identity");
        if let Some(signing_key) = try_load_from_fallback(data_dir)? {
            return Ok((KeyStorage::FilesystemFallback, Some(signing_key)));
        }
        if migrate_from_keychain() {
            if let Some(signing_key) = try_load_from_keychain()? {
                persist_to_fallback(data_dir, &signing_key)?;
                info!("migrated spine identity from keychain to filesystem secret store");
                return Ok((KeyStorage::FilesystemFallback, Some(signing_key)));
            }
        }
        return Ok((KeyStorage::FilesystemFallback, None));
    }

    match try_load_from_keychain() {
        Ok(opt) => Ok((KeyStorage::Keychain, opt)),
        Err(e) => {
            warn!(
                error = %e,
                "keyring unavailable; falling back to filesystem storage. ensure the data dir is on encrypted storage."
            );
            Ok((
                KeyStorage::FilesystemFallback,
                try_load_from_fallback(data_dir)?,
            ))
        }
    }
}

fn prefer_filesystem_secret_store() -> bool {
    std::env::var_os("SYNCMIND_DISABLE_KEYCHAIN").is_some()
        || cfg!(all(target_os = "macos", debug_assertions))
}

fn migrate_from_keychain() -> bool {
    std::env::var_os("SYNCMIND_MIGRATE_KEYCHAIN").is_some()
}

fn try_load_from_keychain() -> Result<Option<SigningKey>, SpineError> {
    let entry = keyring_entry(KEYCHAIN_ACCOUNT_IDENTITY)?;
    match entry.get_password() {
        Ok(b64) => Ok(Some(decode_signing_key_b64(&b64)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keyring_to_spine_err(e)),
    }
}

fn try_load_sync_key_from_keychain(peer_fingerprint: &str) -> Result<Option<[u8; 32]>, SpineError> {
    let account = format!("{KEYCHAIN_SYNC_KEY_ACCOUNT_PREFIX}{peer_fingerprint}");
    let entry = keyring_entry(&account)?;
    match entry.get_password() {
        Ok(b64) => decode_sync_key_b64(&b64).map(Some),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keyring_to_spine_err(e)),
    }
}

fn persist_to_keychain(sk: &SigningKey) -> Result<(), SpineError> {
    let entry = keyring_entry(KEYCHAIN_ACCOUNT_IDENTITY)?;
    let pkcs8 = sk
        .to_pkcs8_der()
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let bytes = Zeroizing::new(pkcs8.as_bytes().to_vec());
    entry
        .set_password(&B64.encode(&*bytes))
        .map_err(keyring_to_spine_err)
}

fn try_load_from_fallback(data_dir: &Path) -> Result<Option<SigningKey>, SpineError> {
    let path = fallback_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(&path)
        .with_context(|| format!("read fallback key file {}", path.display()))
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let b64 = String::from_utf8(raw)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    Ok(Some(decode_signing_key_b64(b64.trim())?))
}

fn persist_to_fallback(data_dir: &Path, sk: &SigningKey) -> Result<(), SpineError> {
    let dir = data_dir.join(LINUX_FALLBACK_DIR);
    fs::create_dir_all(&dir)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    set_dir_permissions_0700(&dir)?;

    let path = dir.join(LINUX_FALLBACK_FILE);
    let pkcs8 = sk
        .to_pkcs8_der()
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let bytes = Zeroizing::new(B64.encode(pkcs8.as_bytes()));
    fs::write(&path, bytes.as_bytes())
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    set_file_permissions_0600(&path)?;
    Ok(())
}

fn fallback_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LINUX_FALLBACK_DIR).join(LINUX_FALLBACK_FILE)
}

fn fallback_dir() -> Result<PathBuf, SpineError> {
    let data_dir = syncmind_core::paths::local_data_dir()
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    let dir = data_dir.join(LINUX_FALLBACK_DIR);
    fs::create_dir_all(&dir)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    set_dir_permissions_0700(&dir)?;
    Ok(dir)
}

fn fallback_sync_key_path(peer_fingerprint: &str) -> Result<PathBuf, SpineError> {
    if peer_fingerprint.len() != 64 || !peer_fingerprint.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            "invalid peer fingerprint for sync_key storage",
        ));
    }
    Ok(fallback_dir()?.join(format!(
        "{FALLBACK_SYNC_KEY_PREFIX}{peer_fingerprint}{FALLBACK_SYNC_KEY_SUFFIX}"
    )))
}

fn persist_sync_key_to_fallback(peer_fingerprint: &str, key: &[u8; 32]) -> Result<(), SpineError> {
    let path = fallback_sync_key_path(peer_fingerprint)?;
    fs::write(&path, B64.encode(key))
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    set_file_permissions_0600(&path)
}

fn try_load_sync_key_from_fallback(peer_fingerprint: &str) -> Result<Option<[u8; 32]>, SpineError> {
    let path = fallback_sync_key_path(peer_fingerprint)?;
    if !path.exists() {
        return Ok(None);
    }
    let b64 = fs::read_to_string(&path)
        .with_context(|| format!("read fallback sync_key file {}", path.display()))
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    decode_sync_key_b64(b64.trim()).map(Some)
}

fn wipe_sync_key_from_fallback(peer_fingerprint: &str) -> Result<(), SpineError> {
    let path = fallback_sync_key_path(peer_fingerprint)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SpineError::new(SpineErrorCode::Internal, e.to_string())),
    }
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

#[cfg(unix)]
fn set_file_permissions_0600(p: &Path) -> Result<(), SpineError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(p)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?
        .permissions();
    perms.set_mode(0o600);
    fs::set_permissions(p, perms)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

#[cfg(not(unix))]
fn set_dir_permissions_0700(_p: &Path) -> Result<(), SpineError> {
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions_0600(_p: &Path) -> Result<(), SpineError> {
    Ok(())
}

fn decode_signing_key_b64(b64: &str) -> Result<SigningKey, SpineError> {
    let bytes = Zeroizing::new(
        B64.decode(b64)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?,
    );
    SigningKey::from_pkcs8_der(&bytes)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

fn decode_sync_key_b64(b64: &str) -> Result<[u8; 32], SpineError> {
    let bytes = B64
        .decode(b64)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    if bytes.len() != 32 {
        return Err(SpineError::new(
            SpineErrorCode::Internal,
            format!("cached sync_key has wrong length: {}", bytes.len()),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn read_metadata(path: &Path) -> Result<DeviceMetadata, SpineError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

fn write_metadata(path: &Path, m: &DeviceMetadata) -> Result<(), SpineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    }
    let raw = serde_json::to_string_pretty(m)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    // Atomic-ish write: tmp + rename.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fingerprint_hex_matches_sha256_of_pubkey() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = sk.verifying_key().to_bytes();
        let fp = fingerprint_hex(&pk);
        // 64 lower-hex chars.
        assert_eq!(fp.len(), 64);
        assert!(fp
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // Stable: a second derivation matches.
        assert_eq!(fp, fingerprint_hex(&pk));
    }

    #[test]
    fn device_metadata_roundtrips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("device.json");
        let meta = DeviceMetadata {
            fingerprint: "a".repeat(64),
            device_type: "desktop".to_string(),
            device_uuid: Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        write_metadata(&path, &meta).unwrap();
        assert!(path.exists());
        let read = read_metadata(&path).unwrap();
        assert_eq!(read, meta);
    }

    #[test]
    fn signing_key_pkcs8_b64_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let pkcs8 = sk.to_pkcs8_der().unwrap();
        let b64 = B64.encode(pkcs8.as_bytes());
        let restored = decode_signing_key_b64(&b64).unwrap();
        assert_eq!(
            restored.verifying_key().to_bytes(),
            sk.verifying_key().to_bytes()
        );
    }

    #[cfg(feature = "keyring-mock")]
    mod keyring_mock_tests {
        use super::*;
        use keyring::credential::{CredentialApi, CredentialBuilderApi, CredentialPersistence};
        use keyring::{Credential, Error};
        use std::any::Any;
        use std::collections::HashMap;
        use std::sync::{Mutex, Once, OnceLock};

        #[derive(Debug)]
        struct ProcessMockCredential {
            key: String,
        }

        impl CredentialApi for ProcessMockCredential {
            fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
                mock_store()
                    .lock()
                    .expect("mock keyring store poisoned")
                    .insert(self.key.clone(), secret.to_vec());
                Ok(())
            }

            fn get_secret(&self) -> keyring::Result<Vec<u8>> {
                mock_store()
                    .lock()
                    .expect("mock keyring store poisoned")
                    .get(&self.key)
                    .cloned()
                    .ok_or(Error::NoEntry)
            }

            fn delete_credential(&self) -> keyring::Result<()> {
                mock_store()
                    .lock()
                    .expect("mock keyring store poisoned")
                    .remove(&self.key)
                    .map(|_| ())
                    .ok_or(Error::NoEntry)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        #[derive(Debug)]
        struct ProcessMockBuilder;

        impl CredentialBuilderApi for ProcessMockBuilder {
            fn build(
                &self,
                target: Option<&str>,
                service: &str,
                user: &str,
            ) -> keyring::Result<Box<Credential>> {
                Ok(Box::new(ProcessMockCredential {
                    key: format!("{}:{service}:{user}", target.unwrap_or("")),
                }))
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn persistence(&self) -> CredentialPersistence {
                CredentialPersistence::ProcessOnly
            }
        }

        fn mock_store() -> &'static Mutex<HashMap<String, Vec<u8>>> {
            static STORE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
            STORE.get_or_init(|| Mutex::new(HashMap::new()))
        }

        fn install_mock_keyring() {
            static INSTALL: Once = Once::new();
            INSTALL.call_once(|| {
                keyring::set_default_credential_builder(Box::new(ProcessMockBuilder));
            });
            mock_store()
                .lock()
                .expect("mock keyring store poisoned")
                .clear();
        }

        #[test]
        fn identity_generate_then_load_roundtrips_through_mock_keyring() {
            install_mock_keyring();
            let dir = tempdir().unwrap();

            let first = ensure_identity(dir.path(), "desktop").unwrap();
            let second = ensure_identity(dir.path(), "desktop").unwrap();

            assert_eq!(second.fingerprint(), first.fingerprint());
            assert_eq!(second.device_uuid(), first.device_uuid());
            assert_eq!(second.public_key_bytes(), first.public_key_bytes());
            assert!(!fallback_path(dir.path()).exists());
        }
    }
}
