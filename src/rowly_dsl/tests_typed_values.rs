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
    let path = directory.path().join("typed-values.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [integer_and_decimal_comparisons_are_numeric: Integerの大小比較が数値順になることと、Decimal/Integer比較を条件に使えることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn integer_and_decimal_comparisons_are_numeric() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR small = Integer("2")
            VAR large = Integer("10")
            VAR decimal = Decimal("10.5")

            If large > small And decimal > large Then
                VAR result = "numeric"
            Else
                VAR result = "bad"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("small"), Some("2"));
    assert_eq!(report.variable("large"), Some("10"));
    assert_eq!(report.variable("decimal"), Some("10.5"));
    assert_eq!(report.variable("result"), Some("numeric"));
}

#[test]
// {
//   責務: [quoted_values_remain_strings_until_explicitly_converted: quoted valueは明示変換まではString比較され、Integer変換後は数値比較される。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn quoted_values_remain_strings_until_explicitly_converted() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "10" < "2" Then
                VAR text_order = "yes"
            End If

            If Integer("10") > Integer("2") Then
                VAR numeric_order = "yes"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("text_order"), Some("yes"));
    assert_eq!(report.variable("numeric_order"), Some("yes"));
}

#[test]
// {
//   責務: [boolean_conversion_and_equality_work: Boolean変換のcase正規化とtyped Boolean同士の比較を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn boolean_conversion_and_equality_work() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR enabled = Boolean("true")
            VAR disabled = Boolean("FALSE")

            If enabled != disabled Then
                VAR result = String(enabled)
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("enabled"), Some("true"));
    assert_eq!(report.variable("disabled"), Some("false"));
    assert_eq!(report.variable("result"), Some("true"));
}

#[test]
// {
//   責務: [typed_values_can_be_written_to_cells_as_text: Decimal値をCSV cellへ文字列として書き込めることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn typed_values_can_be_written_to_cells_as_text() {
    let (_directory, mut document) = open("値\nold\n");
    run(
        r#"
            VAR value = Decimal("12.5")
            This.Worksheet.Editor.Cell(A2).Value.Set = value
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("12.5"));
}

#[test]
// {
//   責務: [ordered_comparison_rejects_boolean_and_string_mix: BooleanとStringの大小比較を拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn ordered_comparison_rejects_boolean_and_string_mix() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            If Boolean("true") > "false" Then
                VAR result = "bad"
            End If
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot compare Boolean and String")
    );
}

#[test]
// {
//   責務: [invalid_conversion_is_reported: 小数形式StringからIntegerへの不正変換を診断することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn invalid_conversion_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Integer("12.5")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot convert String value `12.5` to Integer")
    );
}
