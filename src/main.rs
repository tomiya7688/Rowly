use std::{env, path::PathBuf, process::ExitCode};

use rowly::process::CsvDocument;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments.next().unwrap_or_default();
    let Some(path) = arguments.next() else {
        eprintln!("usage: {} <csv-path>", PathBuf::from(executable).display());
        return ExitCode::from(2);
    };

    if arguments.next().is_some() {
        eprintln!("error: expected exactly one CSV path");
        return ExitCode::from(2);
    }

    match CsvDocument::open(&path) {
        Ok(document) => {
            println!("path: {}", document.path().display());
            println!("source encoding: {}", document.source_encoding());
            println!("rows: {}", document.row_count());
            println!("columns: {}", document.column_count());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
