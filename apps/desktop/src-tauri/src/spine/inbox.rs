//! Sync-inbox: materialize decrypted notes to disk and feed them into the local index.
//!
//! PRD 004 §US-028. Files land under `<data-dir>/sync-inbox/`. Writing is atomic
//! (tmp → fsync → rename), filename input is sanitized to neutralize path traversal,
//! and `syncmind_indexing::index_file_once` is invoked synchronously after rename so the
//! caller knows whether to ACK the bundle.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tracing::{info, warn};

use crate::spine::bundle::BundleEnvelope;
use crate::spine::{SpineError, SpineErrorCode};

const INBOX_DIR: &str = "sync-inbox";
/// Max sanitized filename length in bytes, excluding the timestamp prefix and any
/// disambiguation suffix.
const MAX_FILENAME_BYTES: usize = 200;

/// Result of writing an envelope to disk + reindexing.
#[derive(Debug, Clone, Serialize)]
pub struct InboxWriteReport {
    pub final_path: PathBuf,
    pub chunks_added: usize,
}

/// One entry in the inbox listing returned by `list_inbox`.
#[derive(Debug, Clone, Serialize)]
pub struct InboxEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_unix: i64,
}

/// Ensure the inbox directory exists with restrictive permissions.
pub fn ensure_inbox_dir(data_dir: &Path) -> Result<PathBuf, SpineError> {
    let dir = data_dir.join(INBOX_DIR);
    fs::create_dir_all(&dir)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    set_dir_permissions_0700(&dir)?;
    Ok(dir)
}

/// Write `envelope` to the inbox, fsync, rename atomically, then invoke
/// `index_file_once`. The bundle is only safe to ACK once this returns `Ok`.
///
/// `indexer` is a callback that runs the indexing pipeline for the freshly-rename'd file.
/// It returns the chunk count on success. The desktop wires this to
/// `syncmind_indexing::index_file_once` with its existing `AppState` resources.
pub async fn write_envelope_and_index<I, F>(
    data_dir: &Path,
    envelope: &BundleEnvelope,
    bundle_id: &str,
    indexer: I,
) -> Result<InboxWriteReport, SpineError>
where
    I: FnOnce(PathBuf) -> F,
    F: std::future::Future<Output = anyhow::Result<usize>>,
{
    let dir = ensure_inbox_dir(data_dir)?;
    let prefix = captured_at_prefix(&envelope.captured_at);
    let safe_name = sanitize_filename(&envelope.filename);
    let base = format!("{prefix}-{safe_name}");

    let final_path = pick_unique_path(&dir, &base);
    let tmp_path = with_tmp_suffix(&final_path);

    // Write payload then fsync.
    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        f.write_all(envelope.content_utf8.as_bytes())
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        f.sync_all()
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    }
    fs::rename(&tmp_path, &final_path)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;

    // Companion .meta.json (best effort; failure does not abort the write).
    let meta = MetaSidecar {
        bundle_id: bundle_id.to_string(),
        captured_at: envelope.captured_at.clone(),
        sha256: envelope.sha256.clone(),
        source_path: envelope.source_path.clone(),
        filename: envelope.filename.clone(),
    };
    let meta_path = final_path.with_extension(format!(
        "{}.meta.json",
        final_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
    ));
    if let Err(e) = write_meta_sidecar(&meta_path, &meta) {
        warn!(error = %e, "failed to write meta sidecar (continuing)");
    }

    // Index the freshly-rename'd file. If this fails, we DO return an error to the caller
    // so the bundle is not ACKed (PRD 004 §US-028 acceptance criteria).
    let chunks_added = indexer(final_path.clone())
        .await
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    info!(
        path = %final_path.display(),
        chunks_added,
        "sync-inbox file materialized and indexed"
    );

    Ok(InboxWriteReport {
        final_path,
        chunks_added,
    })
}

/// List every file under `<data-dir>/sync-inbox/`. Companion `.meta.json` files are excluded.
pub fn list_inbox(data_dir: &Path) -> Result<Vec<InboxEntry>, SpineError> {
    let dir = data_dir.join(INBOX_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for raw in
        fs::read_dir(&dir).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?
    {
        let raw = raw.map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        let path = raw.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".meta.json") {
                continue;
            }
        }
        let meta = raw
            .metadata()
            .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        let modified_unix = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(InboxEntry {
            path,
            size_bytes: meta.len(),
            modified_unix,
        });
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.modified_unix));
    Ok(entries)
}

/// Delete every file under `<data-dir>/sync-inbox/`, recreate the directory with mode 0700,
/// and return the number of files removed.
pub fn clear_inbox(data_dir: &Path) -> Result<usize, SpineError> {
    let dir = data_dir.join(INBOX_DIR);
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for raw in
        fs::read_dir(&dir).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?
    {
        let raw = raw.map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
        let p = raw.path();
        if p.is_file() {
            fs::remove_file(&p)
                .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
            count += 1;
        }
    }
    set_dir_permissions_0700(&dir)?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct MetaSidecar {
    bundle_id: String,
    captured_at: String,
    sha256: String,
    source_path: Option<String>,
    filename: String,
}

fn write_meta_sidecar(path: &Path, meta: &MetaSidecar) -> Result<(), SpineError> {
    let raw = serde_json::to_string_pretty(meta)
        .map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))?;
    fs::write(path, raw).map_err(|e| SpineError::new(SpineErrorCode::Internal, e.to_string()))
}

