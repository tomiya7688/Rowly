use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("cell_value.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn cell_value_reads_existing_csv_text() {
    let (_directory, mut document) = open("名前,点数\n山田,42\n");
    let report = run(
        r#"
            Let name = CellValue("A2")
            Let score = CellValue("B2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("name"), Some("山田"));
    assert_eq!(report.variable("score"), Some("42"));
}

#[test]
fn cell_value_composes_with_predicates_and_conversions() {
    let (_directory, mut document) = open("名前,点数\n山田,42\n");

    run(
        r#"
            If IsJapanese(CellValue("A2")) And Integer(CellValue("B2")) >= Integer("40") Then
                This.Worksheet.Editor.Cell(B2).Value.Set = Integer(CellValue("B2")) + Integer("8")
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("B2").unwrap(), Some("50"));
}

#[test]
fn cell_value_reads_changes_made_earlier_in_same_script() {
    let (_directory, mut document) = open("値\nold\n");

    let report = run(
        r#"
            This.Worksheet.Editor.Cell(A2).Value.Set = "new"
            Let current = CellValue("A2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("current"), Some("new"));
}

#[test]
fn cell_value_rejects_non_text_reference() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Let value = CellValue(Integer("1"))
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
fn cell_value_reports_invalid_a1_reference() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Let value = CellValue("invalid")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("invalid"));
}

#[test]
fn cell_value_reports_missing_ragged_cell() {
    let (_directory, mut document) = open("a,b\nc\n");
    let error = run(
        r#"
            Let value = CellValue("B2")
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
