use super::{Alignment, DiffKind, MergeKind, MergeSide, diff_tables, diff_three_way};
use crate::process::CellRef;

// {
// 責務: [
// table: 文字列の二重sliceから比較用tableを作る
// ]
// 引数: [
// rows: &[&[&str]]
// ]
// 戻り値: [
// Vec<Vec<String>>: 作ったtable
// ]
// }
fn table(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

// {
// 責務: [
// kinds: 差分一覧から種類だけを取り出す
// ]
// 引数: [
// diff: &super::TableDiff
// ]
// 戻り値: [
// Vec<DiffKind>: 差分の種類
// ]
// }
fn kinds(diff: &super::TableDiff) -> Vec<DiffKind> {
    diff.changes
        .iter()
        .map(|change| change.kind.clone())
        .collect()
}

#[test]
fn identical_tables_have_no_changes_and_inputs_are_unchanged() {
    let before = table(&[&["id", "name"], &["1", "a"], &["2", "b"]]);
    let after = before.clone();
    let diff = diff_tables("s", &before, &after);
    assert!(diff.changes.is_empty());
    assert_eq!(before, after);
}

#[test]
fn single_cell_change_keeps_source_and_both_positions() {
    let before = table(&[&["id", "name"], &["1", "a"], &["2", "b"]]);
    let after = table(&[&["id", "name"], &["1", "a"], &["2", "z"]]);
    let diff = diff_tables("src-A", &before, &after);
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(diff.changes[0].source_id, "src-A");
    assert_eq!(
        diff.changes[0].kind,
        DiffKind::CellChanged {
            before: CellRef::new(2, 1),
            after: CellRef::new(2, 1),
            before_value: Some("b".into()),
            after_value: Some("z".into()),
            alignment: Alignment::Anchored,
        }
    );
}

#[test]
fn row_insert_and_delete_use_header_inclusive_indices() {
    let before = table(&[&["h"], &["a"], &["b"], &["c"]]);
    let after = table(&[&["h"], &["a"], &["c"], &["d"]]);
    let diff = diff_tables("s", &before, &after);
    assert_eq!(
        kinds(&diff),
        vec![
            DiffKind::RowDeleted { row: 2 },
            DiffKind::RowInserted { row: 3 }
        ]
    );
}

#[test]
fn header_rename_and_column_growth_are_reported() {
    let before = table(&[&["a", "b"], &["1", "2"]]);
    let after = table(&[&["a", "x", "c"], &["1", "2", "3"]]);
    let diff = diff_tables("s", &before, &after);
    let found = kinds(&diff);
    assert!(found.contains(&DiffKind::StructureChanged {
        before_columns: 2,
        after_columns: 3
    }));
    assert!(found.contains(&DiffKind::HeaderChanged {
        column: 1,
        before_value: Some("b".into()),
        after_value: Some("x".into()),
    }));
    assert!(found.contains(&DiffKind::HeaderChanged {
        column: 2,
        before_value: None,
        after_value: Some("c".into()),
    }));
}

#[test]
fn ragged_rows_distinguish_missing_cell_from_empty_cell() {
    let before = table(&[&["a", "b"], &["1"]]);
    let after = table(&[&["a", "b"], &["1", ""]]);
    let diff = diff_tables("s", &before, &after);
    assert_eq!(
        kinds(&diff),
        vec![DiffKind::CellChanged {
            before: CellRef::new(1, 1),
            after: CellRef::new(1, 1),
            before_value: None,
            after_value: Some(String::new()),
            alignment: Alignment::Anchored,
        }]
    );
}

#[test]
fn moved_row_is_ambiguous_not_asserted_as_move() {
    let before = table(&[&["h"], &["a"], &["b"], &["c"], &["d"]]);
    let after = table(&[&["h"], &["b"], &["c"], &["d"], &["a"]]);
    let diff = diff_tables("s", &before, &after);
    assert_eq!(
        kinds(&diff),
        vec![DiffKind::RowMovedOrAmbiguous {
            before_rows: vec![1],
            after_rows: vec![4]
        }]
    );
}

#[test]
fn duplicate_rows_are_grouped_as_ambiguous() {
    let before = table(&[&["h"], &["x"], &["x"], &["y"]]);
    let after = table(&[&["h"], &["y"], &["x"]]);
    let diff = diff_tables("s", &before, &after);
    assert!(
        kinds(&diff)
            .iter()
            .all(|kind| matches!(kind, DiffKind::RowMovedOrAmbiguous { .. }))
    );
    assert!(!kinds(&diff).is_empty());
}

#[test]
fn equal_size_gap_pairs_by_position_and_is_marked_positional() {
    let before = table(&[&["h"], &["a"], &["b"], &["keep"]]);
    let after = table(&[&["h"], &["p"], &["q"], &["keep"]]);
    let diff = diff_tables("s", &before, &after);
    assert_eq!(diff.changes.len(), 2);
    assert!(kinds(&diff).iter().all(|kind| matches!(
        kind,
        DiffKind::CellChanged {
            alignment: Alignment::Positional,
            ..
        }
    )));
}

#[test]
fn empty_before_or_after_does_not_panic() {
    let rows = table(&[&["h"], &["a"]]);
    assert!(!diff_tables("s", &[], &rows).changes.is_empty());
    assert!(!diff_tables("s", &rows, &[]).changes.is_empty());
    assert!(diff_tables("s", &[], &[]).changes.is_empty());
}

#[test]
fn three_way_classifies_local_only_other_only_same_and_conflict() {
    let base = table(&[&["h1", "h2"], &["old", "x"], &["m", "n"], &["s", "t"]]);
    let local = table(&[&["h1", "h2"], &["local", "x"], &["M", "n"], &["same", "t"]]);
    let other = table(&[&["h1", "h2"], &["disk", "x"], &["m", "n"], &["same", "T"]]);
    let merged = diff_three_way("s", &base, &local, &other);
    let find = |row, column| {
        merged
            .cells
            .iter()
            .find(|cell| cell.base == CellRef::new(row, column))
    };
    let conflict = find(1, 0).expect("A2 conflict");
    assert_eq!(conflict.kind, MergeKind::Conflict);
    assert_eq!(conflict.base_value.as_deref(), Some("old"));
    assert_eq!(conflict.local, MergeSide::Changed(Some("local".into())));
    assert_eq!(conflict.other, MergeSide::Changed(Some("disk".into())));
    assert_eq!(find(2, 0).map(|c| c.kind), Some(MergeKind::LocalOnly));
    assert_eq!(find(3, 0).map(|c| c.kind), Some(MergeKind::BothSame));
    assert_eq!(find(3, 1).map(|c| c.kind), Some(MergeKind::OtherOnly));
}

#[test]
fn three_way_conflicts_when_one_side_deletes_a_row_the_other_edits() {
    let base = table(&[&["h"], &["a"], &["b"], &["c"]]);
    let local = table(&[&["h"], &["a"], &["c"]]);
    let other = table(&[&["h"], &["a"], &["B"], &["c"]]);
    let merged = diff_three_way("s", &base, &local, &other);
    let cell = merged
        .cells
        .iter()
        .find(|cell| cell.base == CellRef::new(2, 0))
        .expect("edited cell");
    assert_eq!(cell.kind, MergeKind::Conflict);
    assert_eq!(cell.local, MergeSide::RowDeleted);
    assert!(
        merged
            .local_structure
            .contains(&DiffKind::RowDeleted { row: 2 })
    );
}

#[test]
fn three_way_keeps_sources_separate_for_same_cell_address() {
    let base = table(&[&["h"], &["a"]]);
    let changed = table(&[&["h"], &["b"]]);
    let first = diff_three_way("A", &base, &changed, &base);
    let second = diff_three_way("B", &base, &base, &changed);
    assert_eq!(first.cells[0].source_id, "A");
    assert_eq!(second.cells[0].source_id, "B");
    assert_ne!(first.cells[0].kind, second.cells[0].kind);
}

#[test]
fn three_way_marks_positional_alignment_as_ambiguous() {
    let base = table(&[&["h"], &["a"], &["b"], &["keep"]]);
    let local = table(&[&["h"], &["p"], &["q"], &["keep"]]);
    let merged = diff_three_way("s", &base, &local, &base);
    assert!(merged.cells.iter().all(|cell| cell.ambiguous));
}
