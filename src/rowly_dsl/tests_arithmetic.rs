use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

// {
//   責務: [open: 算術テスト用CSVを一時作成し、documentとfile lifetime保持用TempDirを返す。]
//   処理: [TempDirを作成しsourceをCSV fileへ書き込み、CsvDocumentとして開く。]
//   引数: [source: テストで開くCSVの初期内容。]
//   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
//   副作用: [一時ディレクトリにCSV fileを作成する。]
// }
fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("arithmetic.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [arithmetic_precedence_and_parentheses_work: 乗算の優先順位と括弧による変更を変数の評価結果で確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn arithmetic_precedence_and_parentheses_work() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR two = Integer("2")
            VAR three = Integer("3")
            VAR four = Integer("4")
            VAR first = two + three * four
            VAR second = (two + three) * four
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("first"), Some("14"));
    assert_eq!(report.variable("second"), Some("20"));
}

#[test]
// {
//   責務: [subtraction_is_left_associative_and_unary_minus_works: 減算の左結合と単項マイナスを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn subtraction_is_left_associative_and_unary_minus_works() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR ten = Integer("10")
            VAR three = Integer("3")
            VAR two = Integer("2")
            VAR result = ten - three - two
            VAR negative = -result
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("5"));
    assert_eq!(report.variable("negative"), Some("-5"));
}

#[test]
// {
//   責務: [mixed_numeric_arithmetic_promotes_to_decimal: Integer + DecimalとDecimal * Integerの結果がDecimalになることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn mixed_numeric_arithmetic_promotes_to_decimal() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR integer = Integer("5")
            VAR decimal = Decimal("2.5")
            VAR sum = integer + decimal
            VAR product = decimal * integer
            VAR quotient = integer / Integer("2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("sum"), Some("7.5"));
    assert_eq!(report.variable("product"), Some("12.5"));
    assert_eq!(report.variable("quotient"), Some("2.5"));
}

#[test]
// {
//   責務: [arithmetic_can_feed_comparisons_and_csv_edits: 算術結果を比較条件とCSV編集へ渡せることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn arithmetic_can_feed_comparisons_and_csv_edits() {
    let (_directory, mut document) = open("値\nold\n");
    let report = run(
        r#"
            VAR left = Integer("6")
            VAR right = Integer("4")
            VAR total = left + right

            If total >= Integer("10") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = total / Integer("4")
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("total"), Some("10"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("2.5"));
}

#[test]
// {
//   責務: [function_and_method_results_participate_in_arithmetic: functionとmethodの戻り値を算術operandに使えることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn function_and_method_results_participate_in_arithmetic() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Def Double(value)
                Return value * Integer("2")
            End Def

            Class Counter
                Field value = Integer("3")

                Def Next()
                    Return Self.value + Integer("1")
                End Def
            End Class

            VAR counter = New Counter()
            VAR result = Double(counter.Next()) + Integer("1")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("9"));
}

#[test]
// {
//   責務: [division_by_zero_is_explicit: 0除算時のerror診断メッセージを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn division_by_zero_is_explicit() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Integer("10") / Integer("0")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("division by zero"));
}

#[test]
// {
//   責務: [arithmetic_rejects_non_numeric_values: Stringを二項算術operandにした際の型error診断を確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn arithmetic_rejects_non_numeric_values() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = "10" + Integer("2")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot apply arithmetic operator `+` to String and Integer")
    );
}

#[test]
// {
//   責務: [unary_minus_rejects_non_numeric_values: String値への単項マイナスを拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn unary_minus_rejects_non_numeric_values() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = -"10"
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot apply unary operator `-` to String")
    );
}

#[test]
// {
//   責務: [integer_overflow_is_explicit: Integer加算overflow時のerror診断メッセージを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn integer_overflow_is_explicit() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR max = Integer("9223372036854775807")
            VAR value = max + Integer("1")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("arithmetic overflow"));
}
