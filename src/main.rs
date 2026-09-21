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

fn parse_command(arguments: &[OsString]) -> Result<CliCommand, String> {
    if arguments.len() == 1 {
        return Ok(CliCommand::Inspect {
            csv_path: PathBuf::from(&arguments[0]),
        });
    }

    if arguments.first().is_some_and(|value| value == OsStr::new("excel")) {
        return parse_excel_command(&arguments[1..]);
    }

    Err("expected one CSV path or an Excel subcommand".into())
}

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

fn os_string_to_utf8(value: &OsString) -> Result<String, String> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "sheet name must be valid UTF-8".into())
}

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

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn keeps_single_csv_path_compatibility() {
        assert_eq!(
            parse_command(&args(&["data.csv"])).unwrap(),
            CliCommand::Inspect {
                csv_path: PathBuf::from("data.csv")
            }
        );
    }

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

    #[test]
    fn rejects_invalid_argument_counts_and_excel_actions() {
        for values in [
            vec![],
            vec!["a.csv", "b.csv"],
            vec!["excel"],
            vec!["excel", "unknown"],
            vec!["excel", "import", "input.xlsx"],
            vec!["excel", "export", "input.csv", "output.xlsx", "Sheet1", "extra"],
        ] {
            assert!(parse_command(&args(&values)).is_err(), "{values:?}");
        }
    }
}
