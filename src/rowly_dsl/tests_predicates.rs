use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("predicates.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn string_predicates_can_be_used_directly_as_conditions() {
    let (_directory, mut document) = open("値\nold\n");
    run(
        r#"
            Let value = "東京都"
            If Contains(value, "東京") And StartsWith(value, "東") And EndsWith(value, "都") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "matched"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("matched"));
}

#[test]
fn is_japanese_matches_mixed_text_containing_japanese() {
    let (_directory, mut document) = open("値\nold\n");
    let report = run(
        r#"
            Let mixed = IsJapanese("abc日本語123")
            Let latin = IsJapanese("abc123")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("mixed"), Some("true"));
    assert_eq!(report.variable("latin"), Some("false"));
}

#[test]
fn type_predicates_accept_text_and_typed_values() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Let integerText = IsInteger("42")
            Let integerTyped = IsInteger(Integer("42"))
            Let decimalText = IsDecimal("12.5")
            Let decimalInteger = IsDecimal(Integer("12"))
            Let booleanText = IsBoolean("TRUE")
            Let badInteger = IsInteger("12.5")
            Let badBoolean = IsBoolean("yes")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("integertext"), Some("true"));
    assert_eq!(report.variable("integertyped"), Some("true"));
    assert_eq!(report.variable("decimaltext"), Some("true"));
    assert_eq!(report.variable("decimalinteger"), Some("true"));
    assert_eq!(report.variable("booleantext"), Some("true"));
    assert_eq!(report.variable("badinteger"), Some("false"));
    assert_eq!(report.variable("badboolean"), Some("false"));
}

#[test]
fn boolean_return_values_can_drive_if_conditions() {
    let (_directory, mut document) = open("値\nold\n");
    run(
        r#"
            Def Valid(value)
                Return IsInteger(value)
            End Def

            If Valid("123") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "ok"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("ok"));
}

#[test]
fn non_boolean_expression_condition_is_rejected() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            If "text" Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "x"
            End If
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("condition requires a Boolean value")
    );
}

#[test]
fn string_predicates_require_text_arguments() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Let value = Contains(Integer("12"), "1")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Contains first argument requires a text value")
    );
}

#[test]
fn predicate_argument_count_is_validated() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Let value = Contains("abc")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("function `Contains` expects 2 arguments but received 1")
    );
}
