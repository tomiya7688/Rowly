use rowly::{
    process::{
        CellRef, CsvDocument,
        calculation::{
            CalculationBindingId, CalculationEngine, CalculationFailure, CalculationStatus,
            CalculationTarget, StructureEdit,
        },
    },
    rowly_dsl,
};
use tempfile::tempdir;

/// C2:C3へ絶対・相対の混在参照を割り当てるRowly DSL宣言。
const DSL: &str = "SET This.Worksheet.Editor.Range(\"C2:C3\").Calculation.Expression = CellValue(\"$A$2\") + CellValue(\"B2\")\n";

// {
//   責務: [setup: 2データ行のCSVとC2:C3のBindingを持つdocumentとengineを作る。],
//   戻り値: [(TempDir, CsvDocument, CalculationEngine): 一時directory、document、評価済みengine。]
// }
fn setup() -> (tempfile::TempDir, CsvDocument, CalculationEngine) {
    let directory = tempdir().unwrap();
    let rows = vec![
        ["A", "B", "C"].map(String::from).to_vec(),
        ["10", "1", ""].map(String::from).to_vec(),
        ["20", "2", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(directory.path().join("s.csv"), rows).unwrap();
    let mut engine = CalculationEngine::default();
    rowly_dsl::run_with_calculation_engine(DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    (directory, document, engine)
}

// {
//   責務: [id: dsl:<cell>形式のbinding IDを作る。],
//   引数: [cell: 作成時のA1表記。],
//   戻り値: [CalculationBindingId: binding ID。]
// }
fn id(cell: &str) -> CalculationBindingId {
    CalculationBindingId::new(format!("dsl:{cell}")).unwrap()
}

#[test]
// {
//   責務: [row_insert_moves_targets_and_references_to_the_same_logical_cells: 行挿入後もtargetと参照が同じ論理cellを指すことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn row_insert_moves_targets_and_references_to_the_same_logical_cells() {
    let (_directory, mut document, mut engine) = setup();
    document.insert_rows(1, 1).unwrap();
    let report = engine
        .apply_structure_edit(StructureEdit::InsertRows { index: 1, count: 1 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    assert_eq!(report.rewritten, vec![id("C2"), id("C3")]);
    assert_eq!(
        engine.binding(&id("C2")).unwrap().target(),
        CalculationTarget::Cell(CellRef::new(2, 2))
    );
    // $A$2 は絶対参照でも論理cellに追従して$A$3、B2はB3へ移る。
    assert_eq!(
        engine.result(&id("C2")).unwrap().value.as_deref(),
        Some("11")
    );
    assert_eq!(
        engine.result(&id("C3")).unwrap().value.as_deref(),
        Some("12")
    );
}

#[test]
// {
//   責務: [row_delete_removes_deleted_targets_and_marks_deleted_references: 行削除でtargetを失ったbindingが消え、参照先を失った式がerrorになることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn row_delete_removes_deleted_targets_and_marks_deleted_references() {
    let (_directory, mut document, mut engine) = setup();
    document.delete_rows(1, 1).unwrap();
    let report = engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 1, count: 1 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    assert_eq!(report.removed, vec![id("C2")]);
    assert!(engine.binding(&id("C2")).is_none());
    // 残ったC3(新C2)は、削除された$A$2を参照するためDeletedReference errorになる。
    let result = engine.result(&id("C3")).unwrap();
    assert_eq!(
        result.status,
        CalculationStatus::Error(CalculationFailure::DeletedReference)
    );
    assert_eq!(result.value, None);
    assert_eq!(
        engine.binding(&id("C3")).unwrap().target(),
        CalculationTarget::Cell(CellRef::new(1, 2))
    );
}

#[test]
// {
//   責務: [column_edits_follow_references_and_report_deleted_column: 列の挿入削除でも参照が追従し、削除列はerrorになることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn column_edits_follow_references_and_report_deleted_column() {
    let (_directory, mut document, mut engine) = setup();
    document.insert_columns(0, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::InsertColumns { index: 0, count: 1 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();
    assert_eq!(
        engine.result(&id("C2")).unwrap().value.as_deref(),
        Some("11")
    );

    // 参照先のB列(挿入後はC列、index 2)を削除する。target列(D, index 3)は残る。
    document.delete_columns(2, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::DeleteColumns { index: 2, count: 1 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();
    assert_eq!(
        engine.binding(&id("C2")).unwrap().target(),
        CalculationTarget::Cell(CellRef::new(1, 2))
    );
    assert_eq!(
        engine.result(&id("C2")).unwrap().status,
        CalculationStatus::Error(CalculationFailure::DeletedReference)
    );
}

#[test]
// {
//   責務: [deleted_reference_survives_dsl_round_trip: 削除済み参照が``#REF!``として保存形式を往復しerrorを保つことを検証する。],
//   戻り値: [(): 往復後も評価errorなら成功する。]
// }
fn deleted_reference_survives_dsl_round_trip() {
    let (_directory, mut document, mut engine) = setup();
    document.delete_rows(1, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 1, count: 1 })
        .unwrap();
    let source = engine
        .binding(&id("C3"))
        .unwrap()
        .expression()
        .to_dsl_source();
    assert!(source.contains("#REF!"), "{source}");

    let (_other, mut fresh_document, mut fresh_engine) = setup();
    fresh_document.delete_rows(1, 1).unwrap();
    let dsl = format!("SET This.Worksheet.Editor.Cell(\"C2\").Calculation.Expression = {source}\n");
    rowly_dsl::run_with_calculation_engine(&dsl, &mut fresh_document, &mut fresh_engine).unwrap();
    fresh_engine.recalculate_all(&fresh_document).unwrap();
    assert_eq!(
        fresh_engine.result(&id("C2")).unwrap().status,
        CalculationStatus::Error(CalculationFailure::DeletedReference)
    );
}

#[test]
// {
//   責務: [zero_count_edit_changes_nothing: 件数0の構造編集がruleとresultを変えないことを検証する。],
//   戻り値: [(): revisionとresultが変わらなければ成功する。]
// }
fn zero_count_edit_changes_nothing() {
    let (_directory, _document, mut engine) = setup();
    let before = engine.result(&id("C2")).unwrap().clone();
    let report = engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 1, count: 0 })
        .unwrap();
    assert!(report.rewritten.is_empty() && report.removed.is_empty());
    assert_eq!(engine.result(&id("C2")).unwrap(), &before);
}
