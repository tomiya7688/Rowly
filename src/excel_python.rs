use std::{
    env,
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::{Value, json};
use thiserror::Error;

use crate::process::{CsvDocument, DocumentError};

// {
//   責務: [BRIDGE_SCRIPT: source checkoutでPython bridgeを起動するときのscript pathを固定する。]
// }
const BRIDGE_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/python/excel_bridge.py");
// {
//   責務: [DEFAULT_PYTHON: 開発fallbackで使うOS別Python launcher名を示す。]
// }
const DEFAULT_PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

/// {
///   責務: [export_document: CsvDocumentの文字列cellを指定sheetのExcel workbookへ書き出す。]
///   処理: [documentの行をJSON requestへ変換し、Excel bridgeへexportを依頼する。]
///   引数: [document: 書き出すcanonical CSV document。 output_path: 生成するworkbook path。 sheet_name: 作成するworksheet名。]
///   戻り値: [Result<(), ExcelError>: 書き出し成功、またはbridge・protocol error。]
///   副作用: [output_pathにExcel workbookを作成する。]
/// }
pub fn export_document(
    document: &CsvDocument,
    output_path: impl AsRef<Path>,
    sheet_name: &str,
) -> Result<(), ExcelError> {
    let rows = document
        .rows()
        .map(|row| row.to_vec())
        .collect::<Vec<Vec<String>>>();
    let request = json!({
        "output_path": output_path.as_ref(),
        "sheet_name": sheet_name,
        "rows": rows,
    });
    run_bridge("export", &request)?;
    Ok(())
}

/// {
///   責務: [import_workbook: Excel worksheetの値を文字列cellとしてCsvDocumentへ取り込む。]
///   処理: [bridgeのJSON responseを検証し、rowsを指定csv_pathのdocumentとして作成する。]
///   引数: [input_path: 読み込むworkbook path。 csv_path: 新規documentの保存先。 sheet_name: 指定時に読み込むworksheet名。省略時はactive sheet。]
///   戻り値: [Result<CsvDocument, ExcelError>: import済みdocument、またはbridge・protocol・document error。]
///   副作用: [csv_pathにCSV documentを作成する。]
/// }
pub fn import_workbook(
    input_path: impl AsRef<Path>,
    csv_path: impl AsRef<Path>,
    sheet_name: Option<&str>,
) -> Result<CsvDocument, ExcelError> {
    let request = json!({
        "input_path": input_path.as_ref(),
        "sheet_name": sheet_name,
    });
    let output = run_bridge("import", &request)?;
    let payload: Value =
        serde_json::from_slice(&output).map_err(|error| ExcelError::Protocol(error.to_string()))?;
    let rows = payload
        .get("rows")
        .cloned()
        .ok_or_else(|| ExcelError::Protocol("bridge response does not contain rows".into()))?;
    let rows = serde_json::from_value::<Vec<Vec<String>>>(rows)
        .map_err(|error| ExcelError::Protocol(error.to_string()))?;
    CsvDocument::create(csv_path, rows).map_err(ExcelError::Document)
}

#[derive(Debug)]
// {
//   責務: [BridgeLauncher: Excel bridgeの実行方法として同梱executableか開発用Python launcherを保持する。]
// }
enum BridgeLauncher {
    Bundled(PathBuf),
    Python(OsString),
}

// {
//   責務: [bundled_bridge_filename: OSに対応する同梱Excel bridge executable名を返す。]
//   戻り値: [&'static str: 現在のplatformでRowlyと同じdirectoryに置くbridge名。]
// }
fn bundled_bridge_filename() -> &'static str {
    if cfg!(windows) {
        "rowly-excel-bridge.exe"
    } else {
        "rowly-excel-bridge"
    }
}

// {
//   責務: [bundled_bridge_path: 現在のRowly executableと同じdirectoryにあるbridge pathを解決する。]
//   戻り値: [Result<PathBuf, ExcelError>: 同梱bridge path、またはcurrent executable path解決error。]
// }
fn bundled_bridge_path() -> Result<PathBuf, ExcelError> {
    let executable =
        env::current_exe().map_err(|error| ExcelError::ExecutablePath(error.to_string()))?;
    let directory = executable.parent().ok_or_else(|| {
        ExcelError::ExecutablePath(format!(
            "current executable has no parent directory: {}",
            executable.display()
        ))
    })?;
    Ok(directory.join(bundled_bridge_filename()))
}

