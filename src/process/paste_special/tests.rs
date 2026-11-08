use tempfile::{TempDir, tempdir};

use super::{ClipboardGrid, PasteMode, RaggedPolicy};
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
// cell: 1cellの選択範囲を作る
// ]
// 引数: [
// row: usize
// column: usize
// ]
// 戻り値: [
// CellRange: 1cellの範囲
// ]
// }
fn cell(row: usize, column: usize) -> CellRange {
    CellRange::new(CellRef::new(row, column), CellRef::new(row, column))
}

// {
// 責務: [
// grid: TSVからragged拒否のclipboard値を作る
// ]
// 引数: [
// text: &str
// ]
// 戻り値: [
// ClipboardGrid: 値
// ]
// }
fn grid(text: &str) -> ClipboardGrid {
    ClipboardGrid::from_tsv(text, RaggedPolicy::Reject).expect("valid grid")
}

fn blank(rows: usize, columns: usize) -> Vec<Vec<&'static str>> {
    vec![vec![""; columns]; rows]
}

// {
// 責務: [
// blank_document: 空cellだけのdocumentを作る
// ]
// 引数: [
// rows: usize
// columns: usize
// ]
// 戻り値: [
// Result<(TempDir, CsvDocument), Box<dyn Error>>: documentと一時directory
// ]
// }
fn blank_document(
    rows: usize,
    columns: usize,
) -> Result<(TempDir, CsvDocument), Box<dyn std::error::Error>> {
    let data = blank(rows, columns);
    let refs: Vec<&[&str]> = data.iter().map(Vec::as_slice).collect();
    document(&refs)
}

#[test]
fn transpose_turns_two_by_three_into_three_by_two() -> TestResult {
    let (_d, mut doc) = blank_document(4, 4)?;
    let clip = grid("a\tb\tc\nd\te\tf\n");
    assert_eq!((clip.height(), clip.width()), (2, 3));
    doc.paste_special(cell(0, 0), &clip, PasteMode::Transpose)?;
    assert_eq!(doc.cell(0, 0), Some("a"));
    assert_eq!(doc.cell(0, 1), Some("d"));
    assert_eq!(doc.cell(1, 0), Some("b"));
    assert_eq!(doc.cell(1, 1), Some("e"));
    assert_eq!(doc.cell(2, 0), Some("c"));
    assert_eq!(doc.cell(2, 1), Some("f"));
    assert_eq!(doc.cell(3, 0), Some(""));
    assert_eq!(doc.cell(0, 2), Some(""));
    Ok(())
}

#[test]
fn values_paste_starts_at_the_selection_corner() -> TestResult {
    let (_d, mut doc) = blank_document(3, 3)?;
    doc.paste_special(cell(1, 1), &grid("a\tb\nc\td\n"), PasteMode::Values)?;
    assert_eq!(doc.cell(1, 1), Some("a"));
    assert_eq!(doc.cell(2, 2), Some("d"));
    assert_eq!(doc.cell(0, 0), Some(""));
    Ok(())
}

#[test]
fn repeat_tiles_the_pattern_across_the_selection() -> TestResult {
    let (_d, mut doc) = blank_document(4, 4)?;
    let selection = CellRange::new(CellRef::new(0, 0), CellRef::new(3, 3));
    doc.paste_special(selection, &grid("1\t2\n3\t4\n"), PasteMode::Repeat)?;
    assert_eq!(doc.cell(0, 2), Some("1"));
    assert_eq!(doc.cell(3, 3), Some("4"));
    assert_eq!(doc.cell(2, 1), Some("2"));
    Ok(())
}

#[test]
fn repeat_rejects_a_selection_that_is_not_a_multiple() -> TestResult {
    let (_d, mut doc) = blank_document(4, 4)?;
    let before = doc.csv_text()?;
    let selection = CellRange::new(CellRef::new(0, 0), CellRef::new(2, 3));
    assert!(
        doc.paste_special(selection, &grid("1\t2\n3\t4\n"), PasteMode::Repeat)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}

#[test]
fn paste_never_grows_the_table() -> TestResult {
    let (_d, mut doc) = blank_document(2, 2)?;
    let before = doc.csv_text()?;
    let clip = grid("a\tb\tc\n");
    assert!(
        doc.paste_special(cell(0, 0), &clip, PasteMode::Values)
            .is_err()
    );
    assert!(
        doc.paste_special(cell(1, 0), &clip, PasteMode::Transpose)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn ragged_clipboard_is_rejected_unless_padding_is_explicit() -> TestResult {
    assert!(ClipboardGrid::from_tsv("a\tb\nc\n", RaggedPolicy::Reject).is_err());
    let padded = ClipboardGrid::from_tsv("a\tb\nc\n", RaggedPolicy::PadEmpty)?;
    assert_eq!((padded.height(), padded.width()), (2, 2));
    let (_d, mut doc) = document(&[&["x", "y"], &["p", "q"]])?;
    doc.paste_special(cell(0, 0), &padded, PasteMode::Values)?;
    assert_eq!(doc.csv_text()?, "a,b\nc,\n");
    Ok(())
}

#[test]
fn empty_clipboard_is_rejected() {
    assert!(ClipboardGrid::new(Vec::new(), RaggedPolicy::PadEmpty).is_err());
    assert!(ClipboardGrid::from_tsv("", RaggedPolicy::Reject).is_err());
}

#[test]
fn paste_is_one_undo_and_redo_restores() -> TestResult {
    let (_d, mut doc) = blank_document(3, 3)?;
    let original = doc.csv_text()?;
    doc.paste_special(cell(0, 0), &grid("a\tb\nc\td\n"), PasteMode::Transpose)?;
    let after = doc.csv_text()?;
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, original);
    assert!(!doc.can_undo());
    assert!(doc.redo()?);
    assert_eq!(doc.csv_text()?, after);
    Ok(())
}

#[test]
fn validation_failure_applies_nothing() -> TestResult {
    let (_d, mut doc) = document(&[&["h"], &["ok"], &["ok"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    let before = doc.csv_text()?;
    let selection = CellRange::new(CellRef::new(1, 0), CellRef::new(2, 0));
    assert!(matches!(
        doc.paste_special(selection, &grid("ok\nbad\n"), PasteMode::Values),
        Err(DocumentError::ValidationRejected { .. })
    ));
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn identical_paste_is_a_no_op() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"]])?;
    assert_eq!(
        doc.paste_special(cell(0, 0), &grid("a\tb\n"), PasteMode::Values)?,
        0
    );
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn quoted_tab_and_newline_survive_as_raw_values() -> TestResult {
    let (_d, mut doc) = blank_document(2, 2)?;
    doc.paste_special(cell(0, 0), &grid("\"a\tb\"\t\"c\nd\"\n"), PasteMode::Values)?;
    assert_eq!(doc.cell(0, 0), Some("a\tb"));
    assert_eq!(doc.cell(0, 1), Some("c\nd"));
    Ok(())
}

#[test]
fn stale_plan_is_rejected() -> TestResult {
    let (_d, mut doc) = blank_document(2, 2)?;
    let plan = doc.plan_paste_special(cell(0, 0), &grid("a\n"), PasteMode::Values)?;
    doc.set_cell(0, 0, "changed")?;
    assert!(doc.apply_fill_plan(&plan).is_err());
    Ok(())
}
