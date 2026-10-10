use std::str::FromStr;

use tempfile::tempdir;

use super::{
    CalculationBindingId, CalculationEngine, CalculationError, CalculationExpression,
    CalculationMaterializationError, CalculationOperator, CalculationStatus, CalculationTarget,
    CalculationTrigger, CalculationValue,
};
use crate::process::{CellRef, CsvDocument, ValidationRule, ValidationTarget};

// {
// 責務: [document: Materialize検証用の一時CSVを作成する]
// 引数: [rows: CSV行]
// 戻り値: [(TempDir, CsvDocument): 一時directoryとdocument]
// }
fn document(rows: &[&[&str]]) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().expect("temporary directory should be available");
    let path = directory.path().join("materialize.csv");
    let rows = rows
        .iter()
        .map(|row| row.iter().map(|value| (*value).to_owned()).collect())
        .collect();
    let document = CsvDocument::create(path, rows).expect("CSV fixture should be created");
    (directory, document)
}

// {
// 責務: [cell: A1表記をtest用CellRefへ変換する]
// 引数: [address: A1 cell]
// 戻り値: [CellRef: zero-based参照]
// }
fn cell(address: &str) -> CellRef {
    CellRef::from_str(address).expect("test cell should parse")
}

// {
// 責務: [id: test用binding IDを作成する]
// 引数: [value: ID文字列]
// 戻り値: [CalculationBindingId: 検証済みID]
// }
fn id(value: &str) -> CalculationBindingId {
    CalculationBindingId::new(value).expect("test binding ID should be valid")
}

// {
// 責務: [add: 2式の加算ASTを作る]
// 引数: [left: 左式, right: 右式]
// 戻り値: [CalculationExpression: 加算式]
// }
fn add(left: CalculationExpression, right: CalculationExpression) -> CalculationExpression {
    CalculationExpression::Arithmetic {
        left: Box::new(left),
        operator: CalculationOperator::Add,
        right: Box::new(right),
    }
}

