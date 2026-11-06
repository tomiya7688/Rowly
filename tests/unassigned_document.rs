use std::fs;

use rowly::process::{CellRef, CsvDocument, DocumentError};
use tempfile::tempdir;

fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

#[test]
// {
//   責務: [new_document_has_no_backing_and_creates_no_file: 新規documentは保存先未割当で、diskにfileを作らず、未編集なら汚れていないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn new_document_has_no_backing_and_creates_no_file() {
    let directory = tempdir().unwrap();
    let document = CsvDocument::new_unassigned(rows(&[&["a", "b"], &["1", "2"]]));
    assert!(document.is_unassigned());
    assert_eq!(document.backing_path(), None);
    assert!(!document.is_dirty());
    assert_eq!(document.cell(1, 1), Some("2"));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
// {
//   責務: [unassigned_document_can_be_edited_and_undone: 保存先が無くても編集・Undo・Redoができ、dirty状態が追従することを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn unassigned_document_can_be_edited_and_undone() {
    let mut document = CsvDocument::new_unassigned(rows(&[&["a", "b"], &["1", "2"]]));
    document
        .set_cell_ref(CellRef::new(1, 0), "changed")
        .unwrap();
    assert_eq!(document.cell(1, 0), Some("changed"));
    assert!(document.is_dirty());
    assert!(document.undo().unwrap());
    assert_eq!(document.cell(1, 0), Some("1"));
    assert!(!document.is_dirty());
    assert!(document.redo().unwrap());
    assert_eq!(document.cell(1, 0), Some("changed"));
    assert!(document.is_unassigned());
}

#[test]
// {
//   責務: [first_save_without_a_path_is_refused_and_save_as_assigns_the_backing: pathなしのsaveが拒否されfileを作らず、save_asで初めてbackingが決まりcleanになり、以後のsaveが新しいbackingへ書かれることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn first_save_without_a_path_is_refused_and_save_as_assigns_the_backing() {
    let directory = tempdir().unwrap();
    let mut document = CsvDocument::new_unassigned(rows(&[&["a", "b"], &["1", "2"]]));
    document.set_cell_ref(CellRef::new(1, 0), "x").unwrap();

    assert!(matches!(
        document.save(),
        Err(DocumentError::SaveRequiresPath)
    ));
    assert!(matches!(
        document.save_metadata(),
        Err(DocumentError::SaveRequiresPath)
    ));
    // 拒否されても、編集内容もdirtyも失われず、fileも作られない。
    assert!(document.is_dirty());
    assert_eq!(document.cell(1, 0), Some("x"));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    assert!(!document.refresh_if_external_change().unwrap());

    // 初回Saveは既存のfileを黙って上書きしない。
    let existing = directory.path().join("existing.csv");
    fs::write(&existing, "keep,me\n").unwrap();
    assert!(matches!(
        document.save_first(&existing),
        Err(DocumentError::DestinationExists { .. })
    ));
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep,me\n");
    assert!(document.is_unassigned() && document.is_dirty());

    let path = directory.path().join("first.csv");
    document.save_first(&path).unwrap();
    // 既にbackingがあるdocumentにsave_firstは使えない。
    assert!(
        document
            .save_first(directory.path().join("second.csv"))
            .is_err()
    );
    assert!(!document.is_unassigned());
    assert_eq!(document.backing_path(), Some(path.as_path()));
    assert!(!document.is_dirty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "a,b\nx,2\n");

    // 以後のsaveは新しいbackingへ書かれる。
    document.set_cell_ref(CellRef::new(1, 1), "y").unwrap();
    assert!(document.is_dirty());
    document.save().unwrap();
    assert!(!document.is_dirty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "a,b\nx,y\n");
}

#[test]
// {
//   責務: [new_blank_creates_a_small_finite_table_with_optional_headers: 行数・列数を明示した有限の空tableが作られ、見出し行は任意で、不整合や大きすぎる指定はerrorになることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn new_blank_creates_a_small_finite_table_with_optional_headers() {
    let plain = CsvDocument::new_blank(3, 2, None).unwrap();
    assert!(plain.is_unassigned());
    assert_eq!((plain.row_count(), plain.column_count()), (3, 2));
    assert_eq!(plain.cell(2, 1), Some(""));

    let headers =
        CsvDocument::new_blank(2, 3, Some(vec!["a".into(), "b".into(), "c".into()])).unwrap();
    assert_eq!((headers.row_count(), headers.column_count()), (3, 3));
    assert_eq!(headers.cell(0, 2), Some("c"));
    assert_eq!(headers.cell(1, 0), Some(""));

    let empty = CsvDocument::new_blank(0, 0, None).unwrap();
    assert_eq!((empty.row_count(), empty.column_count()), (0, 0));

    assert!(CsvDocument::new_blank(2, 3, Some(vec!["only".into()])).is_err());
    assert!(CsvDocument::new_blank(1_000_000, 1_000_000, None).is_err());
    assert!(CsvDocument::new_blank(usize::MAX, 2, None).is_err());
}
