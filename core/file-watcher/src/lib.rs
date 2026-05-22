use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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
    watched_dirs: HashSet<PathBuf>,
    tracked_paths: Arc<Mutex<HashSet<PathBuf>>>,
    _debounce_handle: Option<tokio::task::JoinHandle<()>>,
    _runtime: Option<tokio::runtime::Runtime>,
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

        let tracked_paths = Arc::new(Mutex::new(HashSet::new()));
        let debounce_tracked_paths = tracked_paths.clone();
        let mut me = Self {
            watcher,
            watched_dirs: HashSet::new(),
            tracked_paths,
            _debounce_handle: None,
            _runtime: None,
        };
        me.add_paths(&paths)?;

        let debounce_task = async move {
            // Path → latest classified event. A later event for the same path
            // overwrites the earlier one so the final on-disk intent wins.
            let mut pending: HashMap<PathBuf, FileEvent> = HashMap::new();
            let mut debounce_deadline: Option<tokio::time::Instant> = None;

            loop {
                let sleep_fut = debounce_deadline.map(tokio::time::sleep_until);

                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        for classified in classify_event(event, &debounce_tracked_paths).await {
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
        };

        let (handle, runtime) = match tokio::runtime::Handle::try_current() {
            Ok(runtime_handle) => (runtime_handle.spawn(debounce_task), None),
            Err(_) => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("syncmind-file-watcher")
                    .build()?;
                let handle = runtime.spawn(debounce_task);
                (handle, Some(runtime))
            }
        };

        me._debounce_handle = Some(handle);
        me._runtime = runtime;
        Ok(me)
    }

    /// Replace the watched paths with a new set.
    pub fn update_paths(&mut self, paths: &[PathBuf]) -> Result<(), WatcherError> {
        let new_set: HashSet<PathBuf> = paths.iter().flat_map(tracked_file_paths).collect();

        let new_dirs: HashSet<PathBuf> = new_set
            .iter()
            .filter_map(|path| path.parent().map(PathBuf::from))
            .collect();

        let to_remove: Vec<PathBuf> = self.watched_dirs.difference(&new_dirs).cloned().collect();
        let to_add: Vec<PathBuf> = new_dirs.difference(&self.watched_dirs).cloned().collect();

        for dir in to_remove {
            debug!(path = %dir.display(), "unwatching directory");
            if let Err(e) = self.watcher.unwatch(&dir) {
                warn!(path = %dir.display(), error = %e, "failed to unwatch directory");
            }
            self.watched_dirs.remove(&dir);
        }

        for dir in to_add {
            debug!(path = %dir.display(), "watching directory");
            self.watcher.watch(&dir, RecursiveMode::NonRecursive)?;
            self.watched_dirs.insert(dir);
        }

        *self.tracked_paths.lock().unwrap() = new_set;
        Ok(())
    }

    fn add_paths(&mut self, paths: &[PathBuf]) -> Result<(), WatcherError> {
        let files: HashSet<PathBuf> = paths
            .iter()
            .filter_map(|path| {
                let canonical = canonical_file_path(path);
                if canonical.is_none() {
                    warn!(path = %path.display(), "skipping non-existent or non-file path");
                }
                canonical
            })
            .collect();

        let tracked_files: HashSet<PathBuf> = paths.iter().flat_map(tracked_file_paths).collect();

        let dirs: HashSet<PathBuf> = files
            .iter()
            .filter_map(|path| path.parent().map(PathBuf::from))
            .collect();

        let to_add: Vec<PathBuf> = dirs.difference(&self.watched_dirs).cloned().collect();
        for dir in to_add {
            debug!(path = %dir.display(), "watching directory");
            self.watcher.watch(&dir, RecursiveMode::NonRecursive)?;
            self.watched_dirs.insert(dir);
        }

        self.tracked_paths.lock().unwrap().extend(tracked_files);
        Ok(())
    }
}

fn tracked_file_paths(path: &PathBuf) -> Vec<PathBuf> {
    let Some(canonical) = canonical_file_path(path) else {
        return Vec::new();
    };
    if canonical == *path {
        vec![canonical]
    } else {
        vec![path.clone(), canonical]
    }
}

fn canonical_file_path(path: &PathBuf) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
}

