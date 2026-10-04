use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    process::ExitCode,
};

use rowly::{
    excel_python::{export_document, import_workbook},
    process::CsvDocument,
};

const DEFAULT_EXPORT_SHEET: &str = "Sheet1";

// {
//   責務: [
//     CliCommand: CLI引数を実行可能なcommandとして表す
//   ]
//   フィールド: [
//     Inspect: 調査するCSV path
//     ExcelImport: 入力XLSX、出力CSV、任意のsheet名
//     ExcelExport: 入力CSV、出力XLSX、sheet名
//   ]
// }
#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Inspect {
        csv_path: PathBuf,
    },
    ExcelImport {
        xlsx_path: PathBuf,
        csv_path: PathBuf,
        sheet_name: Option<String>,
    },
    ExcelExport {
        csv_path: PathBuf,
        xlsx_path: PathBuf,
        sheet_name: String,
    },
}

// {
//   責務: [
//     main: CLI引数を検証し、実行結果をprocess exit codeへ変換する
//   ]
//   処理: [
//     1: 実行ファイル名とOS形式の引数を取得する
//     2: 引数からcommandを構築し、不正ならusageを表示する
//     3: commandを実行して成功・失敗のexit codeを返す
//   ]
//   引数: []
//   戻り値: [
//     ExitCode: CLI実行の成否を示すprocess exit code
//   ]
// }
fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments.next().unwrap_or_default();
    let arguments = arguments.collect::<Vec<_>>();

    let command = match parse_command(&arguments) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("error: {message}");
            print_usage(&executable);
            return ExitCode::from(2);
        }
    };

    match execute(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

// {
//   責務: [
//     parse_command: top-level CLI引数をtyped commandへ変換する
//   ]
//   処理: [
//     1: 単一pathならCSV調査commandを構築する
//     2: excel subcommandならExcel用parserへ渡す
//     3: 対応しない引数列をerrorにする
//   ]
//   引数: [
//     arguments: 実行ファイル名を除いたOS形式のCLI引数
//   ]
//   戻り値: [
//     CliCommand: 実行対象command
//     String: 引数形式が不正な場合の説明
//   ]
// }
fn parse_command(arguments: &[OsString]) -> Result<CliCommand, String> {
    if arguments.len() == 1 {
        return Ok(CliCommand::Inspect {
            csv_path: PathBuf::from(&arguments[0]),
        });
    }

    if arguments
        .first()
        .is_some_and(|value| value == OsStr::new("excel"))
    {
        return parse_excel_command(&arguments[1..]);
    }

    Err("expected one CSV path or an Excel subcommand".into())
}

// {
//   責務: [
//     parse_excel_command: Excel import / export引数をtyped commandへ変換する
//   ]
//   処理: [
//     1: actionごとに引数数を検証する
//     2: optional sheet名をUTF-8へ変換し、exportの省略値を補う
//     3: 未知のactionや不正な引数をerrorにする
//   ]
//   引数: [
//     arguments: excel action以降のOS形式の引数
//   ]
//   戻り値: [
//     CliCommand: 検証済みのExcel command
//     String: 引数またはsheet名が不正な場合の説明
//   ]
// }
fn parse_excel_command(arguments: &[OsString]) -> Result<CliCommand, String> {
    let Some(action) = arguments.first() else {
        return Err("expected `excel import` or `excel export`".into());
    };

    if action == OsStr::new("import") {
        if !(3..=4).contains(&arguments.len()) {
            return Err("excel import expects <xlsx-path> <csv-path> [sheet-name]".into());
        }
        let sheet_name = arguments.get(3).map(os_string_to_utf8).transpose()?;
        return Ok(CliCommand::ExcelImport {
            xlsx_path: PathBuf::from(&arguments[1]),
            csv_path: PathBuf::from(&arguments[2]),
            sheet_name,
        });
    }

    if action == OsStr::new("export") {
        if !(3..=4).contains(&arguments.len()) {
            return Err("excel export expects <csv-path> <xlsx-path> [sheet-name]".into());
        }
        let sheet_name = arguments
            .get(3)
            .map(os_string_to_utf8)
            .transpose()?
            .unwrap_or_else(|| DEFAULT_EXPORT_SHEET.to_owned());
        return Ok(CliCommand::ExcelExport {
            csv_path: PathBuf::from(&arguments[1]),
            xlsx_path: PathBuf::from(&arguments[2]),
            sheet_name,
        });
    }

    Err("expected `excel import` or `excel export`".into())
}

// {
//   責務: [
//     os_string_to_utf8: OS引数をUTF-8の所有Stringへ変換する
//   ]
//   処理: [
//     1: UTF-8として参照できる場合は内容を複製する
//     2: 変換できない場合はsheet名のerrorを返す
//   ]
//   引数: [
//     value: 変換するOS文字列
//   ]
//   戻り値: [
//     String: UTF-8へ変換した文字列、または失敗理由
//   ]
// }
fn os_string_to_utf8(value: &OsString) -> Result<String, String> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "sheet name must be valid UTF-8".into())
}

