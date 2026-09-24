use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("for_loop.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn for_loop_walks_rows_and_updates_dynamic_cells() {
    let (_directory, mut document) = open("名前,状態\n山田,\nAlice,\n田中,\n");

    run(
        r#"
            For row = Integer("2") To RowCount()
                If Text.IsJapanese(CellValueAt(row, Integer("1"))) Then
                    SetCellValueAt(row, Integer("2"), "日本語")
                Else
                    SetCellValueAt(row, Integer("2"), "その他")
                End If
            Next row
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("B2").unwrap(), Some("日本語"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("その他"));
    assert_eq!(document.cell_a1("B4").unwrap(), Some("日本語"));
}

#[test]
fn row_and_column_count_are_one_based_loop_friendly() {
    let (_directory, mut document) = open("a,b,c\n1,2,3\n");
    let report = run(
        r#"
            VAR rows = RowCount()
            VAR columns = ColumnCount()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("rows"), Some("2"));
    assert_eq!(report.variable("columns"), Some("3"));
}

#[test]
fn descending_for_loop_and_step_work() {
    let (_directory, mut document) = open("値\na\nb\nc\nd\ne\n");

    run(
        r#"
            For row = RowCount() To Integer("2") Step -Integer("2")
                SetCellValueAt(row, Integer("1"), "x")
            Next
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("b"));
    assert_eq!(document.cell_a1("A4").unwrap(), Some("x"));
    assert_eq!(document.cell_a1("A5").unwrap(), Some("d"));
    assert_eq!(document.cell_a1("A6").unwrap(), Some("x"));
}

#[test]
fn nested_for_loops_can_visit_rectangular_cells() {
    let (_directory, mut document) = open("a,b\n1,2\n");

    run(
        r#"
            For row = Integer("1") To RowCount()
                For column = Integer("1") To ColumnCount()
                    SetCellValueAt(row, column, "x")
                Next column
            Next row
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A1").unwrap(), Some("x"));
    assert_eq!(document.cell_a1("B1").unwrap(), Some("x"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
    assert_eq!(document.cell_a1("B2").unwrap(), Some("x"));
}

#[test]
fn loop_variable_does_not_leak_after_loop() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            For row = Integer("1") To Integer("2")
                VAR current = row
            Next row
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("row"), None);
    assert_eq!(report.variable("current"), None);
}

#[test]
fn zero_step_is_rejected() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            For row = Integer("1") To Integer("2") Step Integer("0")
            Next row
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("Step cannot be zero"));
}

#[test]
fn dynamic_cell_indices_must_be_positive_integers() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = CellValueAt(Integer("0"), Integer("1"))
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("must be at least 1"));
}

#[test]
fn mismatched_next_variable_is_parse_error() {
    let error = parse(
        r#"
            For row = Integer("1") To Integer("2")
            Next column
        "#,
    )
    .unwrap_err();

    assert!(error.to_string().contains("does not match For variable"));
}