// {
//   責務: [resolve_bridge_launcher: override・配布物・debug環境の順でbridge launcherを選択する。]
//   戻り値: [Result<BridgeLauncher, ExcelError>: 使用するlauncher、または実行path・配布backendの解決error。]
// }
fn resolve_bridge_launcher() -> Result<BridgeLauncher, ExcelError> {
    if let Some(python) = env::var_os("ROWLY_PYTHON") {
        return Ok(BridgeLauncher::Python(python));
    }

    let bundled = bundled_bridge_path()?;
    if bundled.is_file() {
        return Ok(BridgeLauncher::Bundled(bundled));
    }

    // Source checkout / cargo test 向けの開発 fallback。正式配布は release build と
    // sibling bridge を必須とし、ユーザー環境の Python へ暗黙 fallback しない。
    if cfg!(debug_assertions) {
        return Ok(BridgeLauncher::Python(OsString::from(DEFAULT_PYTHON)));
    }

    Err(ExcelError::BundledBackendMissing {
        path: bundled.display().to_string(),
    })
}

// {
//   責務: [configure_bridge_command: JSON protocol用のUTF-8標準streamをbridge commandへ設定する。]
//   引数: [command: 起動前のbridge process command。]
//   戻り値: [&mut Command: 設定後の同じcommand。]
// }
fn configure_bridge_command(command: &mut Command) -> &mut Command {
    command
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
}

