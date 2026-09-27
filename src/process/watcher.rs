use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

/// Delivers best-effort hints that the watched CSV may have changed.
///
/// The parent directory is watched so atomic replacement of the CSV does not
/// silently detach the watcher from the new file. Callers must verify the
/// document fingerprint after receiving a hint.
pub(crate) struct CsvFileWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<Result<(), String>>,
}

impl CsvFileWatcher {
    pub(crate) fn new<F>(path: impl AsRef<Path>, on_change: F) -> Result<Self, CsvWatchError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let path = path.as_ref();
        let file_path =
            std::fs::canonicalize(path).map_err(|source| CsvWatchError::ResolvePath {
                path: path.to_path_buf(),
                source,
            })?;
        let parent = file_path
            .parent()
            .expect("canonical file paths have a parent")
            .to_path_buf();
        let file_name = file_path
            .file_name()
            .expect("canonical file paths have a file name")
            .to_os_string();
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<Event>| match result {
                Ok(event) if is_relevant_event(&event, &file_name) => {
                    let _ = sender.try_send(Ok(()));
                    on_change();
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = sender.try_send(Err(error.to_string()));
                    on_change();
                }
            })
            .map_err(|source| CsvWatchError::Start {
                path: parent.clone(),
                source,
            })?;
        watcher
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|source| CsvWatchError::Start {
                path: parent,
                source,
            })?;

        Ok(Self {
            _watcher: watcher,
            receiver,
        })
    }

    /// Drain coalesced event hints. A hint means only that the disk content
    /// should be checked; it does not prove the CSV itself changed.
    pub(crate) fn take_change_hint(&mut self) -> Result<bool, CsvWatchError> {
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(())) => changed = true,
                Ok(Err(message)) => return Err(CsvWatchError::Event(message)),
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => return Err(CsvWatchError::Stopped),
            }
        }
    }
}

fn is_relevant_event(event: &Event, file_name: &std::ffi::OsStr) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.is_empty()
        || event.paths.iter().any(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case(&file_name.to_string_lossy())
            })
        })
}

#[derive(Debug, Error)]
pub(crate) enum CsvWatchError {
    #[error("failed to resolve watched CSV `{path}`: {source}")]
    ResolvePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to start watching `{path}`: {source}")]
    Start {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
    #[error("filesystem watcher reported an error: {0}")]
    Event(String),
    #[error("filesystem watcher stopped unexpectedly")]
    Stopped,
}
