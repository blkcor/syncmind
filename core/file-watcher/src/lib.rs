use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
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

/// Classified file change event emitted by the watcher.
///
/// `Upsert` means the file should be re-indexed (created or modified).
/// `Remove` means the file's chunks should be deleted from the index
/// (deletion or the source side of a rename).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileEvent {
    Upsert(PathBuf),
    Remove(PathBuf),
}

impl FileEvent {
    pub fn path(&self) -> &PathBuf {
        match self {
            FileEvent::Upsert(p) | FileEvent::Remove(p) => p,
        }
    }
}

/// A debounced file watcher that emits batches of classified file events.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    watched_paths: HashSet<PathBuf>,
    _debounce_handle: tokio::task::JoinHandle<()>,
}

impl FileWatcher {
    /// Create a new file watcher.
    ///
    /// Watches the given `paths` and emits batches of `FileEvent` values
    /// on `output` after `debounce_duration` of inactivity. Within a batch
    /// each path appears at most once, holding the latest classification
    /// observed during the debounce window.
    pub fn new(
        paths: Vec<PathBuf>,
        debounce_duration: Duration,
        output: mpsc::Sender<Vec<FileEvent>>,
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
            // Path → latest classified event. A later event for the same path
            // overwrites the earlier one so the final on-disk intent wins.
            let mut pending: HashMap<PathBuf, FileEvent> = HashMap::new();
            let mut debounce_deadline: Option<tokio::time::Instant> = None;

            loop {
                let sleep_fut = debounce_deadline.map(tokio::time::sleep_until);

                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        for classified in classify_event(event).await {
                            pending.insert(classified.path().clone(), classified);
                        }
                        debounce_deadline = Some(tokio::time::Instant::now() + debounce_duration);
                    }
                    _ = async { sleep_fut.unwrap().await }, if sleep_fut.is_some() => {
                        if !pending.is_empty() {
                            // Reconcile with disk state: FSEvents on macOS
                            // emits trailing Modify events after a Remove,
                            // which would otherwise mask the deletion. Always
                            // trust the filesystem at flush time.
                            let batch: Vec<FileEvent> = pending
                                .drain()
                                .map(|(path, ev)| {
                                    if !path.exists() {
                                        FileEvent::Remove(path)
                                    } else {
                                        match ev {
                                            FileEvent::Remove(_) => FileEvent::Upsert(path),
                                            FileEvent::Upsert(p) => FileEvent::Upsert(p),
                                        }
                                    }
                                })
                                .collect();
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

/// Classify a notify event into one or more `FileEvent` values.
///
/// Removal events keep the (now-missing) path; we cannot canonicalize after
/// deletion, so the raw event path is forwarded.
async fn classify_event(event: Event) -> Vec<FileEvent> {
    let mut out = Vec::with_capacity(event.paths.len());

    match event.kind {
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Metadata(_)) | EventKind::Modify(ModifyKind::Any) => {
            for path in event.paths {
                let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                out.push(FileEvent::Upsert(canonical));
            }
        }
        EventKind::Modify(ModifyKind::Name(mode)) => {
            // macOS often emits a single Name(Both) event with [from, to].
            // Linux typically emits Name(From) and Name(To) separately.
            match mode {
                RenameMode::Both if event.paths.len() == 2 => {
                    out.push(FileEvent::Remove(event.paths[0].clone()));
                    let to = event.paths[1].clone();
                    let canonical = tokio::fs::canonicalize(&to).await.unwrap_or(to);
                    out.push(FileEvent::Upsert(canonical));
                }
                RenameMode::From => {
                    for path in event.paths {
                        out.push(FileEvent::Remove(path));
                    }
                }
                RenameMode::To => {
                    for path in event.paths {
                        let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                        out.push(FileEvent::Upsert(canonical));
                    }
                }
                _ => {
                    // Unknown/Any rename mode: treat as upsert when the file
                    // exists, remove otherwise.
                    for path in event.paths {
                        if path.exists() {
                            let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                            out.push(FileEvent::Upsert(canonical));
                        } else {
                            out.push(FileEvent::Remove(path));
                        }
                    }
                }
            }
        }
        EventKind::Modify(ModifyKind::Other) => {
            for path in event.paths {
                if path.exists() {
                    let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                    out.push(FileEvent::Upsert(canonical));
                }
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                out.push(FileEvent::Remove(path));
            }
        }
        EventKind::Access(_) | EventKind::Any | EventKind::Other => {
            // Ignore access events and unknown event types.
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn watcher_emits_debounced_upsert_batch() {
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

        sleep(Duration::from_millis(300)).await;

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file_a)
            .unwrap();
        writeln!(f, " modified").unwrap();
        drop(f);

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file_b)
            .unwrap();
        writeln!(f, " modified").unwrap();
        drop(f);

        sleep(Duration::from_secs(3)).await;

        let batch = rx.try_recv().expect("expected a debounced batch");
        assert!(
            batch.iter().any(|e| matches!(e, FileEvent::Upsert(_))),
            "batch should contain at least one Upsert event, got: {:?}",
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

        watcher.update_paths(std::slice::from_ref(&file_b)).unwrap();

        sleep(Duration::from_millis(300)).await;

        std::fs::write(&file_a, "changed a").unwrap();
        sleep(Duration::from_secs(3)).await;
        assert!(rx.try_recv().is_err(), "file_a should not trigger after update");

        std::fs::write(&file_b, "changed b").unwrap();
        sleep(Duration::from_secs(3)).await;
        let batch = rx.try_recv().expect("expected batch for file_b");
        let canonical_b = std::fs::canonicalize(&file_b).unwrap_or_else(|_| file_b.clone());
        assert!(
            batch.iter().any(|e| matches!(e, FileEvent::Upsert(p) if p == &canonical_b)),
            "batch should contain Upsert(file_b), got: {:?}",
            batch
        );
    }

    #[tokio::test]
    async fn watcher_emits_remove_event_on_delete() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doomed.txt");
        std::fs::write(&file, "fleeting").unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let _watcher = FileWatcher::new(
            vec![file.clone()],
            Duration::from_millis(200),
            tx,
        )
        .unwrap();

        sleep(Duration::from_millis(300)).await;

        std::fs::remove_file(&file).unwrap();

        sleep(Duration::from_secs(3)).await;

        let batch = rx.try_recv().expect("expected a debounced batch after delete");
        assert!(
            batch.iter().any(|e| matches!(e, FileEvent::Remove(_))),
            "batch should contain a Remove event, got: {:?}",
            batch
        );
    }
}
