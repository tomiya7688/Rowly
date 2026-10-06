mod cancellation;
mod sandbox;

pub use cancellation::LuauCancellationToken;

use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use mlua::{ChunkMode, Error as LuaError, Lua, VmState};
use thiserror::Error;

use crate::process::CsvDocument;

const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(5);
const DEFAULT_MAX_INTERRUPTS: u64 = 1_000_000;
const DEFAULT_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;

// {
//   責務: [LuauLimits: 1回のLuau実行に適用する時間・中断回数・memory上限をまとめる。]
//   フィールド: [max_duration: 実行時間上限。 max_interrupts: VM interrupt上限。 max_memory_bytes: Lua VM memory上限。]
// }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuauLimits {
    pub max_duration: Duration,
    pub max_interrupts: u64,
    pub max_memory_bytes: usize,
}

// {
//   責務: [LuauLimitsの既定値をアプリケーション標準の実行上限から構成する。]
// }
impl Default for LuauLimits {
    // {
    //   責務: [default: named default limit定数からLuauLimitsを生成する。]
    //   処理: [各上限fieldへ対応するDEFAULT_MAX_*定数を設定する。]
    //   引数: []
    //   戻り値: [LuauLimits: 既定の時間・interrupt・memory上限。]
    // }
    fn default() -> Self {
        Self {
            max_duration: DEFAULT_MAX_DURATION,
            max_interrupts: DEFAULT_MAX_INTERRUPTS,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        }
    }
}

// {
//   責務: [LuauLimitError: Luau VMの時間またはinterrupt上限到達を分類する。]
//   選択肢: [Duration: 実行時間上限を超過。 Interrupts: VM interrupt上限を超過。]
// }
#[derive(Debug, Error)]
enum LuauLimitError {
    #[error("Luau スクリプトが実行時間上限を超えました")]
    Duration,
    #[error("Luau スクリプトが実行ステップ上限を超えました")]
    Interrupts,
}

// {
//   責務: [LuauCancelled: mlua callback内の停止要求を識別可能な外部errorとして運ぶ。]
// }
#[derive(Debug, Error)]
#[error("Luau スクリプトの実行がキャンセルされました")]
struct LuauCancelled;

// {
//   責務: [execute: Luau scriptを既定の実行制限で実行し、Rowly process APIだけを通してdocumentへ作用させる。]
//   処理: [execute_with_limitsへ既定LuauLimitsを渡す。]
//   引数: [document: 編集対象CsvDocument。 script: 実行するLuau source。]
//   戻り値: [(): 正常終了。 LuauError: VM・script・resource limit失敗。]
//   副作用: [Rowly API経由のdocument編集。]
// }
pub fn execute(document: &mut CsvDocument, script: &str) -> Result<(), LuauError> {
    execute_with_limits(document, script, LuauLimits::default())
}

// {
//   責務: [execute_with_limits: 呼び出し側が指定したLuau resource limitsでscriptを実行する。]
//   処理: [新しいcancellation tokenを作り、execute_with_limits_and_cancellationへ委譲する。]
//   引数: [document: 編集対象CsvDocument。 script: 実行するLuau source。 limits: VMへ適用する時間・interrupt・memory上限。]
//   戻り値: [(): 正常終了。 LuauError: VM・script・resource limit失敗。]
//   副作用: [Rowly API経由のdocument編集。]
// }
pub fn execute_with_limits(
    document: &mut CsvDocument,
    script: &str,
    limits: LuauLimits,
) -> Result<(), LuauError> {
    execute_with_limits_and_cancellation(document, script, limits, &LuauCancellationToken::new())
}

