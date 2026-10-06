use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

// {
//   責務: [open: テスト用CSVを一時作成し、documentとTempDirを返してfile lifetimeを保つ。]
//   処理: [TempDirを作成しsourceをCSV fileへ書き込み、CsvDocumentとして開く。]
//   引数: [source: テストで開くCSVの初期内容。]
//   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
//   副作用: [一時ディレクトリにCSV fileを作成する。]
// }
fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("cell_value.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [cell_value_reads_existing_csv_text: CSV既存cellの文字列をCellValueで読み、report変数へ保持できることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_reads_existing_csv_text() {
    let (_directory, mut document) = open("名前,点数\n山田,42\n");
    let report = run(
        r#"
            VAR name = CellValue("A2")
            VAR score = CellValue("B2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("name"), Some("山田"));
    assert_eq!(report.variable("score"), Some("42"));
}

#[test]
// {
//   責務: [cell_value_composes_with_predicates_and_conversions: CellValueを日本語predicate・Integer変換と組み合わせてcell値を更新できることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_composes_with_predicates_and_conversions() {
    let (_directory, mut document) = open("名前,点数\n山田,42\n");

    run(
        r#"
            If Text.IsJapanese(CellValue("A2")) And Integer(CellValue("B2")) >= Integer("40") Then
                This.Worksheet.Editor.Cell(B2).Value.Set = Integer(CellValue("B2")) + Integer("8")
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("B2").unwrap(), Some("50"));
}

#[test]
// {
//   責務: [cell_value_reads_changes_made_earlier_in_same_script: 同じscript内で先に行ったcell編集を後続のCellValueが読み取ることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_reads_changes_made_earlier_in_same_script() {
    let (_directory, mut document) = open("値\nold\n");

    let report = run(
        r#"
            This.Worksheet.Editor.Cell(A2).Value.Set = "new"
            VAR current = CellValue("A2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("current"), Some("new"));
}

#[test]
// {
//   責務: [cell_value_rejects_non_text_reference: CellValueへString以外の参照引数を渡すと型診断になることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_rejects_non_text_reference() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = CellValue(Integer("1"))
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("CellValue argument requires a text value")
    );
}

#[test]
// {
//   責務: [cell_value_reports_invalid_a1_reference: 不正なA1参照をCellValueへ渡したときの診断を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_reports_invalid_a1_reference() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = CellValue("invalid")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("invalid"));
}

#[test]
// {
//   責務: [cell_value_reports_missing_ragged_cell: ragged CSVに存在しないcellをCellValueで参照したときの診断を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn cell_value_reports_missing_ragged_cell() {
    let (_directory, mut document) = open("a,b\nc\n");
    let error = run(
        r#"
            VAR value = CellValue("B2")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cell `B2` is outside the existing CSV table")
    );
}
