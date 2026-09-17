use std::cell::RefCell;

use mlua::{Error as LuaError, Lua};
use thiserror::Error;

use crate::process::CsvDocument;

/// Luau スクリプトを現在の CSV ドキュメントに対して実行する。
///
/// Luau 側には `Rowly` テーブルだけをアプリケーション API として公開する。
/// CSV のデータ層には直接触れず、すべて `CsvDocument` の process API を経由する。
pub fn execute(document: &mut CsvDocument, script: &str) -> Result<(), LuauError> {
    let lua = Lua::new();
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
    #[error("Luau スクリプトの実行に失敗しました: {0}")]
    Runtime(#[from] LuaError),
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
    fn process_errors_are_reported_as_luau_errors() {
        let (_directory, mut document) = sample_document();

        let error = execute(&mut document, r#"Rowly.set_cell("invalid", "x")"#).unwrap_err();

        assert!(error.to_string().contains("invalid"));
    }
}
