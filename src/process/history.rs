use std::collections::BTreeMap;

use super::{CellRef, ValidationRule, ValidationTarget};

#[derive(Debug, Clone)]
// ```text
// 責務: [
// CellChange: 1 cellの変更前後の値をundo/redo用に保持する
// ]
// フィールド: [
// reference: 変更cellのzero-based位置
// before: 編集前の文字列値
// after: 編集後の文字列値
// ]
// ```
pub(super) struct CellChange {
    pub(super) reference: CellRef,
    pub(super) before: String,
    pub(super) after: String,
}

#[derive(Debug, Clone)]
// ```text
// 責務: [
// RowEdit: 行挿入・削除で除去または追加した行内容を保持する
// ]
// フィールド: [
// index: 編集位置
// removed: 編集前に除去した行
// inserted: 編集後に追加した行
// ]
// ```
pub(super) struct RowEdit {
    pub(super) index: usize,
    pub(super) removed: Vec<Vec<String>>,
    pub(super) inserted: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
// ```text
// 責務: [
// ColumnChange: 1 row内の列挿入・削除で除去または追加した値を保持する
// ]
// フィールド: [
// row: 編集対象zero-based row index
// index: 編集対象column index
// removed: 編集前に除去した値
// inserted: 編集後に追加した値
// ]
// ```
pub(super) struct ColumnChange {
    pub(super) row: usize,
    pub(super) index: usize,
    pub(super) removed: Vec<String>,
    pub(super) inserted: Vec<String>,
}

#[derive(Debug, Clone)]
// ```text
// 責務: [
// EditOperation: CSV内容・検証規則に対する編集をundo/redo可能な形で表す
// ]
// 補足: [
// Cells / Rows / Columns: 部分的なcell・構造変更
// Contents: CSV全体の置換
// ValidationRules: session内の検証規則変更
// Batch: transactionに集約した操作列
// ]
// ```
pub(super) enum EditOperation {
    Cells(Vec<CellChange>),
    Rows(RowEdit),
    Columns(Vec<ColumnChange>),
    Contents {
        before: Vec<Vec<String>>,
        after: Vec<Vec<String>>,
    },
    ValidationRules {
        before: BTreeMap<ValidationTarget, ValidationRule>,
        after: BTreeMap<ValidationTarget, ValidationRule>,
    },
    Batch(Vec<EditOperation>),
}

impl EditOperation {
    // ```text
    // 責務: [
    // is_empty: 操作が変更内容を持たないかを再帰的に判定する
    // ]
    // 処理: [
    // 1: variantごとに差分を調べ、Batchは各操作を再帰確認する
    // ]
    // 引数: [
    // self: 判定対象操作
    // ]
    // 戻り値: [
    // bool: 実質的な差分がなければtrue
    // ]
    // ```
    fn is_empty(&self) -> bool {
        match self {
            Self::Cells(changes) => changes.is_empty(),
            Self::Rows(edit) => edit.removed.is_empty() && edit.inserted.is_empty(),
            Self::Columns(changes) => changes.is_empty(),
            Self::Contents { before, after } => before == after,
            Self::ValidationRules { before, after } => before == after,
            Self::Batch(operations) => operations.iter().all(Self::is_empty),
        }
    }

    // ```text
    // 責務: [
    // affects_csv: 操作がcanonical CSV内容を変えるか判定する
    // ]
    // 処理: [
    // 1: CSV変更variantを識別し、Batchは各操作を再帰確認する
    // ]
    // 引数: [
    // self: 判定対象操作
    // ]
    // 戻り値: [
    // bool: CSVに差分を作る操作が含まれる場合true
    // ]
    // 補足: [
    // ValidationRulesはsession設定のためCSV dirty状態に含めない
    // ]
    // ```
    fn affects_csv(&self) -> bool {
        match self {
            Self::Cells(changes) => !changes.is_empty(),
            Self::Rows(edit) => !edit.removed.is_empty() || !edit.inserted.is_empty(),
            Self::Columns(changes) => !changes.is_empty(),
            Self::Contents { before, after } => before != after,
            Self::ValidationRules { .. } => false,
            Self::Batch(operations) => operations.iter().any(Self::affects_csv),
        }
    }
}

#[derive(Debug, Clone)]
// ```text
// 責務: [
// EditCommand: undo/redo履歴の操作と前後の状態識別子を保持する
// ]
// フィールド: [
// operation: 適用・取消する編集内容
// before_state_id / after_state_id: 全編集状態の識別子
// before_csv_state_id / after_csv_state_id: CSV内容状態の識別子
// ]
// ```
pub(super) struct EditCommand {
    pub(super) operation: EditOperation,
    before_state_id: u64,
    after_state_id: u64,
    before_csv_state_id: u64,
    after_csv_state_id: u64,
}

#[derive(Debug, Default)]
// ```text
// 責務: [
// EditHistory: undo/redo履歴、transaction、保存済みCSV状態を管理する
// ]
// フィールド: [
// undo / redo: 確定済みcommand stack
// current_state_id / next_state_id: 全編集の状態識別子
// current_csv_state_id / saved_csv_state_id / next_csv_state_id: CSV dirty判定用状態識別子
// transaction: 未確定操作列。transaction中はcommit時に一commandへ集約する
// ]
// ```
pub(super) struct EditHistory {
    undo: Vec<EditCommand>,
    redo: Vec<EditCommand>,
    current_state_id: u64,
    next_state_id: u64,
    current_csv_state_id: u64,
    saved_csv_state_id: u64,
    next_csv_state_id: u64,
    transaction: Option<Vec<EditOperation>>,
}

impl EditHistory {
    // ```text
    // 責務: [
    // is_dirty: 現在のCSV内容が最後の保存状態から変わったか判定する
    // ]
    // 処理: [
    // 1: 現在と保存済みCSV状態IDを比較する
    // 2: active transaction内にCSV変更があればdirtyとする
    // ]
    // 引数: [
    // self: 履歴状態
    // ]
    // 戻り値: [
    // bool: CSV差分が確定またはtransaction内にある場合true
    // ]
    // ```
    pub(super) fn is_dirty(&self) -> bool {
        self.current_csv_state_id != self.saved_csv_state_id
            || self.transaction.as_ref().is_some_and(|operations| {
                operations
                    .iter()
                    .any(|operation| !operation.is_empty() && operation.affects_csv())
            })
    }

