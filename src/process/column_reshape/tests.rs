use tempfile::{TempDir, tempdir};

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
// names: 文字列sliceをheader用のString配列へ変える
// ]
// 引数: [
// items: &[&str]
// ]
// 戻り値: [
// Vec<String>: 所有した文字列
// ]
// }
fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn split_keeps_source_and_inserts_after_it() -> TestResult {
    let (_d, mut doc) = document(&[&["name", "n"], &["a b", "1"], &["c", "2"]])?;
    let added = doc.split_column(0, " ", &names(&["first", "last"]), true)?;
    assert_eq!(added, 2);
    assert_eq!(doc.csv_text()?, "name,first,last,n\na b,a,b,1\nc,c,,2\n");
    Ok(())
}

#[test]
fn split_can_delete_the_source_column() -> TestResult {
    let (_d, mut doc) = document(&[&["name", "n"], &["a b", "1"]])?;
    doc.split_column(0, " ", &names(&["first", "last"]), false)?;
    assert_eq!(doc.csv_text()?, "first,last,n\na,b,1\n");
    Ok(())
}

#[test]
fn split_puts_the_remainder_in_the_last_column() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a-b-c-d"]])?;
    doc.split_column(0, "-", &names(&["x", "y"]), true)?;
    assert_eq!(doc.csv_text()?, "p,x,y\na-b-c-d,a,b-c-d\n");
    Ok(())
}

#[test]
fn split_supports_multi_character_delimiters() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a::b"]])?;
    doc.split_column(0, "::", &names(&["x", "y"]), false)?;
    assert_eq!(doc.csv_text()?, "x,y\na,b\n");
    Ok(())
}

#[test]
fn split_is_one_undo_and_redo_restores() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a b"]])?;
    let original = doc.csv_text()?;
    doc.split_column(0, " ", &names(&["x", "y"]), false)?;
    let after = doc.csv_text()?;
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, original);
    assert!(!doc.can_undo());
    assert!(doc.redo()?);
    assert_eq!(doc.csv_text()?, after);
    Ok(())
}

#[test]
fn split_rejects_bad_arguments_without_changes() -> TestResult {
    let (_d, mut doc) = document(&[&["p", "q"], &["a b", "1"]])?;
    let before = doc.csv_text()?;
    assert!(doc.split_column(0, "", &names(&["x"]), true).is_err());
    assert!(doc.split_column(0, " ", &[], true).is_err());
    assert!(doc.split_column(5, " ", &names(&["x"]), true).is_err());
    assert!(doc.split_column(0, " ", &names(&["x", "x"]), true).is_err());
    assert!(doc.split_column(0, " ", &names(&[""]), true).is_err());
    // 既存headerとの重複は拒否する
    assert!(doc.split_column(0, " ", &names(&["q"]), true).is_err());
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn split_may_reuse_the_deleted_source_header() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a b"]])?;
    doc.split_column(0, " ", &names(&["p", "q"]), false)?;
    assert_eq!(doc.csv_text()?, "p,q\na,b\n");
    Ok(())
}

#[test]
fn stale_split_plan_is_rejected() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a b"]])?;
    let plan = doc.plan_split_column(0, " ", &names(&["x", "y"]), true)?;
    doc.set_cell(1, 0, "z w")?;
    let before = doc.csv_text()?;
    assert!(doc.apply_split_plan(&plan).is_err());
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}