// {
//   責務: [execute_with_cancellation: 既定のresource limitsと共有cancellation tokenでscriptを実行する。]
//   処理: [既定LuauLimitsと受け取ったtokenを共通実行入口へ渡す。]
//   引数: [document: 編集対象CsvDocument。 script: 実行するLuau source。 cancellation: 停止要求を共有するtoken。]
//   戻り値: [(): 正常終了。 LuauError: cancellation・VM・script失敗。]
//   副作用: [Rowly API経由のdocument編集。]
// }
pub fn execute_with_cancellation(
    document: &mut CsvDocument,
    script: &str,
    cancellation: &LuauCancellationToken,
) -> Result<(), LuauError> {
    execute_with_limits_and_cancellation(document, script, LuauLimits::default(), cancellation)
}

// {
//   責務: [execute_with_limits_and_cancellation: resource limitsとcancellation tokenを適用してscriptを実行し、開始後に残った未確定transactionをcleanupする。]
//   処理: [実行前とVM safepointおよび各Rowly API入口で停止を確認し、sandbox VM内でscriptを実行する。失敗時は開始前にtransactionがなかった場合に限りactive transactionをrollbackする。]
//   引数: [document: 編集対象CsvDocument。 script: 実行するLuau source。 limits: VM resource上限。 cancellation: 実行停止token。]
//   戻り値: [(): 正常終了。 LuauError: cancellation・limit・script・cleanup失敗。]
//   副作用: [Rowly API経由のdocument編集とtransaction cleanup。]
// }
pub fn execute_with_limits_and_cancellation(
    document: &mut CsvDocument,
    script: &str,
    limits: LuauLimits,
    cancellation: &LuauCancellationToken,
) -> Result<(), LuauError> {
    check_cancellation(cancellation)?;
    let lua = sandbox::new_vm()?;
    execute_in_lua(&lua, document, script, limits, cancellation)
}

