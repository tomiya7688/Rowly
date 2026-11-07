use tempfile::{TempDir, tempdir};

use super::{DecimalSeries, FillDirection, IntegerSeries};
use crate::process::{
    CellRange, CellRef, CsvDocument, DocumentError, ValidationRule, ValidationTarget,
};

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
// range: 行・列のindexからCellRangeを作る
// ]
// 引数: [
// from: (usize, usize) 開始(row, column)
// to: (usize, usize) 終了(row, column)
// ]
// 戻り値: [
// CellRange: 範囲
// ]
// }
fn range(from: (usize, usize), to: (usize, usize)) -> CellRange {
    CellRange::new(CellRef::new(from.0, from.1), CellRef::new(to.0, to.1))
}

#[test]
fn fill_down_copies_the_source_raw_value() -> TestResult {
    let (_d, mut doc) = document(&[&["h", "k"], &["東京", "1"], &["", "2"], &["", "3"]])?;
    let changed = doc.fill_values(range((1, 0), (3, 0)), FillDirection::Down, 1)?;
    assert_eq!(changed, 2);
    assert_eq!(doc.csv_text()?, "h,k\n東京,1\n東京,2\n東京,3\n");
    Ok(())
}

#[test]
fn fill_down_repeats_a_multi_cell_pattern_without_guessing_a_series() -> TestResult {
    let (_d, mut doc) = document(&[&["h"], &["1"], &["2"], &[""], &[""], &[""]])?;
    doc.fill_values(range((1, 0), (5, 0)), FillDirection::Down, 2)?;
    assert_eq!(doc.csv_text()?, "h\n1\n2\n1\n2\n1\n");
    Ok(())
}

#[test]
fn fill_right_copies_per_row() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "", ""], &["b", "", ""]])?;
    doc.fill_values(range((0, 0), (1, 2)), FillDirection::Right, 1)?;
    assert_eq!(doc.csv_text()?, "a,a,a\nb,b,b\n");
    Ok(())
}

#[test]
fn fill_is_one_undo_and_redo_restores() -> TestResult {
    let (_d, mut doc) = document(&[&["x"], &[""], &[""]])?;
    let original = doc.csv_text()?;
    doc.fill_values(range((0, 0), (2, 0)), FillDirection::Down, 1)?;
    let after = doc.csv_text()?;
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, original);
    assert!(!doc.can_undo());
    assert!(doc.redo()?);
    assert_eq!(doc.csv_text()?, after);
    Ok(())
}