// {
//   責務: [
//     execute: typed CLI commandを実行し、利用者向け結果を出力する
//   ]
//   処理: [
//     1: CSVを調査するかXLSXをimport / exportする
//     2: 成功結果と対象情報を標準出力へ表示する
//     3: adapterまたはCSV処理のerrorを呼び出し元へ返す
//   ]
//   引数: [
//     command: 実行するtyped CLI command
//   ]
//   戻り値: [
//     (): commandの実行結果
//     String: CSVまたはExcel処理が失敗した理由
//   ]
//   副作用: [
//     CSV / XLSX fileを読み書きし、結果を標準出力へ表示する
//   ]
// }
fn execute(command: CliCommand) -> Result<(), String> {
    match command {
        CliCommand::Inspect { csv_path } => {
            let document = CsvDocument::open(&csv_path).map_err(|error| error.to_string())?;
            println!("path: {}", document.path().display());
            println!("source encoding: {}", document.source_encoding());
            println!("rows: {}", document.row_count());
            println!("columns: {}", document.column_count());
        }
        CliCommand::ExcelImport {
            xlsx_path,
            csv_path,
            sheet_name,
        } => {
            let document = import_workbook(&xlsx_path, &csv_path, sheet_name.as_deref())
                .map_err(|error| error.to_string())?;
            println!("imported: {}", xlsx_path.display());
            println!("csv: {}", document.path().display());
            println!("rows: {}", document.row_count());
            println!("columns: {}", document.column_count());
        }
        CliCommand::ExcelExport {
            csv_path,
            xlsx_path,
            sheet_name,
        } => {
            let document = CsvDocument::open(&csv_path).map_err(|error| error.to_string())?;
            export_document(&document, &xlsx_path, &sheet_name)
                .map_err(|error| error.to_string())?;
            println!("csv: {}", document.path().display());
            println!("exported: {}", xlsx_path.display());
            println!("sheet: {sheet_name}");
            println!("rows: {}", document.row_count());
            println!("columns: {}", document.column_count());
        }
    }
    Ok(())
}

// {
//   責務: [
//     print_usage: CLIの利用形式を標準エラーへ表示する
//   ]
//   処理: [
//     1: executable pathを表示用文字列に変換する
//     2: CSV調査とExcel import / exportのusageを出力する
//   ]
//   引数: [
//     executable: 表示する実行ファイル名またはpath
//   ]
//   戻り値: [
//     (): usage表示後に値を返さない
//   ]
//   副作用: [
//     標準エラーへusageを出力する
//   ]
// }
fn print_usage(executable: &OsString) {
    let executable = PathBuf::from(executable);
    let executable = executable.display();
    eprintln!("usage:");
    eprintln!("  {executable} <csv-path>");
    eprintln!("  {executable} excel import <xlsx-path> <csv-path> [sheet-name]");
    eprintln!("  {executable} excel export <csv-path> <xlsx-path> [sheet-name]");
}

#[cfg(test)]
mod tests {
    use super::*;

