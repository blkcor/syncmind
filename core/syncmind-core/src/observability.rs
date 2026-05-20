use std::path::Path;

use crate::config::LogRotation;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

/// Initialize the global tracing subscriber for the SyncMind daemon.
///
/// Layered design:
/// - Always: rolling file appender in `log_dir` (gated by `log_to_file`)
/// - Optional: pretty stderr layer when `with_stderr` is true (foreground mode)
///
/// The Stdio MCP transport requires stdout to remain reserved for JSON-RPC,
/// so this function never writes to stdout. Stderr is independent of stdout
/// and is safe to use in foreground mode.
///
/// The returned `WorkerGuard` must be kept alive for the lifetime of the
/// process; dropping it causes the non-blocking writer to discard buffered
/// log lines.
///
/// Falls back to stderr-only logging if the log directory cannot be created.
pub fn init_tracing(
    log_dir: &Path,
    log_level: &str,
    log_to_file: bool,
    log_rotation: LogRotation,
    with_stderr: bool,
) -> Option<WorkerGuard> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    let file_layer_outcome = if log_to_file {
        match std::fs::create_dir_all(log_dir) {
            Ok(()) => {
                let appender = match log_rotation {
                    LogRotation::Daily => rolling::daily(log_dir, "syncmind.log"),
                    LogRotation::Hourly => rolling::hourly(log_dir, "syncmind.log"),
                    LogRotation::Never => rolling::never(log_dir, "syncmind.log"),
                };
                let (writer, guard) = tracing_appender::non_blocking(appender);
                Some((writer, guard))
            }
            Err(e) => {
                eprintln!(
                    "warning: could not create log directory {}: {}",
                    log_dir.display(),
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let file_layer = file_layer_outcome
        .as_ref()
        .map(|(writer, _guard)| fmt::layer().with_writer(writer.clone()).with_ansi(false));

    let stderr_layer = with_stderr.then(|| fmt::layer().with_writer(std::io::stderr));

    Registry::default()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();

    file_layer_outcome.map(|(_, guard)| guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // The global tracing dispatcher is process-wide; only one test can install
    // it. We serialize and run the meaningful assertions in a single test.
    fn install_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn init_tracing_creates_log_file_and_writes() {
        let _guard_outer = install_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().to_path_buf();

        let _g = init_tracing(&log_dir, "info", true, LogRotation::Daily, false);
        tracing::info!("hello from init_tracing test");

        // Drop the guard to flush the non-blocking writer.
        drop(_g);
        // Give the writer thread a moment to flush.
        std::thread::sleep(std::time::Duration::from_millis(200));

        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.iter().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("syncmind.log")
            }),
            "expected a syncmind.log* file in {}, got {:?}",
            log_dir.display(),
            entries.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }
}
