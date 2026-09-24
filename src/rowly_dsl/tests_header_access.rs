use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("header_access.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
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
