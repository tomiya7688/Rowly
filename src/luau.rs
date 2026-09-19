use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use mlua::{Error as LuaError, Lua, VmState};
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
    let lua = Lua::new();
    lua.set_memory_limit(limits.max_memory_bytes)?;

    let started_at = Instant::now();
    let interrupts = Arc::new(AtomicU64::new(0));
    let interrupt_count = Arc::clone(&interrupts);
    lua.set_interrupt(move |_| {
        if started_at.elapsed() >= limits.max_duration {
            return Err(LuaError::external(LuauLimitError::Duration));
        }
        let current = interrupt_count.fetch_add(1, Ordering::Relaxed) + 1;
        if current > limits.max_interrupts {
            return Err(LuaError::external(LuauLimitError::Interrupts));
        }
        Ok(VmState::Continue)
    });

    let document = RefCell::new(document);

    lua.scope(|scope| {
        let rowly = lua.create_table()?;

        rowly.set(
            "cell",
            scope.create_function(|_, reference: String| {
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
                document
                    .borrow_mut()
                    .set_cell_a1(&reference, value)
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "set_range",
            scope.create_function(|_, (range, value): (String, String)| {
                document
                    .borrow_mut()
                    .set_range_a1(&range, value)
                    .map_err(runtime_error)
            })?,
        )?;

        rowly.set(
            "row_count",
            scope.create_function(|_, ()| Ok(document.borrow().row_count() as i64))?,
        )?;

        rowly.set(
            "column_count",
            scope.create_function(|_, ()| Ok(document.borrow().column_count() as i64))?,
        )?;

        rowly.set(
            "undo",
            scope.create_function(|_, ()| document.borrow_mut().undo().map_err(runtime_error))?,
        )?;

        rowly.set(
            "redo",
            scope.create_function(|_, ()| document.borrow_mut().redo().map_err(runtime_error))?,
        )?;

        lua.globals().set("Rowly", rowly)?;
        lua.load(script).set_name("rowly-user-script").exec()
    })
    .map_err(LuauError::from)
}

fn runtime_error(error: impl ToString) -> LuaError {
    LuaError::RuntimeError(error.to_string())
}

#[derive(Debug, Error)]
pub enum LuauError {
    #[error("Luau スクリプトが実行制限を超えました: {0}")]
    Limit(String),
    #[error("Luau スクリプトのメモリ上限を超えました: {0}")]
    Memory(String),
    #[error("Luau スクリプトの実行に失敗しました: {0}")]
    Runtime(LuaError),
}

impl From<LuaError> for LuauError {
    fn from(error: LuaError) -> Self {
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
    fn luau_undo_and_redo_use_document_history() {
        let (_directory, mut document) = sample_document();

        execute(
            &mut document,
            r#"
                Rowly.set_cell("B2", "99")
                assert(Rowly.undo() == true)
                assert(Rowly.cell("B2") == "10")
                assert(Rowly.redo() == true)
                assert(Rowly.cell("B2") == "99")
            "#,
        )
        .unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("99"));
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