#[test]
// {
// 責務: [materialize_writes_only_on_explicit_command_and_is_undoable: RecalculateとMaterializeの境界、履歴、binding保持を検証する]
// 戻り値: [(): 全assertion成立時に成功]
// }
fn materialize_writes_only_on_explicit_command_and_is_undoable() {
    let (_directory, mut document) = document(&[&["input", "output"], &["2", "raw-before"]]);
    let mut engine = CalculationEngine::default();
    let output = id("output");
    engine
        .set_binding(
            &document,
            output.clone(),
            CalculationTarget::Cell(cell("B2")),
            add(
                CalculationExpression::Cell(cell("A2")),
                CalculationExpression::Literal(CalculationValue::Integer(3)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();

    engine.recalculate_all(&document).unwrap();
    assert_eq!(document.cell_ref(cell("B2")), Some("raw-before"));
    assert!(!document.is_dirty());

    let result = engine.materialize(&mut document, &output).unwrap();

    assert_eq!(result.target, cell("B2"));
    assert_eq!(result.value, "5");
    assert!(!result.binding_removed);
    assert_eq!(document.cell_ref(cell("B2")), Some("5"));
    assert!(document.is_dirty());
    assert!(engine.binding(&output).is_some());

    assert!(document.undo().unwrap());
    assert_eq!(document.cell_ref(cell("B2")), Some("raw-before"));
    assert!(!document.is_dirty());
    assert!(document.redo().unwrap());
    assert_eq!(document.cell_ref(cell("B2")), Some("5"));
}

#[test]
// {
// 責務: [pending_stale_and_error_results_are_not_materialized: 非fresh状態をCSVへ反映しないことを検証する]
// 戻り値: [(): 各状態が拒否されraw値が維持されると成功]
// }
fn pending_stale_and_error_results_are_not_materialized() {
    let (_directory, mut pending_document) = document(&[&["input", "output"], &["2", "keep"]]);
    let mut pending_engine = CalculationEngine::default();
    let pending = id("pending");
    pending_engine
        .set_binding(
            &pending_document,
            pending.clone(),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Cell(cell("A2")),
            CalculationTrigger::Manual,
        )
        .unwrap();
    assert!(matches!(
        pending_engine.materialize(&mut pending_document, &pending),
        Err(CalculationMaterializationError::ResultUnavailable {
            status: CalculationStatus::Pending,
            ..
        })
    ));
    assert_eq!(pending_document.cell_ref(cell("B2")), Some("keep"));

    pending_engine.recalculate_all(&pending_document).unwrap();
    pending_document.set_cell_ref(cell("A2"), "9").unwrap();
    pending_engine
        .recalculate_for_changes(&pending_document, [cell("A2")])
        .unwrap();
    assert!(matches!(
        pending_engine.materialize(&mut pending_document, &pending),
        Err(CalculationMaterializationError::ResultUnavailable {
            status: CalculationStatus::Stale,
            ..
        })
    ));
    assert_eq!(pending_document.cell_ref(cell("B2")), Some("keep"));

    let (_directory, mut error_document) =
        document(&[&["input", "output"], &["not-number", "keep"]]);
    let mut error_engine = CalculationEngine::default();
    let failed = id("failed");
    error_engine
        .set_binding(
            &error_document,
            failed.clone(),
            CalculationTarget::Cell(cell("B2")),
            add(
                CalculationExpression::Cell(cell("A2")),
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    error_engine.recalculate_all(&error_document).unwrap();
    assert!(matches!(
        error_engine.materialize(&mut error_document, &failed),
        Err(CalculationMaterializationError::ResultUnavailable {
            status: CalculationStatus::Error(_),
            ..
        })
    ));
    assert_eq!(error_document.cell_ref(cell("B2")), Some("keep"));
}

#[test]
// {
// 責務: [materialize_detects_raw_dependency_drift_without_recalculate_notification: engine通知なしraw変更でも旧resultを拒否する]
// 戻り値: [(): stale snapshot検出時にtargetが不変なら成功]
// }
fn materialize_detects_raw_dependency_drift_without_recalculate_notification() {
    let (_directory, mut document) = document(&[&["input", "output"], &["2", "keep"]]);
    let mut engine = CalculationEngine::default();
    let output = id("output");
    engine
        .set_binding(
            &document,
            output.clone(),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Cell(cell("A2")),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    document.set_cell_ref(cell("A2"), "changed").unwrap();
    let before_undo = document.can_undo();

    assert!(matches!(
        engine.materialize(&mut document, &output),
        Err(CalculationMaterializationError::StaleResult(_))
    ));
    assert_eq!(document.cell_ref(cell("B2")), Some("keep"));
    assert_eq!(document.can_undo(), before_undo);
}

#[test]
// {
// 責務: [materialize_detects_transitive_stale_dependency: 上流raw変更を通知しなくても多段derived resultを拒否する]
// 戻り値: [(): downstream targetが変更されなければ成功]
// }
fn materialize_detects_transitive_stale_dependency() {
    let (_directory, mut document) = document(&[&["input", "first", "second"], &["2", "", "keep"]]);
    let mut engine = CalculationEngine::default();
    let first = id("first");
    let second = id("second");
    engine
        .set_binding(
            &document,
            first.clone(),
            CalculationTarget::Cell(cell("B2")),
            add(
                CalculationExpression::Cell(cell("A2")),
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine
        .set_binding(
            &document,
            second.clone(),
            CalculationTarget::Cell(cell("C2")),
            add(
                CalculationExpression::Cell(cell("B2")),
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    document.set_cell_ref(cell("A2"), "10").unwrap();

    assert!(matches!(
        engine.materialize(&mut document, &second),
        Err(CalculationMaterializationError::StaleResult(_))
    ));
    assert_eq!(document.cell_ref(cell("C2")), Some("keep"));
}

#[test]
// {
// 責務: [validation_rejection_is_atomic_and_keeps_binding: Materializeが既存validationを迂回しないことを確認する]
// 戻り値: [(): CSVとbindingが両方維持されれば成功]
// }
fn validation_rejection_is_atomic_and_keeps_binding() {
    let (_directory, mut document) = document(&[&["input", "output"], &["2", "keep"]]);
    document
        .set_validation_rule(
            ValidationTarget::Index(1),
            ValidationRule::AllowedValues(vec!["allowed".to_owned()]),
        )
        .unwrap();

    let mut engine = CalculationEngine::default();
    let output = id("output");
    engine
        .set_binding(
            &document,
            output.clone(),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Literal(CalculationValue::Integer(42)),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    let error = engine
        .materialize_and_unbind(&mut document, &output)
        .unwrap_err();

    assert!(matches!(
        error,
        CalculationMaterializationError::Document(_)
    ));
    assert_eq!(document.cell_ref(cell("B2")), Some("keep"));
    assert!(engine.binding(&output).is_some());
}

#[test]
// {
// 責務: [materialize_and_unbind_requires_explicit_operation: 通常Materializeとunbind付き操作を区別する]
// 戻り値: [(): explicit操作だけbindingを削除すれば成功]
// }
fn materialize_and_unbind_requires_explicit_operation() {
    let (_directory, mut document) = document(&[&["input", "output"], &["2", "keep"]]);
    let mut engine = CalculationEngine::default();
    let output = id("output");
    engine
        .set_binding(
            &document,
            output.clone(),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Literal(CalculationValue::Integer(7)),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    let result = engine
        .materialize_and_unbind(&mut document, &output)
        .unwrap();

    assert!(result.binding_removed);
    assert_eq!(document.cell_ref(cell("B2")), Some("7"));
    assert!(engine.binding(&output).is_none());
    assert!(document.undo().unwrap());
    assert_eq!(document.cell_ref(cell("B2")), Some("keep"));
    assert!(engine.binding(&output).is_none());
}

#[test]
// {
// 責務: [unbind_preflight_failure_does_not_write_csv: unbind準備失敗時にpartial Materializeを残さない]
// 戻り値: [(): revision failure後もCSVとbindingが元状態なら成功]
// }
fn unbind_preflight_failure_does_not_write_csv() {
    let (_directory, mut document) = document(&[
        &["input", "first", "second"],
        &["2", "raw-first", "raw-second"],
    ]);
    let mut engine = CalculationEngine::default();
    let first = id("first");
    let second = id("second");
    engine
        .set_binding(
            &document,
            first.clone(),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Cell(cell("A2")),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine
        .set_binding(
            &document,
            second,
            CalculationTarget::Cell(cell("C2")),
            CalculationExpression::Cell(cell("B2")),
            CalculationTrigger::DependencyChange,
        )
        .unwrap();
    engine.recalculate_all(&document).unwrap();

    engine.derived_revision = u64::MAX;
    let error = engine
        .materialize_and_unbind(&mut document, &first)
        .unwrap_err();

    assert!(matches!(
        error,
        CalculationMaterializationError::Calculation(CalculationError::RevisionOverflow)
    ));
    assert_eq!(document.cell_ref(cell("B2")), Some("raw-first"));
    assert!(engine.binding(&first).is_some());
}