// {
//   責務: [execute_in_lua: 制限・停止callbackとRowly process APIを登録し、sandbox内でLuau sourceを実行する。]
//   処理: [memory limitとVM interrupt callbackを設定する。Rowly cell/range/count/transaction APIを登録してreadonly化しsandboxを有効にした後、text chunkを実行する。停止errorを再分類し、必要なら開始後のtransactionをrollbackする。]
//   引数: [lua: 新しく生成したVM。 document: APIが操作するCsvDocument。 script: 実行source。 limits: resource上限。 cancellation: 停止token。]
//   戻り値: [(): script正常終了。 LuauError: script実行・limit・cleanup失敗。]
//   副作用: [CSV document編集。script失敗時の未確定transaction rollback。]
// }
fn execute_in_lua(
    lua: &Lua,
    document: &mut CsvDocument,
    script: &str,
    limits: LuauLimits,
    cancellation: &LuauCancellationToken,
) -> Result<(), LuauError> {
    check_cancellation(cancellation)?;
    lua.set_memory_limit(limits.max_memory_bytes)?;

    let started_at = Instant::now();
    let interrupts = Arc::new(AtomicU64::new(0));
    let interrupt_count = Arc::clone(&interrupts);
    let interrupt_cancellation = cancellation.clone();
    lua.set_interrupt(move |_| {
        check_cancellation(&interrupt_cancellation)?;
        if started_at.elapsed() >= limits.max_duration {
            return Err(LuaError::external(LuauLimitError::Duration));
        }
        let current = interrupt_count.fetch_add(1, Ordering::Relaxed) + 1;
        if current > limits.max_interrupts {
            return Err(LuaError::external(LuauLimitError::Interrupts));
        }
        Ok(VmState::Continue)
    });

    let transaction_active_before = document.transaction_active();
    let document = RefCell::new(document);

    let result = lua.scope(|scope| {
        let rowly = lua.create_table()?;

        rowly.set(
            "cell",
            scope.create_function(|_, reference: String| {
                check_cancellation(cancellation)?;
                document
                    .borrow()
                    .cell_a1(&reference)
                    .map(|value| value.map(str::to_owned))
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "set_cell",
            scope.create_function(|_, (reference, value): (String, String)| {
                check_cancellation(cancellation)?;
                document
                    .borrow_mut()
                    .set_cell_a1(&reference, value)
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "set_range",
            scope.create_function(|_, (range, value): (String, String)| {
                check_cancellation(cancellation)?;
                document
                    .borrow_mut()
                    .set_range_a1(&range, value)
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "row_count",
            scope.create_function(|_, ()| {
                check_cancellation(cancellation)?;
                Ok(document.borrow().row_count() as i64)
            })?,
        )?;

        rowly.set(
            "column_count",
            scope.create_function(|_, ()| {
                check_cancellation(cancellation)?;
                Ok(document.borrow().column_count() as i64)
            })?,
        )?;

        rowly.set(
            "begin_transaction",
            scope.create_function(|_, ()| {
                check_cancellation(cancellation)?;
                document
                    .borrow_mut()
                    .begin_transaction()
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "commit_transaction",
            scope.create_function(|_, ()| {
                check_cancellation(cancellation)?;
                document
                    .borrow_mut()
                    .commit_transaction()
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "rollback_transaction",
            scope.create_function(|_, ()| {
                check_cancellation(cancellation)?;
                document
                    .borrow_mut()
                    .rollback_transaction()
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set_readonly(true);
        lua.globals().set("Rowly", rowly)?;
        // API 登録後に標準テーブル・組み込みメタテーブル・共有グローバルを
        // 保護する。各実行の変数代入は Luau のローカル環境へ隔離される。
        lua.sandbox(true)?;
        check_cancellation(cancellation)?;
        lua.load(script)
            .set_name("rowly-user-script")
            .set_mode(ChunkMode::Text)
            .exec()
    });

    // pcall / xpcall により停止エラーが捕捉されても、Rust 側では成功にしない。
    // この確認を cleanup より先に行い、未確定 transaction を取り残さない。
    let result = if cancellation.is_cancelled() {
        Err(LuauError::Cancelled)
    } else {
        result.map_err(LuauError::from)
    };
    if result.is_err() && !transaction_active_before && document.borrow().transaction_active() {
        document
            .borrow_mut()
            .rollback_transaction()
            .map_err(|error| LuauError::Cleanup(error.to_string()))?;
    }

    result
}

// {
//   責務: [check_cancellation: 停止tokenが要求済みならmluaの外部errorへ変換する。]
//   処理: [cancelled flagを読み、trueならLuauCancelledを含むLuaErrorを返す。]
//   引数: [cancellation: 読み取り対象の実行token。]
//   戻り値: [(): 停止要求なし。 LuaError: 停止要求あり。]
// }
fn check_cancellation(cancellation: &LuauCancellationToken) -> Result<(), LuaError> {
    if cancellation.is_cancelled() {
        return Err(LuaError::external(LuauCancelled));
    }
    Ok(())
}

// {
//   責務: [runtime_error: Rowly process errorをmlua runtime errorへ変換する。]
//   処理: [errorを文字列化してLuaError::RuntimeErrorを作る。]
//   引数: [error: ToStringを実装したprocess error。]
//   戻り値: [LuaError: Luau scriptへ伝えるruntime error。]
// }
fn runtime_error(error: impl ToString) -> LuaError {
    LuaError::RuntimeError(error.to_string())
}

// {
//   責務: [LuauError: cancellation・resource limit・memory・cleanup・runtime失敗を公開API用errorへ分類する。]
//   選択肢: [Cancelled: 停止要求。 Limit: 時間またはinterrupt制限。 Memory: VM memory制限。 Cleanup: rollback失敗。 Runtime: その他のmlua error。]
// }
#[derive(Debug, Error)]
pub enum LuauError {
    #[error("Luau スクリプトの実行がキャンセルされました")]
    Cancelled,
    #[error("Luau スクリプトが実行制限を超えました: {0}")]
    Limit(String),
    #[error("Luau スクリプトのメモリ上限を超えました: {0}")]
    Memory(String),
    #[error("Luau スクリプト失敗後の transaction rollback に失敗しました: {0}")]
    Cleanup(String),
    #[error("Luau スクリプトの実行に失敗しました: {0}")]
    Runtime(LuaError),
}

// {
//   責務: [LuaErrorをLuauErrorへ分類変換し、呼び出し側に安定した失敗種別を返す。]
// }
impl From<LuaError> for LuauError {
    // {
    //   責務: [from: mlua errorを停止・limit・memory・runtime分類へ変換する。]
    //   処理: [外部停止marker、limit error、MemoryErrorを順に判定し、残りをRuntimeに包む。]
    //   引数: [error: mluaから返された実行error。]
    //   戻り値: [LuauError: 公開するerror分類。]
    // }
    fn from(error: LuaError) -> Self {
        if error.downcast_ref::<LuauCancelled>().is_some() {
            return Self::Cancelled;
        }
        if let Some(limit) = error.downcast_ref::<LuauLimitError>() {
            return Self::Limit(limit.to_string());
        }
        if matches!(error, LuaError::MemoryError(_)) {
            return Self::Memory(error.to_string());
        }
        Self::Runtime(error)
    }
}

#[cfg(test)]
mod tests_cancellation;
#[cfg(test)]
mod tests_sandbox;

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    // {
    //   責務: [sample_document: Luau runtime test用CSV fixtureを一時fileに保存してCsvDocumentを開く。]
    //   処理: [TempDirを作成し、固定のName/Score CSVを書き込んでCsvDocument::openへ渡す。]
    //   引数: []
    //   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
    //   副作用: [一時ディレクトリにsample.csvを作成する。]
    // }
    fn sample_document() -> (tempfile::TempDir, CsvDocument) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sample.csv");
        fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
        let document = CsvDocument::open(path).unwrap();
        (directory, document)
    }

    // {
    //   責務: [luau_can_read_and_edit_through_process_api: Rowly process APIでcell・range・countを読み書きする。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn luau_can_read_and_edit_through_process_api() {
        let (_directory, mut document) = sample_document();

        execute(
            &mut document,
            r#"
                assert(Rowly.row_count() == 3)
                assert(Rowly.column_count() == 2)
                assert(Rowly.cell("A2") == "Alice")
                Rowly.set_cell("B2", "42")
                Rowly.set_range("A3:B3", "updated")
            "#,
        )
        .unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
        assert_eq!(document.cell_a1("A3").unwrap(), Some("updated"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("updated"));
    }

    // {
    //   責務: [luau_does_not_expose_undo_or_redo_but_edits_still_use_document_history: Luauへundo/redoを公開せず、編集がdocument historyへ記録されることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn luau_does_not_expose_undo_or_redo_but_edits_still_use_document_history() {
        let (_directory, mut document) = sample_document();

        execute(
            &mut document,
            r#"
                assert(Rowly.undo == nil)
                assert(Rowly.redo == nil)
                Rowly.set_cell("B2", "99")
            "#,
        )
        .unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("99"));
        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert!(document.redo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("99"));
    }

    // {
    //   責務: [luau_transaction_commit_is_one_undoable_command: transaction内の複数Luau編集がcommit後に1回のundo/redo単位になることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn luau_transaction_commit_is_one_undoable_command() {
        let (_directory, mut document) = sample_document();

        execute(
            &mut document,
            r#"
                Rowly.begin_transaction()
                Rowly.set_cell("B2", "42")
                Rowly.set_cell("B3", "99")
                Rowly.commit_transaction()
            "#,
        )
        .unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("99"));
        assert!(!document.transaction_active());

        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
        assert!(!document.can_undo());

        assert!(document.redo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("99"));
    }

    // {
    //   責務: [luau_transaction_rollback_restores_without_history: 明示rollbackがcell値を戻し、history・dirty状態を残さないことを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn luau_transaction_rollback_restores_without_history() {
        let (_directory, mut document) = sample_document();

        execute(
            &mut document,
            r#"
                Rowly.begin_transaction()
                Rowly.set_range("B2:B3", "changed")
                Rowly.rollback_transaction()
            "#,
        )
        .unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
        assert!(!document.transaction_active());
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    // {
    //   責務: [failed_luau_script_rolls_back_its_uncommitted_transaction: script errorで実行側が開始した未確定transactionをrollbackすることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn failed_luau_script_rolls_back_its_uncommitted_transaction() {
        let (_directory, mut document) = sample_document();

        let error = execute(
            &mut document,
            r#"
                Rowly.begin_transaction()
                Rowly.set_cell("B2", "42")
                error("stop")
            "#,
        )
        .unwrap_err();

        assert!(matches!(error, LuauError::Runtime(_)));
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert!(!document.transaction_active());
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    // {
    //   責務: [execution_limit_rolls_back_uncommitted_luau_transaction: interrupt上限超過で未確定transactionをrollbackすることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn execution_limit_rolls_back_uncommitted_luau_transaction() {
        let (_directory, mut document) = sample_document();

        let error = execute_with_limits(
            &mut document,
            r#"
                Rowly.begin_transaction()
                Rowly.set_cell("B2", "42")
                while true do end
            "#,
            LuauLimits {
                max_duration: Duration::from_secs(10),
                max_interrupts: 100,
                max_memory_bytes: 8 * 1024 * 1024,
            },
        )
        .unwrap_err();

        assert!(matches!(error, LuauError::Limit(_)));
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert!(!document.transaction_active());
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    // {
    //   責務: [luau_transaction_state_errors_are_runtime_errors: 未開始transactionへのcommitがLuau runtime errorになることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn luau_transaction_state_errors_are_runtime_errors() {
        let (_directory, mut document) = sample_document();

        let error = execute(&mut document, "Rowly.commit_transaction()").unwrap_err();

        assert!(matches!(error, LuauError::Runtime(_)));
        assert!(error.to_string().contains("transaction"));
    }

    // {
    //   責務: [infinite_loop_is_stopped_by_interrupt_limit: 無限loopをinterrupt上限で停止しLuau limit errorにすることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn infinite_loop_is_stopped_by_interrupt_limit() {
        let (_directory, mut document) = sample_document();

        let error = execute_with_limits(
            &mut document,
            "while true do end",
            LuauLimits {
                max_duration: Duration::from_secs(10),
                max_interrupts: 100,
                max_memory_bytes: 8 * 1024 * 1024,
            },
        )
        .unwrap_err();

        assert!(matches!(error, LuauError::Limit(_)));
    }

    // {
    //   責務: [memory_limit_stops_unbounded_allocation: 際限ないallocationをmemoryまたは実行制限errorで止めることを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn memory_limit_stops_unbounded_allocation() {
        let (_directory, mut document) = sample_document();

        let error = execute_with_limits(
            &mut document,
            r#"
                local values = {}
                while true do
                    table.insert(values, string.rep("x", 4096))
                end
            "#,
            LuauLimits {
                max_duration: Duration::from_secs(10),
                max_interrupts: 1_000_000,
                max_memory_bytes: 512 * 1024,
            },
        )
        .unwrap_err();

        assert!(matches!(error, LuauError::Memory(_) | LuauError::Limit(_)));
    }

    // {
    //   責務: [process_errors_are_reported_as_luau_errors: 不正cell参照によるprocess errorをLuau側のerrorとして報告することを確認する。]
    //   処理: [sample_documentで用意したdocumentに対して対象Luau scriptを実行し、期待するerrorまたはdocument状態をassertする。]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない。]
    // }
    #[test]
    fn process_errors_are_reported_as_luau_errors() {
        let (_directory, mut document) = sample_document();

        let error = execute(&mut document, r#"Rowly.set_cell("invalid", "x")"#).unwrap_err();

        assert!(error.to_string().contains("invalid"));
    }
}
