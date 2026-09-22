use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("typed-values.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
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
