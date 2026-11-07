use tempfile::{TempDir, tempdir};

use super::{CaseMode, TextTransform, TransformTarget, TrimMode, apply_text_transform};
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

#[test]
fn trim_modes_follow_unicode_white_space() {
    let value = " \u{3000}a b\t\u{a0}";
    assert_eq!(
        apply_text_transform(TextTransform::Trim(TrimMode::Leading), value),
        "a b\t\u{a0}"
    );
    assert_eq!(
        apply_text_transform(TextTransform::Trim(TrimMode::Trailing), value),
        " \u{3000}a b"
    );
    assert_eq!(
        apply_text_transform(TextTransform::Trim(TrimMode::Both), value),
        "a b"
    );
    // ゼロ幅スペースはWhite_Spaceではないので残る
    assert_eq!(
        apply_text_transform(TextTransform::Trim(TrimMode::Both), "\u{200b}x"),
        "\u{200b}x"
    );
}

#[test]
fn case_conversion_uses_locale_independent_unicode_rules() {
    let upper = |value| apply_text_transform(TextTransform::Case(CaseMode::Upper), value);
    let lower = |value| apply_text_transform(TextTransform::Case(CaseMode::Lower), value);
    // 文字数が変わる変換
    assert_eq!(upper("straße"), "STRASSE");
    // 語末のシグマは文脈規則で小文字ς
    assert_eq!(lower("ΟΔΟΣ"), "οδος");
    // トルコ語のİはロケール非依存では i + 結合ドット
    assert_eq!(lower("İ"), "i\u{307}");
    // ASCIIと全角と日本語
    assert_eq!(upper("abc ａｂｃ あいう"), "ABC ＡＢＣ あいう");
    assert_eq!(lower("ABC ＡＢＣ"), "abc ａｂｃ");
}

#[test]
fn range_transform_changes_only_raw_values_in_one_undo() -> TestResult {
    let (_dir, mut doc) = document(&[&[" Name ", "Memo"], &[" a ", " b "], &[" c ", "d"]])?;
    let range = CellRange::new(CellRef::new(1, 0), CellRef::new(2, 1));
    let plan = doc.plan_text_transform(
        TransformTarget::Range(range),
        TextTransform::Trim(TrimMode::Both),
    )?;
    assert_eq!(plan.scanned(), 4);
    assert_eq!(plan.change_count(), 3);
    assert!(!doc.is_dirty());

    assert_eq!(doc.apply_text_transform_plan(&plan)?, 3);
    assert_eq!(doc.cell_a1("A2")?, Some("a"));
    assert_eq!(doc.cell_a1("B2")?, Some("b"));
    assert_eq!(doc.cell_a1("A3")?, Some("c"));
    // 範囲外のheaderは変わらない
    assert_eq!(doc.cell_a1("A1")?, Some(" Name "));
    assert!(doc.is_dirty());

    assert!(doc.undo()?);
    assert_eq!(doc.cell_a1("A2")?, Some(" a "));
    assert_eq!(doc.cell_a1("B2")?, Some(" b "));
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn column_target_skips_header_row() -> TestResult {
    let (_dir, mut doc) = document(&[&[" id ", "x"], &[" 1 ", " y"]])?;
    let changed = doc.transform_text(
        TransformTarget::Column(0),
        TextTransform::Trim(TrimMode::Both),
    )?;
    assert_eq!(changed, 1);
    assert_eq!(doc.cell_a1("A1")?, Some(" id "));
    assert_eq!(doc.cell_a1("A2")?, Some("1"));
    assert_eq!(doc.cell_a1("B2")?, Some(" y"));
    Ok(())
}

#[test]
fn range_that_includes_header_changes_header() -> TestResult {
    let (_dir, mut doc) = document(&[&["Name"], &["a"]])?;
    doc.transform_text(
        TransformTarget::Range("A1:A2".parse()?),
        TextTransform::Case(CaseMode::Upper),
    )?;
    assert_eq!(doc.cell_a1("A1")?, Some("NAME"));
    assert_eq!(doc.cell_a1("A2")?, Some("A"));
    Ok(())
}

#[test]
fn no_op_transform_records_nothing_and_stays_clean() -> TestResult {
    let (_dir, mut doc) = document(&[&["h"], &["ABC"]])?;
    let changed = doc.transform_text(
        TransformTarget::Column(0),
        TextTransform::Case(CaseMode::Upper),
    )?;
    assert_eq!(changed, 0);
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn validation_failure_leaves_every_cell_unchanged() -> TestResult {
    let (_dir, mut doc) = document(&[&["state", "memo"], &["ok", " x "], &["ok", " y "]])?;
    // 小文字のokだけ許可するので、大文字化は全件拒否される
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    let before = doc.csv_text()?;
    let range = CellRange::new(CellRef::new(1, 0), CellRef::new(2, 1));
    let plan_error = doc.transform_text(
        TransformTarget::Range(range),
        TextTransform::Case(CaseMode::Upper),
    );
    assert!(matches!(
        plan_error,
        Err(DocumentError::ValidationRejected { .. })
    ));
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    // 他列(B)の変更だけが通る範囲なら成功する
    doc.transform_text(
        TransformTarget::Column(1),
        TextTransform::Trim(TrimMode::Both),
    )?;
    assert_eq!(doc.cell_a1("B2")?, Some("x"));
    Ok(())
}

#[test]
fn stale_plan_is_rejected_without_partial_change() -> TestResult {
    let (_dir, mut doc) = document(&[&["h", "i"], &[" a ", " b "]])?;
    let plan = doc.plan_text_transform(
        TransformTarget::Column(0),
        TextTransform::Trim(TrimMode::Both),
    )?;
    doc.set_cell_a1("A2", " edited ")?;
    let before = doc.csv_text()?;
    assert!(matches!(
        doc.apply_text_transform_plan(&plan),
        Err(DocumentError::Edit(_))
    ));
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}

#[test]
fn missing_column_or_cell_is_an_error() -> TestResult {
    let (_dir, mut doc) = document(&[&["h"], &["a"]])?;
    assert!(
        doc.plan_text_transform(
            TransformTarget::Column(5),
            TextTransform::Trim(TrimMode::Both)
        )
        .is_err()
    );
    assert!(
        doc.transform_text(
            TransformTarget::Range("A1:C9".parse()?),
            TextTransform::Trim(TrimMode::Both)
        )
        .is_err()
    );
    assert!(!doc.is_dirty());
    Ok(())
}
