use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

/// ```text
/// 責務: [
/// CsvFileWatcher: 対象CSVの変更可能性を非同期hintとして通知する
/// ]
/// フィールド: [
/// _watcher: parent directoryを監視し続けるnotify watcher
/// receiver: coalesceした変更hintまたは監視errorの受信口
/// ]
/// 補足: [
/// atomic replacement後も監視が継続するようCSVのparent directoryを監視する
/// hintはCSV内容の変更確定を意味しないため、呼び出し側がfingerprintを検証する
/// ]
/// ```
pub(crate) struct CsvFileWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<Result<(), String>>,
}

impl CsvFileWatcher {
    /// ```text
    /// 責務: [
    /// new: CSVのparent directoryを監視し、関連eventをcallbackとhint channelへ送る
    /// ]
    /// 処理: [
    /// 1: CSV pathをcanonicalizeし、parent pathとfile nameを得る
    /// 2: 対象file eventと監視errorを受けるwatcherを作成する
    /// 3: parent directoryをnon-recursiveで監視する
    /// ]
    /// 引数: [
    /// path: 監視するCSV path
    /// on_change: 対象eventまたはwatcher errorごとに呼ぶcallback
    /// ]
    /// 戻り値: [
    /// Result<CsvFileWatcher, CsvWatchError>: active watcher、またはpath解決・監視開始error
    /// ]
    /// 副作用: [
    /// filesystem watcherを開始し、callbackをwatcher event処理へ登録する
    /// ]
    /// ```
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

    /// ```text
    /// 責務: [
    /// take_change_hint: queue内のcoalesce済み変更hintを全て読み取り状態を返す
    /// ]
    /// 処理: [
    /// 1: channelにあるhintを空になるまでdrainする
    /// 2: hintが1件以上ならtrueを返す
    /// ]
    /// 引数: [
    /// self: 対象watcher
    /// ]
    /// 戻り値: [
    /// Result<bool, CsvWatchError>: hintの有無、またはwatcher event/stopped error
    /// ]
    /// 補足: [
    /// hintだけではCSV内容変更を確定できないため、呼び出し側がfingerprintを確認する
    /// ]
    /// ```
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

// ```text
// 責務: [
// is_relevant_event: filesystem eventが監視対象CSVに関係するかを判定する
// ]
// 処理: [
// 1: access eventを除外する
// 2: event pathが空なら関連ありとし、他はfile nameをcase-insensitiveで照合する
// ]
// 引数: [
// event: 判定するnotify event
// file_name: 監視対象CSVのfile name
// ]
// 戻り値: [
// bool: 対象eventとして扱う場合true
// ]
// ```
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
/// ```text
/// 責務: [
/// CsvWatchError: CSV file watcherの初期化・event受信の失敗を表す
/// ]
/// 補足: [
/// ResolvePath: 監視対象pathをcanonicalizeできない
/// Start: notify watcherを作成またはparent directoryへ接続できない
/// Event: filesystem watcherからerror eventが届いた
/// Stopped: hint channelが予期せず閉じた
/// ]
/// ```
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