// {
//   責務: [run_bridge: JSON requestを選択済みExcel bridgeへ送り、response bytesを返す。]
//   処理: [launcherを起動してUTF-8 JSONをstdinへ書き、終了statusを検査してstdoutを取得する。]
//   引数: [mode: bridgeへ渡すimportまたはexport mode。 request: bridgeへ送るJSON payload。]
//   戻り値: [Result<Vec<u8>, ExcelError>: 成功時のstdout bytes、または起動・protocol・bridge error。]
//   副作用: [外部bridge processを起動し、その標準streamを介してI/Oする。]
// }
fn run_bridge(mode: &str, request: &Value) -> Result<Vec<u8>, ExcelError> {
    let launcher = resolve_bridge_launcher()?;
    let mut command;
    let mut child = match launcher {
        BridgeLauncher::Bundled(executable) => {
            command = Command::new(&executable);
            command.arg(mode);
            configure_bridge_command(&mut command)
                .spawn()
                .map_err(|error| ExcelError::BundledBackendStart {
                    executable: executable.display().to_string(),
                    message: error.to_string(),
                })?
        }
        BridgeLauncher::Python(python) => {
            command = Command::new(&python);
            command.arg(BRIDGE_SCRIPT).arg(mode);
            configure_bridge_command(&mut command)
                .spawn()
                .map_err(|error| ExcelError::PythonStart {
                    executable: python.to_string_lossy().into_owned(),
                    message: error.to_string(),
                })?
        }
    };

    let input =
        serde_json::to_vec(request).map_err(|error| ExcelError::Protocol(error.to_string()))?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(&input)
        .map_err(|error| {
            ExcelError::Protocol(format!("failed to write bridge request: {error}"))
        })?;

    let output = child.wait_with_output().map_err(|error| {
        ExcelError::Protocol(format!("failed to wait for Excel bridge: {error}"))
    })?;

    if !output.status.success() {
        return Err(ExcelError::Bridge {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

#[derive(Debug, Error)]
/// {
///   責務: [ExcelError: Excel bridgeの起動・protocol・workbook操作およびCSV document作成の失敗を表す。]
/// }
pub enum ExcelError {
    #[error("failed to resolve current Rowly executable path: {0}")]
    ExecutablePath(String),

    #[error("bundled Excel backend is missing at `{path}`; reinstall the Rowly distribution")]
    BundledBackendMissing { path: String },

    #[error("failed to start bundled Excel backend `{executable}`: {message}")]
    BundledBackendStart { executable: String, message: String },

    #[error("failed to start Python executable `{executable}`: {message}")]
    PythonStart { executable: String, message: String },

    #[error("Excel bridge failed with status {status:?}: {stderr}")]
    Bridge { status: Option<i32>, stderr: String },

    #[error("invalid Excel bridge protocol: {0}")]
    Protocol(String),

    #[error(transparent)]
    Document(#[from] DocumentError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    // {
    //   責務: [sample_document: Excel adapter tests用CSV fixtureを一時directoryへ作成して開く。]
    //   戻り値: [(tempfile::TempDir, CsvDocument): fixture file lifetimeを保つ一時directoryとdocument。]
    //   副作用: [一時directoryとCSV fixture fileを作成する。]
    // }
    fn sample_document() -> (tempfile::TempDir, CsvDocument) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("source.csv");
        fs::write(&path, "Name,Value\nAlice,001\nFormula,=1+1\n日本語,田中\n").unwrap();
        let document = CsvDocument::open(path).unwrap();
        (directory, document)
    }

    #[test]
    // {
    //   責務: [bundled_bridge_uses_platform_executable_name: bridge executable名がOS別の配布名に一致することを確認する。]
    //   戻り値: [(): assertion成功時は値を返さない。]
    // }
    fn bundled_bridge_uses_platform_executable_name() {
        assert_eq!(
            bundled_bridge_filename(),
            if cfg!(windows) {
                "rowly-excel-bridge.exe"
            } else {
                "rowly-excel-bridge"
            }
        );
    }

    #[test]
    // {
    //   責務: [bundled_bridge_is_resolved_next_to_rowly_executable: bridge pathがRowly executableの隣を指すことを確認する。]
    //   戻り値: [(): assertion成功時は値を返さない。]
    // }
    fn bundled_bridge_is_resolved_next_to_rowly_executable() {
        let expected_parent = env::current_exe().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(
            bundled_bridge_path().unwrap(),
            expected_parent.join(bundled_bridge_filename())
        );
    }

    #[test]
    // {
    //   責務: [excel_round_trip_preserves_csv_strings: Excel export/import round tripで先頭zero・formula風文字列・日本語を保持することを確認する。]
    //   処理: [一時CSVをworkbookへexport後に再importし、cell text・encoding・dirty stateを検証する。]
    //   戻り値: [(): assertion成功時は値を返さない。]
    //   副作用: [一時directory内にCSVとExcel workbookを作成する。]
    // }
    fn excel_round_trip_preserves_csv_strings() {
        let (directory, document) = sample_document();
        let workbook = directory.path().join("export.xlsx");
        let imported_csv = directory.path().join("imported.csv");

        export_document(&document, &workbook, "データ").unwrap();
        let imported = import_workbook(&workbook, &imported_csv, Some("データ")).unwrap();

        assert_eq!(imported.cell_a1("A2").unwrap(), Some("Alice"));
        assert_eq!(imported.cell_a1("B2").unwrap(), Some("001"));
        assert_eq!(imported.cell_a1("B3").unwrap(), Some("=1+1"));
        assert_eq!(imported.cell_a1("B4").unwrap(), Some("田中"));
        assert_eq!(
            imported.source_encoding(),
            crate::process::SourceEncoding::Utf8
        );
        assert!(!imported.is_dirty());
    }

    #[test]
    // {
    //   責務: [import_uses_active_sheet_when_name_is_omitted: worksheet名を省略したimportがactive sheetを選ぶことを確認する。]
    //   戻り値: [(): assertion成功時は値を返さない。]
    //   副作用: [一時directory内にCSVとExcel workbookを作成する。]
    // }
    fn import_uses_active_sheet_when_name_is_omitted() {
        let (directory, document) = sample_document();
        let workbook = directory.path().join("export.xlsx");
        let imported_csv = directory.path().join("imported.csv");

        export_document(&document, &workbook, "Sheet1").unwrap();
        let imported = import_workbook(&workbook, &imported_csv, None).unwrap();

        assert_eq!(imported.cell_a1("A1").unwrap(), Some("Name"));
        assert_eq!(imported.cell_a1("B2").unwrap(), Some("001"));
    }

    #[test]
    // {
    //   責務: [missing_sheet_is_reported_as_bridge_error: 存在しないworksheet指定をbridge errorとして呼び出し元へ返すことを確認する。]
    //   戻り値: [(): assertion成功時は値を返さない。]
    //   副作用: [一時directory内にCSVとExcel workbookを作成する。]
    // }
    fn missing_sheet_is_reported_as_bridge_error() {
        let (directory, document) = sample_document();
        let workbook = directory.path().join("export.xlsx");

        export_document(&document, &workbook, "Data").unwrap();
        let error = import_workbook(
            &workbook,
            directory.path().join("imported.csv"),
            Some("Missing"),
        )
        .unwrap_err();

        assert!(matches!(error, ExcelError::Bridge { .. }));
        assert!(error.to_string().contains("Missing"));
    }
}