    // ```text
    // 責務: [
    // can_undo: transaction外に取消可能commandがあるか判定する
    // ]
    // 処理: [
    // 1: transactionの有無とundo stackを確認する
    // ]
    // 引数: [
    // self: 履歴状態
    // ]
    // 戻り値: [
    // bool: transactionがなくundo stackが空でなければtrue
    // ]
    // ```
    pub(super) fn can_undo(&self) -> bool {
        self.transaction.is_none() && !self.undo.is_empty()
    }

    // ```text
    // 責務: [
    // can_redo: transaction外に再適用可能commandがあるか判定する
    // ]
    // 処理: [
    // 1: transactionの有無とredo stackを確認する
    // ]
    // 引数: [
    // self: 履歴状態
    // ]
    // 戻り値: [
    // bool: transactionがなくredo stackが空でなければtrue
    // ]
    // ```
    pub(super) fn can_redo(&self) -> bool {
        self.transaction.is_none() && !self.redo.is_empty()
    }

    // ```text
    // 責務: [
    // record: 空操作を除き、transactionまたはundo履歴へ操作を記録する
    // ]
    // 処理: [
    // 1: 空操作を無視する
    // 2: active transactionへ追加するか、確定commandとして記録する
    // ]
    // 引数: [
    // self: 変更対象履歴
    // operation: 記録する編集操作
    // ]
    // 戻り値: [unit]
    // 副作用: [
    // transaction外で記録するとredo履歴を破棄する
    // ]
    // ```
    pub(super) fn record(&mut self, operation: EditOperation) {
        if operation.is_empty() {
            return;
        }

        if let Some(transaction) = &mut self.transaction {
            transaction.push(operation);
            return;
        }

        self.record_committed(operation);
    }

