use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use rowly::process::CsvDocument;
use tempfile::tempdir;

// 親側の非 UTF-8 設定を子 bridge が引き継がないことも検査する。
// テストプロセス自身の環境を変更しないため、他のテストと並列実行できる。
fn cli(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rowly"));
    command
        .current_dir(directory)
        .env("PYTHONUTF8", "0")
        .env("PYTHONIOENCODING", "ascii");
    command
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("CLI output must be UTF-8")
}

#[test]
fn csv_inspection_accepts_unicode_relative_paths_and_keeps_crlf_input_unchanged() {
    let directory = tempdir().unwrap();
    let working = directory.path().join("日本語 作業用");
    fs::create_dir(&working).unwrap();
    let path = working.join("入力 データ.csv");
    let source = "名前,値\r\n田中,001\r\n";
    fs::write(&path, source).unwrap();

    let stdout = assert_success(cli(&working).arg("入力 データ.csv").output().unwrap());

    assert!(stdout.contains("入力 データ.csv"));
    assert!(stdout.contains("rows: 2"));
    assert!(stdout.contains("columns: 2"));
    assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
}

#[test]
fn excel_cli_round_trip_preserves_unicode_values_paths_and_sheet_names() {
    let directory = tempdir().unwrap();
    let working = directory.path().join("日本語 帳票");
    fs::create_dir(&working).unwrap();
    let input = "入力 データ.csv";
    let workbook = "出力 データ.xlsx";
    let output = "復元 データ.csv";
    let sheet = "日本語 シート";
    let source = "名前,コード,式,備考\r\n𠮷野 😀,001,=1+1,\"東京,大阪\"\r\n田中,002,=3+7,\"1行目\n2行目\"\r\n";
    fs::write(working.join(input), source).unwrap();

    let exported = assert_success(
        cli(&working)
            .args(["excel", "export", input, workbook, sheet])
            .output()
            .unwrap(),
    );
    assert!(exported.contains("sheet: 日本語 シート"));
    assert!(working.join(workbook).is_file());
    let imported = assert_success(
        cli(&working)
            .args(["excel", "import", workbook, output, sheet])
            .output()
            .unwrap(),
    );
    assert!(imported.contains("rows: 3"));
    assert!(imported.contains("columns: 4"));

    let original = CsvDocument::open(working.join(input)).unwrap();
    let restored = CsvDocument::open(working.join(output)).unwrap();
    assert_eq!(
        original.rows().collect::<Vec<_>>(),
        restored.rows().collect::<Vec<_>>()
    );
    assert_eq!(restored.cell_a1("A2").unwrap(), Some("𠮷野 😀"));
    assert_eq!(restored.cell_a1("B2").unwrap(), Some("001"));
    assert_eq!(restored.cell_a1("C2").unwrap(), Some("=1+1"));
    assert_eq!(restored.cell_a1("D3").unwrap(), Some("1行目\n2行目"));
    let saved = fs::read_to_string(working.join(output)).unwrap();
    assert!(!saved.contains('\r'));
    assert_eq!(fs::read(working.join(input)).unwrap(), source.as_bytes());
}

#[test]
fn excel_cli_default_sheet_and_active_sheet_round_trip() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("input.csv"), "value\n001\n").unwrap();
    let stdout = assert_success(
        cli(directory.path())
            .args(["excel", "export", "input.csv", "output.xlsx"])
            .output()
            .unwrap(),
    );
    assert!(stdout.contains("sheet: Sheet1"));
    assert_success(
        cli(directory.path())
            .args(["excel", "import", "output.xlsx", "restored.csv"])
            .output()
            .unwrap(),
    );
    let restored = CsvDocument::open(directory.path().join("restored.csv")).unwrap();
    assert_eq!(restored.cell_a1("A2").unwrap(), Some("001"));
}

#[test]
fn missing_python_override_is_reported_without_creating_output() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("input.csv"), "value\n1\n").unwrap();
    let missing = directory.path().join("未存在 Python.exe");
    let output = cli(directory.path())
        .env("ROWLY_PYTHON", &missing)
        .args(["excel", "export", "input.csv", "output.xlsx"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to start Python executable"));
    assert!(stderr.contains("未存在 Python.exe"));
    assert!(!directory.path().join("output.xlsx").exists());
    assert_eq!(
        fs::read_to_string(directory.path().join("input.csv")).unwrap(),
        "value\n1\n"
    );
}

#[test]
fn invalid_arguments_return_usage_exit_code_on_both_platforms() {
    let directory = tempdir().unwrap();
    let output = cli(directory.path())
        .args(["excel", "export", "input.csv"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr).unwrap().contains("usage:"));
}