#[test]
fn split_plan_previews_without_mutating() -> TestResult {
    let (_d, doc) = document(&[&["p"], &["a b"], &["c"]])?;
    let plan = doc.plan_split_column(0, " ", &names(&["x", "y"]), true)?;
    assert_eq!(plan.scanned, 2);
    assert_eq!(plan.pieces[1], (2, names(&["c", ""])));
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn split_shifts_validation_rules_of_columns_to_the_right() -> TestResult {
    let (_d, mut doc) = document(&[&["p", "q"], &["a b", "1"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(1),
        ValidationRule::AllowedValues(vec!["1".to_owned()]),
    )?;
    doc.split_column(0, " ", &names(&["x", "y"]), true)?;
    assert_eq!(doc.csv_text()?, "p,x,y,q\na b,a,b,1\n");
    assert!(doc.validation_report().violations.is_empty());
    // ruleは列qへ追従しているため、列qへの不正値は拒否される
    assert!(doc.set_cell(1, 3, "no").is_err());
    Ok(())
}

#[test]
fn combine_joins_in_the_given_order_and_keeps_sources() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b", "c"], &["1", "2", "3"], &["x", "", "z"]])?;
    doc.combine_columns(&[2, 0], "-", "ca", true)?;
    assert_eq!(doc.csv_text()?, "a,b,c,ca\n1,2,3,3-1\nx,,z,z-x\n");
    Ok(())
}

#[test]
fn combine_can_delete_the_sources() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b", "c"], &["1", "2", "3"]])?;
    doc.combine_columns(&[0, 1], "/", "ab", false)?;
    assert_eq!(doc.csv_text()?, "ab,c\n1/2,3\n");
    Ok(())
}

#[test]
fn combine_keeps_empty_values_in_the_join() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["", "2"]])?;
    doc.combine_columns(&[0, 1], "|", "ab", false)?;
    assert_eq!(doc.csv_text()?, "ab\n|2\n");
    Ok(())
}

#[test]
fn combine_is_one_undo_and_redo_restores() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["1", "2"]])?;
    let original = doc.csv_text()?;
    doc.combine_columns(&[0, 1], "", "ab", false)?;
    let after = doc.csv_text()?;
    assert!(doc.undo()?);
    assert_eq!(doc.csv_text()?, original);
    assert!(!doc.can_undo());
    assert!(doc.redo()?);
    assert_eq!(doc.csv_text()?, after);
    Ok(())
}

#[test]
fn combine_rejects_bad_arguments_without_changes() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["1", "2"]])?;
    let before = doc.csv_text()?;
    assert!(doc.combine_columns(&[0], "-", "x", true).is_err());
    assert!(doc.combine_columns(&[0, 0], "-", "x", true).is_err());
    assert!(doc.combine_columns(&[0, 9], "-", "x", true).is_err());
    assert!(doc.combine_columns(&[0, 1], "-", "", true).is_err());
    assert!(doc.combine_columns(&[0, 1], "-", "a", true).is_err());
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    Ok(())
}

#[test]
fn combine_may_reuse_a_deleted_source_header() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["1", "2"]])?;
    doc.combine_columns(&[0, 1], "-", "a", false)?;
    assert_eq!(doc.csv_text()?, "a\n1-2\n");
    Ok(())
}

#[test]
fn stale_combine_plan_is_rejected() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b"], &["1", "2"]])?;
    let plan = doc.plan_combine_columns(&[0, 1], "-", "ab", true)?;
    doc.set_cell(1, 0, "9")?;
    assert!(matches!(
        doc.apply_combine_plan(&plan),
        Err(DocumentError::Edit(_))
    ));
    Ok(())
}

#[test]
fn combine_keeps_validation_rule_of_the_surviving_column() -> TestResult {
    let (_d, mut doc) = document(&[&["a", "b", "c"], &["1", "2", "3"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(2),
        ValidationRule::AllowedValues(vec!["3".to_owned()]),
    )?;
    doc.combine_columns(&[0, 1], "-", "ab", false)?;
    assert_eq!(doc.csv_text()?, "ab,c\n1-2,3\n");
    assert!(doc.validation_report().violations.is_empty());
    assert!(doc.set_cell(1, 1, "no").is_err());
    Ok(())
}

#[test]
fn reused_header_with_a_header_rule_is_validated_and_rolled_back() -> TestResult {
    let (_d, mut doc) = document(&[&["p"], &["a b"]])?;
    doc.set_validation_rule(
        ValidationTarget::Header("p".to_owned()),
        ValidationRule::AllowedValues(vec!["a b".to_owned()]),
    )?;
    let before = doc.csv_text()?;
    // 新しいp列へ"a"が入るとruleに反するため、全体を取り消す
    assert!(
        doc.split_column(0, " ", &names(&["p", "q"]), false)
            .is_err()
    );
    assert_eq!(doc.csv_text()?, before);
    Ok(())
}
