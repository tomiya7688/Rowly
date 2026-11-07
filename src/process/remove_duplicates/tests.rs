use tempfile::{TempDir, tempdir};

use super::KeepPolicy;
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

#[test]
fn keep_first_removes_later_rows_with_the_same_key() -> TestResult {
    let (_d, mut doc) = document(&[
        &["id", "v"],
        &["1", "a"],
        &["2", "b"],
        &["1", "c"],
        &["1", "d"],
    ])?;
    let plan = doc.plan_remove_duplicates(&[0], KeepPolicy::First)?;
    assert_eq!(plan.scanned(), 4);
    assert_eq!(plan.removed_count(), 2);
    assert_eq!(plan.removed()[0].row, 3);
    assert_eq!(plan.removed()[0].kept_row, 1);
    assert!(!doc.is_dirty());
    assert_eq!(doc.apply_duplicate_plan(&plan)?, 2);
    assert_eq!(doc.csv_text()?, "id,v\n1,a\n2,b\n");
    Ok(())
}

#[test]
fn keep_last_removes_earlier_rows_and_keeps_row_order() -> TestResult {
    let (_d, mut doc) = document(&[&["id", "v"], &["1", "a"], &["2", "b"], &["1", "c"]])?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::Last)?, 1);
    assert_eq!(doc.cell(1, 1), Some("b"));
    assert_eq!(doc.cell(2, 1), Some("c"));
    assert_eq!(doc.row_count(), 3);
    Ok(())
}

#[test]
fn comparison_is_raw_equality_only() -> TestResult {
    // 表示上同じに見える値でも、raw文字列が違えば別の行
    let (_d, mut doc) = document(&[
        &["k"],
        &["1"],
        &["1.0"],
        &["01"],
        &[" 1"],
        &["A"],
        &["a"],
        &["1"],
    ])?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::First)?, 1);
    assert_eq!(doc.row_count(), 7);
    Ok(())
}

#[test]
fn multiple_key_columns_must_all_match() -> TestResult {
    let (_d, mut doc) = document(&[
        &["a", "b", "c"],
        &["1", "x", "p"],
        &["1", "y", "q"],
        &["1", "x", "r"],
    ])?;
    assert_eq!(doc.remove_duplicates(&[0, 1], KeepPolicy::First)?, 1);
    assert_eq!(doc.cell(2, 2), Some("q"));
    // 同じ列を重複指定しても1列として扱う
    let (_d2, mut doc2) = document(&[&["a"], &["1"], &["1"]])?;
    assert_eq!(doc2.remove_duplicates(&[0, 0], KeepPolicy::First)?, 1);
    Ok(())
}

#[test]
fn header_row_is_never_compared_or_removed() -> TestResult {
    let (_d, mut doc) = document(&[&["x"], &["x"], &["x"]])?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::First)?, 1);
    assert_eq!(doc.cell(0, 0), Some("x"));
    assert_eq!(doc.row_count(), 2);
    Ok(())
}

#[test]
fn removal_is_one_undo_and_redo_restores() -> TestResult {
    let (_d, mut doc) = document(&[&["k"], &["1"], &["1"], &["2"], &["2"], &["1"]])?;
    let original = doc.csv_text()?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::First)?, 3);
    let removed = doc.csv_text()?;
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, original);
    assert!(!doc.can_undo());
    assert!(doc.redo()?);
    assert_eq!(doc.csv_text()?, removed);
    Ok(())
}

#[test]
fn no_duplicates_records_nothing() -> TestResult {
    let (_d, mut doc) = document(&[&["k"], &["1"], &["2"]])?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::First)?, 0);
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn invalid_key_columns_are_rejected_without_change() -> TestResult {
    let (_d, doc) = document(&[&["k"], &["1"], &["1"]])?;
    assert!(matches!(
        doc.plan_remove_duplicates(&[], KeepPolicy::First),
        Err(DocumentError::Edit(_))
    ));
    assert!(doc.plan_remove_duplicates(&[3], KeepPolicy::First).is_err());
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn stale_plan_is_rejected_without_partial_change() -> TestResult {
    let (_d, mut doc) = document(&[&["k", "v"], &["1", "a"], &["1", "b"]])?;
    let plan = doc.plan_remove_duplicates(&[0], KeepPolicy::First)?;
    doc.set_cell_a1("B3", "edited")?;
    let before = doc.csv_text()?;
    assert!(doc.apply_duplicate_plan(&plan).is_err());
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}

#[test]
fn active_transaction_blocks_apply_without_change() -> TestResult {
    let (_d, mut doc) = document(&[&["k"], &["1"], &["1"]])?;
    let plan = doc.plan_remove_duplicates(&[0], KeepPolicy::First)?;
    doc.begin_transaction()?;
    let before = doc.csv_text()?;
    assert!(doc.apply_duplicate_plan(&plan).is_err());
    assert_eq!(doc.csv_text()?, before);
    doc.rollback_transaction()?;
    Ok(())
}

#[test]
fn validation_rules_do_not_block_row_removal() -> TestResult {
    let (_d, mut doc) = document(&[&["k"], &["ok"], &["ok"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    assert_eq!(doc.remove_duplicates(&[0], KeepPolicy::First)?, 1);
    Ok(())
}

#[test]
fn keep_last_reports_the_final_kept_row_for_every_removed_row() -> TestResult {
    let (_d, doc) = document(&[&["k"], &["1"], &["1"], &["1"]])?;
    let plan = doc.plan_remove_duplicates(&[0], KeepPolicy::Last)?;
    assert_eq!(plan.removed_count(), 2);
    assert!(plan.removed().iter().all(|entry| entry.kept_row == 3));
    Ok(())
}
