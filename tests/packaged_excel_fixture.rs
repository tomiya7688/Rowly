use std::{env, path::PathBuf};

use rowly::process::CsvDocument;

fn path_from_env(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

fn assert_fixture(path: PathBuf) {
    let document = CsvDocument::open(path).unwrap();

    assert_eq!(document.row_count(), 3);
    assert_eq!(document.column_count(), 6);
    assert_eq!(document.cell_a1("A1").unwrap(), Some("名前"));
    assert_eq!(document.cell_a1("D1").unwrap(), Some("空欄"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("𠮷野 😀"));
    assert_eq!(document.cell_a1("B2").unwrap(), Some("001"));
    assert_eq!(document.cell_a1("C2").unwrap(), Some("=1+1"));
    assert_eq!(document.cell_a1("D2").unwrap(), Some(""));
    assert_eq!(document.cell_a1("E2").unwrap(), Some("東京,大阪"));
    assert_eq!(document.cell_a1("F2").unwrap(), Some("true"));
    assert_eq!(document.cell_a1("C3").unwrap(), Some("=3+7"));
    assert_eq!(document.cell_a1("D3").unwrap(), Some(""));
    assert_eq!(document.cell_a1("E3").unwrap(), Some("1行目\n2行目"));
    assert_eq!(document.cell_a1("F3").unwrap(), Some("false"));
}

#[test]
#[ignore = "配布相当成果物を生成した CI から明示実行する"]
fn packaged_fixture_matches_expected_rowly_values() {
    assert_fixture(path_from_env("ROWLY_IMPORTED_FIXTURE"));
    assert_fixture(path_from_env("ROWLY_ROUNDTRIP_FIXTURE"));
}
