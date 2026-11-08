use tempfile::{TempDir, tempdir};

use super::{SortDirection, SortKey, SortKind, reject_multi_source};
use crate::process::{CsvDocument, DocumentError, ValidationRule, ValidationTarget};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// {
// 責務: [
// document: 指定行から未編集のCSV documentを作る
// ]
// 引数: [
// rows: &[&[&str]]
// ]
// 戻り値: [
// Result<(TempDir, CsvDocument), Box<dyn Error>>: 一時directoryと未編集document
// ]
// }
fn document(rows: &[&[&str]]) -> Result<(TempDir, CsvDocument), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let rows = rows
        .iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect();
    let document = CsvDocument::create(directory.path().join("t.csv"), rows)?;
    Ok((directory, document))
}

// {
// 責務: [
// key: 列・方向・方式からSortKeyを作る
// ]
// 引数: [
// column: usize
// direction: SortDirection
// kind: SortKind
// ]
// 戻り値: [
// SortKey: 作ったkey
// ]
// }
fn key(column: usize, direction: SortDirection, kind: SortKind) -> SortKey {
    SortKey {
        column,
        direction,
        kind,
    }
}

// {
// 責務: [
// column_values: 指定列のデータ行の値を上から集める
// ]
// 引数: [
// doc: &CsvDocument
// column: usize
// ]
// 戻り値: [
// Vec<String>: header行を除く値
// ]
// }
fn column_values(doc: &CsvDocument, column: usize) -> Vec<String> {
    (1..doc.row_count())
        .map(|row| doc.cell(row, column).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn planning_does_not_change_the_document() -> TestResult {
    let (_d, doc) = document(&[&["n"], &["b"], &["a"]])?;
    let before = doc.csv_text()?;
    let plan = doc.plan_sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert_eq!(plan.order(), &[2, 1]);
    assert_eq!(plan.moved_count(), 2);
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn apply_reorders_data_rows_and_keeps_the_header_in_one_undo() -> TestResult {
    let (_d, mut doc) = document(&[&["n", "v"], &["b", "1"], &["c", "2"], &["a", "3"]])?;
    let moved = doc.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert_eq!(moved, 3);
    assert_eq!(doc.cell(0, 0), Some("n"));
    assert_eq!(column_values(&doc, 0), ["a", "b", "c"]);
    assert_eq!(column_values(&doc, 1), ["3", "1", "2"]);
    assert!(doc.is_dirty());
    assert!(doc.undo()?);
    assert_eq!(column_values(&doc, 0), ["b", "c", "a"]);
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn descending_reverses_and_ties_keep_the_original_order() -> TestResult {
    let (_d, mut doc) = document(&[
        &["k", "id"],
        &["x", "1"],
        &["y", "2"],
        &["x", "3"],
        &["y", "4"],
    ])?;
    doc.sort_rows(&[key(0, SortDirection::Descending, SortKind::Raw)])?;
    assert_eq!(column_values(&doc, 1), ["2", "4", "1", "3"]);
    Ok(())
}

#[test]
fn multiple_keys_apply_in_priority_order() -> TestResult {
    let (_d, mut doc) = document(&[
        &["s", "d"],
        &["b", "1"],
        &["a", "1"],
        &["b", "2"],
        &["a", "2"],
    ])?;
    doc.sort_rows(&[
        key(0, SortDirection::Ascending, SortKind::Raw),
        key(1, SortDirection::Descending, SortKind::Raw),
    ])?;
    assert_eq!(column_values(&doc, 0), ["a", "a", "b", "b"]);
    assert_eq!(column_values(&doc, 1), ["2", "1", "2", "1"]);
    Ok(())
}

#[test]
fn raw_compare_does_not_infer_numbers_but_number_compare_does() -> TestResult {
    let (_d, mut raw) = document(&[&["n"], &["10"], &["9"], &["100"]])?;
    raw.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert_eq!(column_values(&raw, 0), ["10", "100", "9"]);

    let (_d2, mut number) = document(&[&["n"], &["10"], &["9"], &["100"]])?;
    number.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Number)])?;
    assert_eq!(column_values(&number, 0), ["9", "10", "100"]);
    Ok(())
}

#[test]
fn unreadable_numbers_stay_at_the_end_in_both_directions() -> TestResult {
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        let (_d, mut doc) = document(&[&["n"], &["x"], &["2"], &[""], &["NaN"], &["1"], &["inf"]])?;
        doc.sort_rows(&[key(0, direction, SortKind::Number)])?;
        let values = column_values(&doc, 0);
        let expected_head = match direction {
            SortDirection::Ascending => ["1", "2"],
            SortDirection::Descending => ["2", "1"],
        };
        assert_eq!(&values[..2], &expected_head);
        // 末尾の未解決グループは元の順を保つ
        assert_eq!(&values[2..], ["x", "", "NaN", "inf"]);
    }
    Ok(())
}

#[test]
fn already_sorted_input_records_nothing() -> TestResult {
    let (_d, mut doc) = document(&[&["n"], &["a"], &["b"]])?;
    let moved = doc.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert_eq!(moved, 0);
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn no_rows_are_lost_or_added() -> TestResult {
    let (_d, mut doc) = document(&[
        &["n", "m"],
        &["c", "1"],
        &["a", "2"],
        &["b", "3"],
        &["a", "4"],
    ])?;
    let before = doc.row_count();
    doc.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert_eq!(doc.row_count(), before);
    let mut values = column_values(&doc, 1);
    values.sort();
    assert_eq!(values, ["1", "2", "3", "4"]);
    Ok(())
}

#[test]
fn invalid_keys_are_rejected_without_changes() -> TestResult {
    let (_d, doc) = document(&[&["n"], &["a"]])?;
    assert!(matches!(
        doc.plan_sort_rows(&[]),
        Err(DocumentError::Edit(_))
    ));
    let outside = key(5, SortDirection::Ascending, SortKind::Raw);
    assert!(matches!(
        doc.plan_sort_rows(&[outside]),
        Err(DocumentError::Edit(_))
    ));
    Ok(())
}

#[test]
fn stale_plan_is_rejected() -> TestResult {
    let (_d, mut doc) = document(&[&["n"], &["b"], &["a"]])?;
    let plan = doc.plan_sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    doc.set_cell(1, 0, "0")?;
    assert!(matches!(
        doc.apply_row_order_plan(&plan),
        Err(DocumentError::Edit(_))
    ));
    assert_eq!(column_values(&doc, 0), ["0", "a"]);
    Ok(())
}

#[test]
fn reordering_keeps_validation_rules_valid() -> TestResult {
    let (_d, mut doc) = document(&[&["n"], &["b"], &["a"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["a".to_owned(), "b".to_owned()]),
    )?;
    doc.sort_rows(&[key(0, SortDirection::Ascending, SortKind::Raw)])?;
    assert!(doc.validation_report().is_valid());
    Ok(())
}

#[test]
fn multi_source_is_rejected_and_single_source_is_accepted() {
    assert!(reject_multi_source(1).is_ok());
    assert!(matches!(
        reject_multi_source(2),
        Err(DocumentError::Edit(_))
    ));
}