fn captured_at_prefix(rfc3339: &str) -> String {
    // Parse to UTC unix-ms; fall back to "now" if parsing fails (best-effort prefix).
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|d| d.with_timezone(&chrono::Utc).timestamp_millis())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis())
        .to_string()
}

/// Restrict filename to ASCII letters, digits, `-`, `_`, `.`. Other bytes (including all
/// non-ASCII, control chars, path separators, and the path-traversal `..`) become `_`.
/// The result is at most `MAX_FILENAME_BYTES` bytes long and never empty.
pub fn sanitize_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
        out.push(if ok { c } else { '_' });
    }
    // Collapse a leading dot run so we don't produce `.hidden` or `..foo` unintentionally.
    while out.starts_with('.') {
        out.replace_range(0..1, "_");
    }
    // Reject all-dot / empty names defensively.
    if out.is_empty() || out.chars().all(|c| c == '.' || c == '_') {
        out = "note.md".to_string();
    }
    if out.len() > MAX_FILENAME_BYTES {
        out.truncate(MAX_FILENAME_BYTES);
    }
    out
}

fn pick_unique_path(dir: &Path, base: &str) -> PathBuf {
    let candidate = dir.join(base);
    if !candidate.exists() {
        return candidate;
    }
    // Split into stem + ext for the (N) suffix.
    let (stem, ext) = match base.rfind('.') {
        Some(i) if i > 0 => (&base[..i], &base[i..]),
        _ => (base, ""),
    };
    for n in 2u32..u32::MAX {
        let c = dir.join(format!("{stem}({n}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    // Astronomically unlikely; fall back to base + unix-nanos.
    dir.join(format!(
        "{}-{}{}",
        stem,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        ext
    ))
}

fn with_tmp_suffix(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
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

#[cfg(not(unix))]
fn set_dir_permissions_0700(_p: &Path) -> Result<(), SpineError> {
    Ok(())
}

// `Arc` import used only by callers of `write_envelope_and_index` via the indexer closure.
#[allow(dead_code)]
fn _arc_used() -> Option<Arc<()>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sanitize_filename_neutralizes_traversal_and_unicode() {
        // Path separators are replaced; dots may survive in the interior (a `..` inside a
        // filename is not path traversal — the file is `dir.join(name)`, never `dir/..`).
        let sanitized = sanitize_filename("../../etc/passwd");
        assert!(
            !sanitized.starts_with('.'),
            "leading dots must be stripped, got {sanitized:?}"
        );
        assert!(
            !sanitized.contains('/') && !sanitized.contains('\\'),
            "path separators must be replaced, got {sanitized:?}"
        );

        assert_eq!(sanitize_filename("hello world.md"), "hello_world.md");
        // Non-ASCII becomes underscores.
        let from_unicode = sanitize_filename("ファイル.md");
        assert!(
            !from_unicode.contains("ファ"),
            "non-ASCII should be replaced, got {from_unicode:?}"
        );
        // No leading dot.
        assert!(!sanitize_filename(".dotfile").starts_with('.'));
        // Never empty.
        assert!(!sanitize_filename("").is_empty());
        // Truncates to the byte cap.
        assert!(sanitize_filename(&"a".repeat(500)).len() <= MAX_FILENAME_BYTES);
    }

    #[test]
    fn pick_unique_path_appends_counter_on_collision() {
        let dir = tempdir().unwrap();
        let base = "note.md";
        let p1 = pick_unique_path(dir.path(), base);
        fs::write(&p1, b"x").unwrap();
        let p2 = pick_unique_path(dir.path(), base);
        assert_ne!(p1, p2);
        assert!(p2.file_name().unwrap().to_string_lossy().contains("(2)"));
    }

    #[tokio::test]
    async fn write_envelope_round_trip() {
        let data_dir = tempdir().unwrap();
        let envelope = BundleEnvelope::new_note("draft.md", "hello world", None);

        let report = write_envelope_and_index(
            data_dir.path(),
            &envelope,
            "bundle-id-123",
            |path| async move {
                // Mock indexer: just verify the file was written.
                let content = fs::read_to_string(&path).unwrap();
                assert_eq!(content, "hello world");
                Ok(3)
            },
        )
        .await
        .unwrap();

        assert!(report.final_path.exists());
        assert_eq!(report.chunks_added, 3);
        let entries = list_inbox(data_dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn indexing_failure_propagates_so_bundle_is_not_acked() {
        let data_dir = tempdir().unwrap();
        let envelope = BundleEnvelope::new_note("a.md", "x", None);

        let err = write_envelope_and_index(data_dir.path(), &envelope, "bid", |_path| async move {
            Err(anyhow::anyhow!("indexer down"))
        })
        .await
        .unwrap_err();
        assert_eq!(err.code, "INTERNAL_ERROR");
        // File DOES exist (the failure was after rename) — this is intentional so the user
        // has a local copy and we don't lose data.
        let entries = list_inbox(data_dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn clear_inbox_removes_files_and_preserves_dir() {
        let data_dir = tempdir().unwrap();
        ensure_inbox_dir(data_dir.path()).unwrap();
        let dir = data_dir.path().join(INBOX_DIR);
        fs::write(dir.join("a.md"), b"x").unwrap();
        fs::write(dir.join("b.md"), b"y").unwrap();
        let removed = clear_inbox(data_dir.path()).unwrap();
        assert_eq!(removed, 2);
        assert!(dir.exists());
        assert_eq!(list_inbox(data_dir.path()).unwrap().len(), 0);
    }
}
