use std::{env, path::PathBuf};

use rowly::process::CsvDocument;

// {
//   責務: [path_from_env: 指定environment variableから配布fixture pathを取得する。]
//   引数: [name: 必須environment variable名。]
//   戻り値: [PathBuf: 指定されたfixture file path。]
//   エラー: [variableが未設定なら必要名を示してtestを失敗させる。]
// }
fn path_from_env(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

// {
//   責務: [assert_fixture: packaged Excel import/export fixtureが期待する6列のCSV値を保持することを検証する。]
//   引数: [path: 検証対象の生成済みCSV fixture。]
//   戻り値: [(): assertion成功時は値を返さない。]
//   エラー: [file openまたは期待するrow・column・cell値が一致しない場合はtestを失敗させる。]
// }
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
// {
//   責務: [packaged_fixture_matches_expected_rowly_values: 配布XLSXから直接importしたCSVとround-trip後のCSVを同じ期待値で検証する。]
//   処理: [CIが渡す二つのfixture pathを読み込み、各CSVのrow・column・cell内容を検査する。]
//   戻り値: [(): assertion成功時は値を返さない。]
//   エラー: [fixture pathが未設定またはCSV内容が期待値と異なる場合はtestを失敗させる。]
// }
fn packaged_fixture_matches_expected_rowly_values() {
    assert_fixture(path_from_env("ROWLY_IMPORTED_FIXTURE"));
    assert_fixture(path_from_env("ROWLY_ROUNDTRIP_FIXTURE"));
}