#[test]
fn fill_rejects_bad_patterns_and_out_of_table_ranges() -> TestResult {
    let (_d, mut doc) = document(&[&["x"], &["y"]])?;
    let before = doc.csv_text()?;
    assert!(
        doc.fill_values(range((0, 0), (1, 0)), FillDirection::Down, 0)
            .is_err()
    );
    assert!(
        doc.fill_values(range((0, 0), (1, 0)), FillDirection::Down, 3)
            .is_err()
    );
    assert!(
        doc.fill_values(range((0, 0), (5, 0)), FillDirection::Down, 1)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn fill_with_nothing_to_change_leaves_the_document_clean() -> TestResult {
    let (_d, mut doc) = document(&[&["x"], &["x"]])?;
    assert_eq!(
        doc.fill_values(range((0, 0), (1, 0)), FillDirection::Down, 1)?,
        0
    );
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn validation_failure_applies_nothing() -> TestResult {
    let (_d, mut doc) = document(&[&["h"], &["ok"], &["ok"], &["ok"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    // 最初のcellを規則外へ書き換えてから、その値で下へ埋めようとする
    let plan = doc.plan_fill_values(range((1, 0), (3, 0)), FillDirection::Down, 1)?;
    assert_eq!(plan.change_count(), 0);
    let (_d2, mut doc2) = document(&[&["h"], &["bad"], &["ok"], &["ok"]])?;
    doc2.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    let before = doc2.csv_text()?;
    assert!(matches!(
        doc2.fill_values(range((1, 0), (3, 0)), FillDirection::Down, 1),
        Err(DocumentError::ValidationRejected { .. })
    ));
    assert_eq!(doc2.csv_text()?, before);
    assert!(!doc2.is_dirty());
    Ok(())
}

#[test]
fn stale_fill_plan_is_rejected() -> TestResult {
    let (_d, mut doc) = document(&[&["x"], &[""], &[""]])?;
    let plan = doc.plan_fill_values(range((0, 0), (2, 0)), FillDirection::Down, 1)?;
    doc.set_cell(0, 0, "changed")?;
    assert!(doc.apply_fill_plan(&plan).is_err());
    let plan = doc.plan_fill_values(range((0, 0), (2, 0)), FillDirection::Down, 1)?;
    doc.set_cell(2, 0, "other")?;
    assert!(doc.apply_fill_plan(&plan).is_err());
    Ok(())
}

#[test]
fn integer_series_uses_the_explicit_start_and_step() -> TestResult {
    let (_d, mut doc) = document(&[&["n"], &[""], &[""], &[""], &[""]])?;
    let series = IntegerSeries::new("10", "-3")?;
    doc.generate_series(range((1, 0), (4, 0)), FillDirection::Down, &series)?;
    assert_eq!(doc.csv_text()?, "n\n10\n7\n4\n1\n");
    Ok(())
}

#[test]
fn decimal_series_is_exact_and_keeps_the_scale() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "", "", ""]])?;
    let series = DecimalSeries::new("0.1", "0.1")?;
    doc.generate_series(range((0, 1), (0, 3)), FillDirection::Right, &series)?;
    assert_eq!(doc.csv_text()?, "a,0.1,0.2,0.3\n");
    let (_d2, mut doc2) = document(&[&["a"], &[""], &[""], &[""]])?;
    let series = DecimalSeries::new("1", "0.5")?;
    doc2.generate_series(range((1, 0), (3, 0)), FillDirection::Down, &series)?;
    assert_eq!(doc2.csv_text()?, "a\n1.0\n1.5\n2.0\n");
    Ok(())
}

#[test]
fn decimal_series_crosses_zero_with_correct_signs() -> TestResult {
    let series = DecimalSeries::new("0.5", "-0.25")?;
    let values: Result<Vec<_>, _> = (0..5)
        .map(|i| super::SeriesGenerator::value(&series, i))
        .collect();
    assert_eq!(values?, ["0.50", "0.25", "0.00", "-0.25", "-0.50"]);
    Ok(())
}

#[test]
fn series_rejects_invalid_numbers_and_overflow() -> TestResult {
    for text in ["", "1e3", " 1", "1.", ".5", "--1", "a", "1.2.3"] {
        assert!(DecimalSeries::new(text, "1").is_err(), "{text:?}");
    }
    assert!(IntegerSeries::new("1.0", "1").is_err());
    let huge = DecimalSeries::new("0", "100000000000000000000000000000")?;
    assert!(super::SeriesGenerator::value(&huge, usize::MAX).is_err());
    Ok(())
}

#[test]
fn series_needs_a_single_line_inside_the_table() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["", ""], &["", ""]])?;
    let series = IntegerSeries::new("1", "1")?;
    let before = doc.csv_text()?;
    assert!(
        doc.generate_series(range((1, 0), (2, 1)), FillDirection::Down, &series)
            .is_err()
    );
    assert!(
        doc.generate_series(range((1, 0), (9, 0)), FillDirection::Down, &series)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}

#[test]
fn series_validation_failure_applies_nothing_and_is_one_undo_otherwise() -> TestResult {
    let (_d, mut doc) = document(&[&["n"], &[""], &[""], &[""]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["".to_owned(), "1".to_owned(), "2".to_owned()]),
    )?;
    let before = doc.csv_text()?;
    let series = IntegerSeries::new("1", "1")?;
    assert!(
        doc.generate_series(range((1, 0), (3, 0)), FillDirection::Down, &series)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    // 2件だけなら規則を満たし、1回のUndoで戻る
    doc.generate_series(range((1, 0), (2, 0)), FillDirection::Down, &series)?;
    assert_eq!(doc.csv_text()?, "n\n1\n2\n\"\"\n");
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}
