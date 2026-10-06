use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

// {
//   責務: [open: 条件テスト用CSVを一時作成し、documentと一時file保持用TempDirを返す。]
// }
fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("conditions.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [else_executes_when_condition_is_false: If条件がfalseのときElse側だけが実行されることを確認する。]
// }
fn else_executes_when_condition_is_false() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" = "b" Then
                VAR result = "then"
            Else
                VAR result = "else"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("else"));
}

#[test]
// {
//   責務: [logical_precedence_is_not_then_and_then_or: Not > And > Orの論理優先順位を確認する。]
// }
fn logical_precedence_is_not_then_and_then_or() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If Not "a" = "b" And "x" = "x" Or "z" = "q" Then
                VAR result = "yes"
            Else
                VAR result = "no"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("yes"));
}

#[test]
// {
//   責務: [and_and_or_short_circuit_rhs: And/Orが右辺の不要な評価とその副作用を省くことを確認する。]
// }
fn and_and_or_short_circuit_rhs() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" != "a" And missing = "boom" Then
                VAR first = "bad"
            Else
                VAR first = "ok"
            End If

            If "a" = "a" Or missing = "boom" Then
                VAR second = "ok"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("first"), Some("ok"));
    assert_eq!(report.variable("second"), Some("ok"));
}

#[test]
// {
//   責務: [comparison_operators_use_text_ordering: Text同士の順序比較が辞書順に基づくことを確認する。]
// }
fn comparison_operators_use_text_ordering() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" < "b" And "b" <= "b" And "c" > "b" And "c" >= "c" And "a" != "z" Then
                VAR result = "ordered"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("ordered"));
}

#[test]
// {
//   責務: [parenthesized_conditions_override_precedence: 括弧が論理演算子の既定優先順位を上書きすることを確認する。]
// }
fn parenthesized_conditions_override_precedence() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If ("a" = "b" Or "x" = "x") And Not ("q" = "q") Then
                VAR result = "bad"
            Else
                VAR result = "good"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("good"));
}

#[test]
// {
//   責務: [class_methods_use_the_same_condition_evaluator: class method内で同じ条件評価結果を使えることを確認する。]
// }
fn class_methods_use_the_same_condition_evaluator() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Choice
                Field value = "b"

                Def Pick()
                    If Self.value >= "b" And Not Self.value = "z" Then
                        Return "match"
                    Else
                        Return "fallback"
                    End If
                End Def
            End Class

            VAR choice = New Choice()
            VAR result = choice.Pick()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("match"));
}

#[test]
// {
//   責務: [else_supports_nested_if_blocks: Else body内のnested Ifで対象分岐だけを実行することを確認する。]
// }
fn else_supports_nested_if_blocks() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "outer" = "outer" Then
                If "inner" != "inner" Then
                    VAR result = "bad"
                Else
                    VAR result = "nested"
                End If
            Else
                VAR result = "bad"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("nested"));
}
