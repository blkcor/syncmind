use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("Notify error: {0}")]
    Notify(#[from] notify::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// A debounced file watcher that emits batches of changed paths.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    watched_paths: HashSet<PathBuf>,
    _debounce_handle: tokio::task::JoinHandle<()>,
}

impl FileWatcher {
    /// Create a new file watcher.
    ///
    /// Watches the given `paths` and emits batches of changed file paths
    /// on `output` after `debounce_duration` of inactivity.
    pub fn new(
        paths: Vec<PathBuf>,
        debounce_duration: Duration,
        output: mpsc::Sender<Vec<PathBuf>>,
    ) -> Result<Self, WatcherError> {
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = event_tx.try_send(event);
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        let mut me = Self {
            watcher,
            watched_paths: HashSet::new(),
            _debounce_handle: tokio::spawn(async move {}),
        };
        me.add_paths(&paths)?;

        let handle = tokio::spawn(async move {
            let mut pending: HashSet<PathBuf> = HashSet::new();
            let mut debounce_deadline: Option<tokio::time::Instant> = None;

            loop {
                let sleep_fut = debounce_deadline.map(|d| tokio::time::sleep_until(d));

                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        for path in event.paths {
                            if path.is_file() {
                                let canonical = tokio::fs::canonicalize(&path).await
                                    .unwrap_or(path);
                                pending.insert(canonical);
                            }
                        }
                        debounce_deadline = Some(tokio::time::Instant::now() + debounce_duration);
                    }
                    _ = async { sleep_fut.unwrap().await }, if sleep_fut.is_some() => {
                        if !pending.is_empty() {
                            let batch: Vec<PathBuf> = pending.drain().collect();
                            debug!(count = batch.len(), "emitting debounced batch");
                            if output.send(batch).await.is_err() {
                                break;
                            }
                        }
                        debounce_deadline = None;
                    }
                    else => break,
                }
            }
        });

        me._debounce_handle = handle;
        Ok(me)
    }

    /// Replace the watched paths with a new set.
    pub fn update_paths(&mut self, paths: &[PathBuf]) -> Result<(), WatcherError> {
        let new_set: HashSet<PathBuf> = paths.iter().cloned().collect();

        let to_remove: Vec<PathBuf> = self
            .watched_paths
            .difference(&new_set)
            .cloned()
            .collect();
        let to_add: Vec<PathBuf> = new_set
            .difference(&self.watched_paths)
            .cloned()
            .collect();

        for path in to_remove {
            debug!(path = %path.display(), "unwatching file");
            if let Err(e) = self.watcher.unwatch(&path) {
                warn!(path = %path.display(), error = %e, "failed to unwatch file");
            }
            self.watched_paths.remove(&path);
        }

        self.add_paths(&to_add)?;
        Ok(())
    }

    fn add_paths(&mut self, paths: &[PathBuf]) -> Result<(), WatcherError> {
        for path in paths {
            if path.is_file() {
                let canonical = std::fs::canonicalize(path)
                    .unwrap_or_else(|_| path.clone());
                debug!(path = %canonical.display(), "watching file");
                self.watcher.watch(&canonical, RecursiveMode::NonRecursive)?;
                self.watched_paths.insert(canonical);
            } else {
                warn!(path = %path.display(), "skipping non-existent or non-file path");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn watcher_emits_debounced_batch() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "hello").unwrap();
        std::fs::write(&file_b, "world").unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let _watcher = FileWatcher::new(
            vec![file_a.clone(), file_b.clone()],
            Duration::from_millis(200),
            tx,
        )
        .unwrap();

        // Give FSEvents time to register.
        sleep(Duration::from_millis(300)).await;

        // Modify both files in quick succession.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&file_a)
            .unwrap();
        writeln!(f, " modified").unwrap();
        drop(f);

        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&file_b)
            .unwrap();
        writeln!(f, " modified").unwrap();
        drop(f);

        // Wait for debounce + FSEvents latency margin.
        sleep(Duration::from_secs(3)).await;

        let batch = rx.try_recv().expect("expected a debounced batch");
        let canonical_a = std::fs::canonicalize(&file_a).unwrap_or_else(|_| file_a.clone());
        let canonical_b = std::fs::canonicalize(&file_b).unwrap_or_else(|_| file_b.clone());
        assert!(
            batch.contains(&canonical_a) || batch.contains(&canonical_b),
            "batch should contain at least one modified file, got: {:?}",
            batch
        );
    }

    #[tokio::test]
    async fn update_paths_changes_watched_files() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "hello").unwrap();
        std::fs::write(&file_b, "world").unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let mut watcher = FileWatcher::new(
            vec![file_a.clone()],
            Duration::from_millis(200),
            tx,
        )
        .unwrap();

        sleep(Duration::from_millis(300)).await;

        // Switch to watching file_b only.
        watcher.update_paths(&[file_b.clone()]).unwrap();

        sleep(Duration::from_millis(300)).await;

        // Modify file_a — should not be emitted.
        std::fs::write(&file_a, "changed a").unwrap();
        sleep(Duration::from_secs(3)).await;
        assert!(rx.try_recv().is_err(), "file_a should not trigger after update");

        // Modify file_b — should be emitted.
        std::fs::write(&file_b, "changed b").unwrap();
        sleep(Duration::from_secs(3)).await;
        let batch = rx.try_recv().expect("expected batch for file_b");
        let canonical_b = std::fs::canonicalize(&file_b).unwrap_or_else(|_| file_b.clone());
        assert!(batch.contains(&canonical_b), "batch should contain file_b, got: {:?}", batch);
    }
}