/// Classify a notify event into one or more `FileEvent` values.
///
/// Removal events keep the (now-missing) path; we cannot canonicalize after
/// deletion, so the raw event path is forwarded.
async fn classify_event(
    event: Event,
    tracked_paths: &Arc<Mutex<HashSet<PathBuf>>>,
) -> Vec<FileEvent> {
    let mut out = Vec::with_capacity(event.paths.len());

    match event.kind {
        EventKind::Create(_)
        | EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Metadata(_))
        | EventKind::Modify(ModifyKind::Any) => {
            for path in event.paths {
                let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                if is_tracked(&canonical, tracked_paths) {
                    out.push(FileEvent::Upsert(canonical));
                }
            }
        }
        EventKind::Modify(ModifyKind::Name(mode)) => {
            // macOS often emits a single Name(Both) event with [from, to].
            // Linux typically emits Name(From) and Name(To) separately.
            match mode {
                RenameMode::Both if event.paths.len() == 2 => {
                    let from = event.paths[0].clone();
                    let to = event.paths[1].clone();
                    if is_tracked(&from, tracked_paths) {
                        out.push(FileEvent::Remove(from.clone()));
                        let canonical_to = tokio::fs::canonicalize(&to).await.unwrap_or(to);
                        replace_tracked_path(&from, &canonical_to, tracked_paths);
                        out.push(FileEvent::Upsert(canonical_to));
                    }
                }
                RenameMode::From => {
                    for path in event.paths {
                        if is_tracked(&path, tracked_paths) {
                            out.push(FileEvent::Remove(path));
                        }
                    }
                }
                RenameMode::To => {
                    for path in event.paths {
                        let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                        if is_tracked(&canonical, tracked_paths) {
                            out.push(FileEvent::Upsert(canonical));
                        }
                    }
                }
                _ => {
                    // Unknown/Any rename mode: treat as upsert when the file
                    // exists, remove otherwise.
                    for path in event.paths {
                        if path.exists() {
                            let canonical = tokio::fs::canonicalize(&path).await.unwrap_or(path);
                            if is_tracked(&canonical, tracked_paths) {
                                out.push(FileEvent::Upsert(canonical));
                            }
                        } else if is_tracked(&path, tracked_paths) {
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
                    if is_tracked(&canonical, tracked_paths) {
                        out.push(FileEvent::Upsert(canonical));
                    }
                }
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                if is_tracked(&path, tracked_paths) {
                    out.push(FileEvent::Remove(path));
                }
            }
        }
        EventKind::Access(_) | EventKind::Any | EventKind::Other => {
            // Ignore access events and unknown event types.
        }
    }

    out
}

fn is_tracked(path: &PathBuf, tracked_paths: &Arc<Mutex<HashSet<PathBuf>>>) -> bool {
    tracked_paths.lock().unwrap().contains(path)
}

fn replace_tracked_path(
    from: &PathBuf,
    to: &PathBuf,
    tracked_paths: &Arc<Mutex<HashSet<PathBuf>>>,
) {
    let mut tracked = tracked_paths.lock().unwrap();
    tracked.remove(from);
    tracked.insert(to.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{RemoveKind, RenameMode};
    use tokio::time::Duration;

    fn tracked_set(paths: &[PathBuf]) -> Arc<Mutex<HashSet<PathBuf>>> {
        Arc::new(Mutex::new(
            paths.iter().flat_map(tracked_file_paths).collect(),
        ))
    }

    #[tokio::test]
    async fn classify_remove_keeps_deleted_registered_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doomed.txt");
        std::fs::write(&file, "fleeting").unwrap();
        let tracked = tracked_set(std::slice::from_ref(&file));
        std::fs::remove_file(&file).unwrap();

        let event = Event::new(EventKind::Remove(RemoveKind::File)).add_path(file.clone());
        let classified = classify_event(event, &tracked).await;

        assert_eq!(classified, vec![FileEvent::Remove(file)]);
    }

    #[tokio::test]
    async fn classify_rename_both_removes_old_path_and_tracks_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old.txt");
        let to = dir.path().join("new.txt");
        std::fs::write(&from, "moving").unwrap();
        let tracked = tracked_set(std::slice::from_ref(&from));
        std::fs::rename(&from, &to).unwrap();

        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(from.clone())
            .add_path(to.clone());
        let classified = classify_event(event, &tracked).await;
        let canonical_to = std::fs::canonicalize(&to).unwrap_or(to);

        assert_eq!(
            classified,
            vec![
                FileEvent::Remove(from.clone()),
                FileEvent::Upsert(canonical_to.clone()),
            ]
        );
        assert!(tracked.lock().unwrap().contains(&canonical_to));
        assert!(!tracked.lock().unwrap().contains(&from));
    }

    #[tokio::test]
    async fn classify_upsert_ignores_untracked_paths() {
        let dir = tempfile::tempdir().unwrap();
        let tracked = dir.path().join("tracked.txt");
        let ignored = dir.path().join("ignored.txt");
        std::fs::write(&tracked, "tracked").unwrap();
        std::fs::write(&ignored, "ignored").unwrap();
        let tracked_paths = tracked_set(std::slice::from_ref(&tracked));

        let event = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(tracked.clone())
        .add_path(ignored);
        let classified = classify_event(event, &tracked_paths).await;
        let canonical = std::fs::canonicalize(&tracked).unwrap_or(tracked);

        assert_eq!(classified, vec![FileEvent::Upsert(canonical)]);
    }

    #[test]
    fn update_paths_changes_tracked_files_and_watched_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let subdir = dir.path().join("nested");
        std::fs::create_dir(&subdir).unwrap();
        let file_b = subdir.join("b.txt");
        std::fs::write(&file_a, "hello").unwrap();
        std::fs::write(&file_b, "world").unwrap();

        let (tx, _rx) = mpsc::channel(16);
        let mut watcher =
            FileWatcher::new(vec![file_a.clone()], Duration::from_millis(200), tx).unwrap();

        let canonical_a = std::fs::canonicalize(&file_a).unwrap_or_else(|_| file_a.clone());
        let canonical_a_dir = canonical_a.parent().unwrap();
        assert!(watcher.tracked_paths.lock().unwrap().contains(&canonical_a));
        assert!(watcher.watched_dirs.contains(canonical_a_dir));

        watcher.update_paths(std::slice::from_ref(&file_b)).unwrap();

        let canonical_b = std::fs::canonicalize(&file_b).unwrap_or_else(|_| file_b.clone());
        let tracked = watcher.tracked_paths.lock().unwrap();
        assert!(!tracked.contains(&canonical_a));
        assert!(tracked.contains(&canonical_b));
        drop(tracked);
        assert!(!watcher.watched_dirs.contains(canonical_a_dir));
        assert!(watcher.watched_dirs.contains(canonical_b.parent().unwrap()));
    }

    #[test]
    fn watcher_skips_missing_registered_files_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.txt");

        let (tx, _rx) = mpsc::channel(16);
        let watcher = FileWatcher::new(vec![missing], Duration::from_millis(200), tx).unwrap();

        assert!(watcher.tracked_paths.lock().unwrap().is_empty());
        assert!(watcher.watched_dirs.is_empty());
    }
}