    // ```text
    // 責務: [
    // begin_transaction: 操作集約用transactionを開始する
    // ]
    // 処理: [
    // 1: 開始済みならfalse、未開始なら空の操作列を作りtrueを返す
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [
    // bool: transactionが未開始ならtrue、既に開始済みならfalse
    // ]
    // ```
    pub(super) fn begin_transaction(&mut self) -> bool {
        if self.transaction.is_some() {
            return false;
        }
        self.transaction = Some(Vec::new());
        true
    }

    // ```text
    // 責務: [
    // commit_transaction: transaction内の有効操作を1つのBatch commandとして確定する
    // ]
    // 処理: [
    // 1: active操作列を取り出して空操作を除く
    // 2: 残った操作があればBatch commandとして記録する
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [
    // bool: active transactionを閉じた場合true、開始されていなければfalse
    // ]
    // ```
    pub(super) fn commit_transaction(&mut self) -> bool {
        let Some(operations) = self.transaction.take() else {
            return false;
        };
        let non_empty = operations
            .into_iter()
            .filter(|operation| !operation.is_empty())
            .collect::<Vec<_>>();
        if !non_empty.is_empty() {
            self.record_committed(EditOperation::Batch(non_empty));
        }
        true
    }

    // ```text
    // 責務: [
    // take_transaction: active transactionを履歴から取り出す
    // ]
    // 処理: [
    // 1: transaction欄をtakeして返す
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [
    // Option<Vec<EditOperation>>: 取り出した操作列。transactionがなければNone
    // ]
    // ```
    pub(super) fn take_transaction(&mut self) -> Option<Vec<EditOperation>> {
        self.transaction.take()
    }

    // ```text
    // 責務: [
    // restore_transaction: rollback等で一時退避した操作列をactive transactionへ戻す
    // ]
    // 処理: [
    // 1: 指定された操作列をtransaction欄へ設定する
    // ]
    // 引数: [
    // self: 変更対象履歴
    // operations: 復元する操作列
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn restore_transaction(&mut self, operations: Vec<EditOperation>) {
        self.transaction = Some(operations);
    }

    // ```text
    // 責務: [
    // transaction_active: transactionが開始中かを返す
    // ]
    // 処理: [
    // 1: transaction欄がSomeかどうかを返す
    // ]
    // 引数: [
    // self: 履歴状態
    // ]
    // 戻り値: [
    // bool: 操作列を集約中ならtrue
    // ]
    // ```
    pub(super) fn transaction_active(&self) -> bool {
        self.transaction.is_some()
    }

    // ```text
    // 責務: [
    // record_committed: 編集を新しいcommandとして確定し、状態IDとstackを更新する
    // ]
    // 処理: [
    // 1: CSV変更ならCSV状態IDを進め、全編集状態IDを進める
    // 2: commandをundoへ積み、redo stackを破棄する
    // ]
    // 引数: [
    // self: 変更対象履歴
    // operation: 確定する編集操作
    // ]
    // 戻り値: [unit]
    // 副作用: [
    // CSV変更時にCSV状態IDを進め、undoへ追加してredoを破棄する
    // ]
    // ```
    fn record_committed(&mut self, operation: EditOperation) {
        let before_csv_state_id = self.current_csv_state_id;
        if operation.affects_csv() {
            self.next_csv_state_id = self.next_csv_state_id.saturating_add(1);
            self.current_csv_state_id = self.next_csv_state_id;
        }
        self.next_state_id = self.next_state_id.saturating_add(1);
        let command = EditCommand {
            operation,
            before_state_id: self.current_state_id,
            after_state_id: self.next_state_id,
            before_csv_state_id,
            after_csv_state_id: self.current_csv_state_id,
        };
        self.current_state_id = command.after_state_id;
        self.undo.push(command);
        self.redo.clear();
    }

    // ```text
    // 責務: [
    // take_undo: undo stackから直近commandを一時的に取り出す
    // ]
    // 処理: [
    // 1: undo stackからpopして返す
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [
    // Option<EditCommand>: 直近command。stackが空ならNone
    // ]
    // ```
    pub(super) fn take_undo(&mut self) -> Option<EditCommand> {
        self.undo.pop()
    }

    // ```text
    // 責務: [
    // commit_undo: 適用済みundoを確定して状態を戻し、commandをredoへ移す
    // ]
    // 処理: [
    // 1: commandの変更前state IDへ戻し、redo stackへ積む
    // ]
    // 引数: [
    // self: 変更対象履歴
    // command: undoしたcommand
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn commit_undo(&mut self, command: EditCommand) {
        self.current_state_id = command.before_state_id;
        self.current_csv_state_id = command.before_csv_state_id;
        self.redo.push(command);
    }

    // ```text
    // 責務: [
    // restore_undo: undo適用が失敗したcommandをundo stackへ戻す
    // ]
    // 処理: [
    // 1: commandをundo stackへ再度積む
    // ]
    // 引数: [
    // self: 変更対象履歴
    // command: 適用前へ戻すcommand
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn restore_undo(&mut self, command: EditCommand) {
        self.undo.push(command);
    }

    // ```text
    // 責務: [
    // take_redo: redo stackから直近commandを一時的に取り出す
    // ]
    // 処理: [
    // 1: redo stackからpopして返す
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [
    // Option<EditCommand>: 直近command。stackが空ならNone
    // ]
    // ```
    pub(super) fn take_redo(&mut self) -> Option<EditCommand> {
        self.redo.pop()
    }

    // ```text
    // 責務: [
    // commit_redo: 適用済みredoを確定して状態を進め、commandをundoへ移す
    // ]
    // 処理: [
    // 1: commandの変更後state IDへ進め、undo stackへ積む
    // ]
    // 引数: [
    // self: 変更対象履歴
    // command: 再適用したcommand
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn commit_redo(&mut self, command: EditCommand) {
        self.current_state_id = command.after_state_id;
        self.current_csv_state_id = command.after_csv_state_id;
        self.undo.push(command);
    }

    // ```text
    // 責務: [
    // restore_redo: redo適用が失敗したcommandをredo stackへ戻す
    // ]
    // 処理: [
    // 1: commandをredo stackへ再度積む
    // ]
    // 引数: [
    // self: 変更対象履歴
    // command: 適用前へ戻すcommand
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn restore_redo(&mut self, command: EditCommand) {
        self.redo.push(command);
    }

    // ```text
    // 責務: [
    // mark_saved: 現在のCSV状態IDを保存済み状態として記録する
    // ]
    // 処理: [
    // 1: current CSV state IDをsaved state IDへ設定する
    // ]
    // 引数: [
    // self: 変更対象履歴
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn mark_saved(&mut self) {
        self.saved_csv_state_id = self.current_csv_state_id;
    }
}
