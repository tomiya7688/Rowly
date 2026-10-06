use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// {
///   責務: [LuauCancellationToken: 1回のLuau実行へスレッド間で共有可能な停止要求を伝える。]
///   フィールド: [cancelled: clone間で共有する単調なAtomicBool flag。]
///   補足: [停止要求は解除できない。別の実行には新しいtokenを作る。VMとCsvDocumentは共有しない。]
/// }
#[derive(Debug, Clone, Default)]
pub struct LuauCancellationToken {
    cancelled: Arc<AtomicBool>,
}

// {
//   責務: [LuauCancellationTokenの生成・停止要求・停止状態読取を提供する。]
// }
impl LuauCancellationToken {
    /// {
    ///   責務: [new: 停止要求のない新しい実行用tokenを作成する。]
    ///   処理: [Defaultでcancelled flagをfalseに初期化する。]
    ///   引数: []
    ///   戻り値: [LuauCancellationToken: clone可能な停止token。]
    /// }
    pub fn new() -> Self {
        Self::default()
    }

    /// {
    ///   責務: [cancel: 共有flagをtrueにして実行側へ停止を要求する。]
    ///   処理: [AtomicBoolをRelaxed orderingでstoreする。]
    ///   引数: []
    ///   戻り値: [(): 値を返さない。]
    ///   副作用: [同じtokenの全cloneから停止要求が観測可能になる。]
    ///   補足: [繰り返し呼べる。script実行完了を待機しない。]
    /// }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// {
    ///   責務: [is_cancelled: 共有停止flagの現在値を読む。]
    ///   処理: [他stateとのsynchronizationを目的としないRelaxed loadを行う。]
    ///   引数: []
    ///   戻り値: [bool: 停止要求済みならtrue。]
    /// }
    pub fn is_cancelled(&self) -> bool {
        // 他のデータを公開する同期ではなく、単独の停止フラグとして使用する。
        self.cancelled.load(Ordering::Relaxed)
    }
}
