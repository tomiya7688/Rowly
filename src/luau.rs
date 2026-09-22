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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuauLimits {
    pub max_duration: Duration,
    pub max_interrupts: u64,
    pub max_memory_bytes: usize,
}

impl Default for LuauLimits {
    fn default() -> Self {
        Self {
            max_duration: DEFAULT_MAX_DURATION,
            max_interrupts: DEFAULT_MAX_INTERRUPTS,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        }
    }
}

#[derive(Debug, Error)]
enum LuauLimitError {
    #[error("Luau スクリプトが実行時間上限を超えました")]
    Duration,
    #[error("Luau スクリプトが実行ステップ上限を超えました")]
    Interrupts,
}

#[derive(Debug, Error)]
#[error("Luau スクリプトの実行がキャンセルされました")]
struct LuauCancelled;

/// Luau スクリプトを現在の CSV ドキュメントに対して既定の制限付きで実行する。
///
/// Luau 側には `Rowly` テーブルだけをアプリケーション API として公開する。
/// CSV のデータ層には直接触れず、すべて `CsvDocument` の process API を経由する。
pub fn execute(document: &mut CsvDocument, script: &str) -> Result<(), LuauError> {
    execute_with_limits(document, script, LuauLimits::default())
}

/// Luau スクリプトを現在の CSV ドキュメントに対して指定した制限付きで実行する。
pub fn execute_with_limits(
    document: &mut CsvDocument,
    script: &str,
    limits: LuauLimits,
) -> Result<(), LuauError> {
    execute_with_limits_and_cancellation(document, script, limits, &LuauCancellationToken::new())
}

/// 既定の実行制限と、外部からの停止要求を受け付けるトークンで実行する。
///
/// この関数は同期実行する。停止を要求する側はトークンの clone を保持する。
pub fn execute_with_cancellation(
    document: &mut CsvDocument,
    script: &str,
    cancellation: &LuauCancellationToken,
) -> Result<(), LuauError> {
    execute_with_limits_and_cancellation(document, script, LuauLimits::default(), cancellation)
}

/// 指定した実行制限と停止トークンで実行する。
///
/// 停止は VM の safepoint と Rowly API の入口で確認する。実行中の Rust / C の
/// 処理を OS スレッドごと強制終了するものではない。スクリプト開始時に
/// transaction がなければ、失敗時に残った未確定 transaction を rollback する。
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

fn check_cancellation(cancellation: &LuauCancellationToken) -> Result<(), LuaError> {
    if cancellation.is_cancelled() {
        return Err(LuaError::external(LuauCancelled));
    }
    Ok(())
}

fn runtime_error(error: impl ToString) -> LuaError {
    LuaError::RuntimeError(error.to_string())
}

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

impl From<LuaError> for LuauError {
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

    fn sample_document() -> (tempfile::TempDir, CsvDocument) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sample.csv");
        fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
        let document = CsvDocument::open(path).unwrap();
        (directory, document)
    }

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

    #[test]
    fn luau_transaction_state_errors_are_runtime_errors() {
        let (_directory, mut document) = sample_document();

        let error = execute(&mut document, "Rowly.commit_transaction()").unwrap_err();

        assert!(matches!(error, LuauError::Runtime(_)));
        assert!(error.to_string().contains("transaction"));
    }

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

    #[test]
    fn process_errors_are_reported_as_luau_errors() {
        let (_directory, mut document) = sample_document();

        let error = execute(&mut document, r#"Rowly.set_cell("invalid", "x")"#).unwrap_err();

        assert!(error.to_string().contains("invalid"));
    }
}