    // {
    //   責務: [
    //     args: parser用の文字列sliceをOS形式の引数列にする
    //   ]
    //   処理: [
    //     1: 各文字列をOsStringへ変換する
    //   ]
    //   引数: [
    //     values: parserへ渡す引数文字列
    //   ]
    //   戻り値: [
    //     Vec<OsString>: CLI parserへ渡せる引数列
    //   ]
    // }
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    // {
    //   責務: [
    //     keeps_single_csv_path_compatibility: 単一CSV pathの従来形式を維持する
    //   ]
    //   処理: [
    //     1: 単一pathがInspect commandになることを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn keeps_single_csv_path_compatibility() {
        assert_eq!(
            parse_command(&args(&["data.csv"])).unwrap(),
            CliCommand::Inspect {
                csv_path: PathBuf::from("data.csv")
            }
        );
    }

    // {
    //   責務: [
    //     path_named_excel_keeps_single_path_compatibility: excelというpathを通常CSV pathとして扱う
    //   ]
    //   処理: [
    //     1: 単一のexcel引数がInspect commandになることを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn path_named_excel_keeps_single_path_compatibility() {
        assert_eq!(
            parse_command(&args(&["excel"])).unwrap(),
            CliCommand::Inspect {
                csv_path: PathBuf::from("excel")
            }
        );
    }

    // {
    //   責務: [
    //     parses_excel_import_with_optional_sheet: Excel importと任意sheet名をparseする
    //   ]
    //   処理: [
    //     1: sheet名を指定したimport commandを確認する
    //     2: sheet名を省略したimport commandを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn parses_excel_import_with_optional_sheet() {
        assert_eq!(
            parse_command(&args(&[
                "excel",
                "import",
                "input.xlsx",
                "output.csv",
                "データ"
            ]))
            .unwrap(),
            CliCommand::ExcelImport {
                xlsx_path: PathBuf::from("input.xlsx"),
                csv_path: PathBuf::from("output.csv"),
                sheet_name: Some("データ".into()),
            }
        );

        assert_eq!(
            parse_command(&args(&["excel", "import", "input.xlsx", "output.csv"])).unwrap(),
            CliCommand::ExcelImport {
                xlsx_path: PathBuf::from("input.xlsx"),
                csv_path: PathBuf::from("output.csv"),
                sheet_name: None,
            }
        );
    }

    // {
    //   責務: [
    //     parses_excel_export_with_default_and_explicit_sheet: exportでsheet名省略と明示をparseする
    //   ]
    //   処理: [
    //     1: sheet名省略時に既定値となることを確認する
    //     2: sheet名を指定したcommandを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn parses_excel_export_with_default_and_explicit_sheet() {
        assert_eq!(
            parse_command(&args(&["excel", "export", "input.csv", "output.xlsx"])).unwrap(),
            CliCommand::ExcelExport {
                csv_path: PathBuf::from("input.csv"),
                xlsx_path: PathBuf::from("output.xlsx"),
                sheet_name: DEFAULT_EXPORT_SHEET.into(),
            }
        );

        assert_eq!(
            parse_command(&args(&[
                "excel",
                "export",
                "input.csv",
                "output.xlsx",
                "帳票"
            ]))
            .unwrap(),
            CliCommand::ExcelExport {
                csv_path: PathBuf::from("input.csv"),
                xlsx_path: PathBuf::from("output.xlsx"),
                sheet_name: "帳票".into(),
            }
        );
    }

    // {
    //   責務: [
    //     rejects_invalid_argument_counts_and_excel_actions: 不正な引数数とExcel actionを拒否する
    //   ]
    //   処理: [
    //     1: 各不正入力がparse errorになることを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn rejects_invalid_argument_counts_and_excel_actions() {
        for values in [
            vec![],
            vec!["a.csv", "b.csv"],
            vec!["excel", "unknown"],
            vec!["excel", "import", "input.xlsx"],
            vec![
                "excel",
                "export",
                "input.csv",
                "output.xlsx",
                "Sheet1",
                "extra",
            ],
        ] {
            assert!(parse_command(&args(&values)).is_err(), "{values:?}");
        }
    }
}
