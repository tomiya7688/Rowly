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
    let path = directory.path().join("header_access.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [column_index_returns_one_based_position: ColumnIndexがheader位置を1-basedの数値で返すことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn column_index_returns_one_based_position() {
    let (_directory, mut document) = open("ID,名前,状態\n1,山田,\n");
    let report = run(
        r#"
            VAR nameColumn = ColumnIndex("名前")
            VAR statusColumn = ColumnIndex("状態")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("namecolumn"), Some("2"));
    assert_eq!(report.variable("statuscolumn"), Some("3"));
}

#[test]
// {
//   責務: [header_access_integrates_with_row_loop_and_predicates: header指定のcell読取・日本語predicate・cell更新をrow loop内で組み合わせる。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn header_access_integrates_with_row_loop_and_predicates() {
    let (_directory, mut document) = open("ID,名前,状態\n1,山田,\n2,Alice,\n3,田中,\n");

    run(
        r#"
            For row = Integer("2") To RowCount()
                If Text.IsJapanese(CellValueByHeader(row, "名前")) Then
                    SetCellValueByHeader(row, "状態", "日本語")
                Else
                    SetCellValueByHeader(row, "状態", "その他")
                End If
            Next row
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("C2").unwrap(), Some("日本語"));
    assert_eq!(document.cell_a1("C3").unwrap(), Some("その他"));
    assert_eq!(document.cell_a1("C4").unwrap(), Some("日本語"));
}

#[test]
// {
//   責務: [header_access_sees_prior_edits_in_same_script: 同一scriptで先に行ったheader指定cell編集を後続の読取が反映することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn header_access_sees_prior_edits_in_same_script() {
    let (_directory, mut document) = open("名前\nold\n");
    let report = run(
        r#"
            SetCellValueByHeader(Integer("2"), "名前", "山田")
            VAR current = CellValueByHeader(Integer("2"), "名前")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("current"), Some("山田"));
}

#[test]
// {
//   責務: [missing_header_is_reported: 存在しないheader名での読取を診断することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn missing_header_is_reported() {
    let (_directory, mut document) = open("名前\n山田\n");
    let error = run(
        r#"
            VAR value = CellValueByHeader(Integer("2"), "状態")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("was not found"));
}

#[test]
// {
//   責務: [ambiguous_header_is_reported: 重複header名のColumnIndex参照を曖昧として拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn ambiguous_header_is_reported() {
    let (_directory, mut document) = open("名前,名前\n山田,田中\n");
    let error = run(
        r#"
            VAR column = ColumnIndex("名前")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("ambiguous"));
}

#[test]
// {
//   責務: [header_argument_must_be_text: ColumnIndexへString以外のheader引数を渡したときの型診断を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn header_argument_must_be_text() {
    let (_directory, mut document) = open("名前\n山田\n");
    let error = run(
        r#"
            VAR column = ColumnIndex(Integer("1"))
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("ColumnIndex header requires a text value")
    );
}

#[test]
// {
//   責務: [missing_ragged_cell_by_header_is_reported: ragged CSVの欠落cellをheader指定で参照したときの診断を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn missing_ragged_cell_by_header_is_reported() {
    let (_directory, mut document) = open("名前,状態\n山田\n");
    let error = run(
        r#"
            VAR value = CellValueByHeader(Integer("2"), "状態")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("row 2, header 状態"));
}
