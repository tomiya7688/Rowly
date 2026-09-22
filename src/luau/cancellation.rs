use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Luau の1回の実行に対する、スレッド間で共有可能な停止要求。
///
/// 実行側へ渡したトークンの clone を GUI や上位ランタイムが保持し、
/// `cancel()` で停止を要求する。VM や CsvDocument 自体は共有しない。
/// 一度要求した停止は解除できない。次の実行には新しいトークンを作成する。
#[derive(Debug, Clone, Default)]
pub struct LuauCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl LuauCancellationToken {
    /// 停止要求のない、新しい実行用トークンを作成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 停止を要求する。繰り返し呼んでもよい。実行完了を待つ操作ではない。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// このトークンに停止が要求されているかを返す。
    pub fn is_cancelled(&self) -> bool {
        // 他のデータを公開する同期ではなく、単独の停止フラグとして使用する。
        self.cancelled.load(Ordering::Relaxed)
    }
}
