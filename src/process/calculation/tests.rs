use std::str::FromStr;

use tempfile::tempdir;

use super::{
    CalculationEngine, CalculationError, CalculationExpression, CalculationFailure,
    CalculationOperator, CalculationStatus, CalculationTarget, CalculationTrigger,
    CalculationValue, PureCalculationFunction,
};
use crate::process::{CellRange, CellRef, CsvDocument};

// {
//   責務: [create_document: 一時CSVを作り、CSV非変更を比較するための初期状態を返す]
//   引数: [rows: 初期CSVの各行]
//   戻り値: [(TempDir, CsvDocument): 一時directoryと開いたCSV document]
// }
fn create_document(rows: &[&[&str]]) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().expect("temporary directory should be available");
    let path = directory.path().join("calculation.csv");
    let rows = rows
        .iter()
        .map(|row| row.iter().map(|value| (*value).to_owned()).collect())
        .collect();
    let document = CsvDocument::create(path, rows).expect("CSV fixture should be created");
    (directory, document)
}

// {
//   責務: [cell: A1形式のfixture参照をzero-based CellRefへ変換する]
//   引数: [address: A1形式のcell address]
//   戻り値: [CellRef: parse済みcell位置]
// }
fn cell(address: &str) -> CellRef {
    CellRef::from_str(address).expect("test cell address should be valid")
}

// {
//   責務: [binding_id: fixture用の検証済みbinding IDを作る]
//   引数: [value: stable binding ID]
//   戻り値: [CalculationBindingId: binding ID]
// }
fn binding_id(value: &str) -> super::CalculationBindingId {
    super::CalculationBindingId::new(value).expect("test binding ID should be valid")
}

// {
//   責務: [binary: 二項計算式を作る]
//   引数: [left: 左辺, operator: 演算, right: 右辺]
//   戻り値: [CalculationExpression: 二項式AST]
// }
fn binary(
    left: CalculationExpression,
    operator: CalculationOperator,
    right: CalculationExpression,
) -> CalculationExpression {
    CalculationExpression::Arithmetic {
        left: Box::new(left),
        operator,
        right: Box::new(right),
    }
}

// {
//   責務: [raw_csv_stays_unchanged_after_recalculation: 再計算結果をCSVから分離する契約を検証する]
//   処理: [A1とB1からC1のderived resultを計算し、CSV文字列とdirty stateを比較する]
//   戻り値: [(): 全assertionが成立した場合に成功する]
// }
#[test]
fn raw_csv_stays_unchanged_after_recalculation() {
    let (_directory, document) =
        create_document(&[&["price", "count", "total"], &["200", "6", ""]]);
    let original_csv = document.csv_text().expect("CSV text should be readable");
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("total"),
            CalculationTarget::Cell(cell("C2")),
            binary(
                CalculationExpression::Cell(cell("A2")),
                CalculationOperator::Multiply,
                CalculationExpression::Cell(cell("B2")),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("binding should be registered");

    let report = engine
        .recalculate_all(&document)
        .expect("calculation should complete");

    assert_eq!(report.evaluated, vec![binding_id("total")]);
    assert_eq!(
        engine
            .result(&binding_id("total"))
            .and_then(|result| result.value.as_deref()),
        Some("1200")
    );
    assert_eq!(
        document
            .csv_text()
            .expect("CSV text should remain readable"),
        original_csv
    );
    assert!(!document.is_dirty());
}

// {
//   責務: [changed_dependencies_recalculate_downstream_bindings: raw cell変更後にbinding依存closureを順に再評価する]
//   処理: [計算結果を別bindingが参照するchainを作り、A2変更後の最終値を確認する]
//   戻り値: [(): dependency closureが再評価されると成功する]
// }
#[test]
fn changed_dependencies_recalculate_downstream_bindings() {
    let (_directory, mut document) =
        create_document(&[&["input", "first", "second"], &["2", "", ""]]);
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("first"),
            CalculationTarget::Cell(cell("B2")),
            binary(
                CalculationExpression::Cell(cell("A2")),
                CalculationOperator::Add,
                CalculationExpression::Literal(CalculationValue::Integer(3)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("first binding should be registered");
    engine
        .set_binding(
            &document,
            binding_id("second"),
            CalculationTarget::Cell(cell("C2")),
            binary(
                CalculationExpression::Cell(cell("B2")),
                CalculationOperator::Multiply,
                CalculationExpression::Literal(CalculationValue::Integer(4)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("second binding should be registered");
    engine
        .recalculate_all(&document)
        .expect("initial chain should evaluate");
    document
        .set_cell(cell("A2").row(), cell("A2").column(), "5")
        .expect("input should be editable");

    let report = engine
        .recalculate_for_changes(&document, [cell("A2")])
        .expect("changed dependency chain should evaluate");

    assert_eq!(report.evaluated.len(), 2);
    assert_eq!(
        engine
            .result(&binding_id("second"))
            .and_then(|result| result.value.as_deref()),
        Some("32")
    );
}

// {
//   責務: [manual_binding_becomes_stale_and_recalculate_all_refreshes_it: Manual triggerのstaleと明示再計算を検証する]
//   処理: [raw dependency変更では旧値を保ち、全件再計算で新値へ更新する]
//   戻り値: [(): Manual triggerの再評価境界が成立すると成功する]
// }
#[test]
fn manual_binding_becomes_stale_and_recalculate_all_refreshes_it() {
    let (_directory, mut document) = create_document(&[&["input", "output"], &["2", ""]]);
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("manual"),
            CalculationTarget::Cell(cell("B2")),
            CalculationExpression::Cell(cell("A2")),
            CalculationTrigger::Manual,
        )
        .expect("binding should be registered");
    engine
        .recalculate_all(&document)
        .expect("initial value should evaluate");
    document
        .set_cell(cell("A2").row(), cell("A2").column(), "9")
        .expect("input should be editable");

    let report = engine
        .recalculate_for_changes(&document, [cell("A2")])
        .expect("manual binding should become stale");

    assert_eq!(report.stale, vec![binding_id("manual")]);
    let stale = engine
        .result(&binding_id("manual"))
        .expect("result should exist");
    assert_eq!(stale.status, CalculationStatus::Stale);
    assert_eq!(stale.value.as_deref(), Some("2"));
    engine
        .recalculate_all(&document)
        .expect("manual recalculate should refresh the value");
    assert_eq!(
        engine
            .result(&binding_id("manual"))
            .and_then(|result| result.value.as_deref()),
        Some("9")
    );
}

// {
//   責務: [cycles_are_reported_without_changing_raw_cells: dependency cycleを実行せずerrorへ分類する]
//   処理: [相互参照する2つのbindingを登録し、CSV値を保ったままcycle状態を確認する]
//   戻り値: [(): 循環依存を安全に拒否すると成功する]
// }
#[test]
fn cycles_are_reported_without_changing_raw_cells() {
    let (_directory, document) = create_document(&[&["left", "right"], &["raw-left", "raw-right"]]);
    let original_csv = document.csv_text().expect("CSV text should be readable");
    let mut engine = CalculationEngine::default();
    for (id, target, dependency) in [("left", "A2", "B2"), ("right", "B2", "A2")] {
        engine
            .set_binding(
                &document,
                binding_id(id),
                CalculationTarget::Cell(cell(target)),
                CalculationExpression::Cell(cell(dependency)),
                CalculationTrigger::DependencyChange,
            )
            .expect("cycle fixture binding should be accepted before evaluation");
    }

    let report = engine
        .recalculate_all(&document)
        .expect("cycle should be reported as a result");

    assert_eq!(report.failed.len(), 2);
    assert!(report.failed.iter().all(|id| engine.result(id).is_some_and(
        |result| result.status == CalculationStatus::Error(CalculationFailure::CycleDetected)
    )));
    assert_eq!(
        document.csv_text().expect("CSV should remain readable"),
        original_csv
    );
}

// {
//   責務: [evaluation_errors_preserve_raw_target_and_report_failure: 数値変換error後もtarget cellを変更しない]
//   処理: [非数値dependencyを算術式へ渡し、error statusとraw CSV保持を検証する]
//   戻り値: [(): 評価失敗がCSVへ伝播しないと成功する]
// }
#[test]
fn evaluation_errors_preserve_raw_target_and_report_failure() {
    let (_directory, document) =
        create_document(&[&["input", "output"], &["not-a-number", "keep-me"]]);
    let original_csv = document.csv_text().expect("CSV text should be readable");
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("output"),
            CalculationTarget::Cell(cell("B2")),
            binary(
                CalculationExpression::Cell(cell("A2")),
                CalculationOperator::Add,
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("binding should be registered");

    let report = engine
        .recalculate_all(&document)
        .expect("calculation error should be reported");

    assert_eq!(report.failed, vec![binding_id("output")]);
    assert!(matches!(
        engine
            .result(&binding_id("output"))
            .map(|result| &result.status),
        Some(CalculationStatus::Error(
            CalculationFailure::ExpectedNumber(_)
        ))
    ));
    assert_eq!(document.cell_ref(cell("B2")), Some("keep-me"));
    assert_eq!(
        document.csv_text().expect("CSV should remain readable"),
        original_csv
    );
}

// {
//   責務: [external_csv_sync_keeps_binding_and_recalculates: 外部CSV更新後もengine bindingを使って再計算できることを確かめる]
//   処理: [CSVを外部から書き換えて同期し、既存bindingの新derived resultを評価する]
//   戻り値: [(): external sync後もbindingが有効なら成功する]
// }
#[test]
fn external_csv_sync_keeps_binding_and_recalculates() {
    let (directory, mut document) = create_document(&[&["input", "output"], &["4", ""]]);
    let path = directory.path().join("calculation.csv");
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("output"),
            CalculationTarget::Cell(cell("B2")),
            binary(
                CalculationExpression::Cell(cell("A2")),
                CalculationOperator::Add,
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("binding should be registered");
    engine
        .recalculate_all(&document)
        .expect("initial result should evaluate");
    std::fs::write(&path, "input,output\n8,\n").expect("external CSV should be updated");
    assert!(
        document
            .refresh_if_external_change()
            .expect("external change should synchronize")
    );
    engine
        .recalculate_all(&document)
        .expect("existing binding should recalculate after sync");

    assert_eq!(
        engine
            .result(&binding_id("output"))
            .and_then(|result| result.value.as_deref()),
        Some("9")
    );
    assert_eq!(document.cell_ref(cell("A2")), Some("8"));
    assert_eq!(document.cell_ref(cell("B2")), Some(""));
}

// {
//   責務: [unsupported_targets_are_rejected_before_registration: 初期engineが範囲・列targetを拒否する境界を検証する]
//   処理: [range targetを渡しengine stateが増えないことを確認する]
//   戻り値: [(): 未実装targetを誤って登録しないと成功する]
// }
#[test]
fn unsupported_targets_are_rejected_before_registration() {
    let (_directory, document) = create_document(&[&["a", "b"], &["1", ""]]);
    let mut engine = CalculationEngine::default();
    let result = engine.set_binding(
        &document,
        binding_id("range"),
        CalculationTarget::Range(CellRange::new(cell("A2"), cell("B2"))),
        CalculationExpression::Literal(CalculationValue::Integer(1)),
        CalculationTrigger::DependencyChange,
    );

    assert_eq!(result, Err(CalculationError::UnsupportedTarget));
    assert_eq!(engine.bindings().count(), 0);
}

// {
//   責務: [pure_functions_evaluate_without_general_macro_access: 許可関数の評価を検証する]
//   処理: [Abs、Min、Maxからなる式を評価しderived resultのscalarを確認する]
//   戻り値: [(): restricted function setが動作すると成功する]
// }
#[test]
fn pure_functions_evaluate_without_general_macro_access() {
    let (_directory, document) = create_document(&[&["input", "output"], &["-7", ""]]);
    let mut engine = CalculationEngine::default();
    let absolute = CalculationExpression::Call {
        function: PureCalculationFunction::Abs,
        arguments: vec![CalculationExpression::Cell(cell("A2"))],
    };
    let bounded = CalculationExpression::Call {
        function: PureCalculationFunction::Min,
        arguments: vec![
            absolute,
            CalculationExpression::Literal(CalculationValue::Integer(5)),
        ],
    };
    let expression = CalculationExpression::Call {
        function: PureCalculationFunction::Max,
        arguments: vec![
            bounded,
            CalculationExpression::Literal(CalculationValue::Integer(3)),
        ],
    };
    engine
        .set_binding(
            &document,
            binding_id("output"),
            CalculationTarget::Cell(cell("B2")),
            expression,
            CalculationTrigger::DependencyChange,
        )
        .expect("pure expression binding should be registered");
    engine
        .recalculate_all(&document)
        .expect("pure functions should evaluate");

    assert_eq!(
        engine
            .result(&binding_id("output"))
            .and_then(|result| result.value.as_deref()),
        Some("5")
    );
}

// {
//   責務: [decimal_division_and_zero_division_report_correct_results: 小数除算と0除算の結果分類を検証する]
//   処理: [非ゼロ小数の除算を評価し、0除算だけがerrorになることを確認する]
//   戻り値: [(): 除算の値とerrorが正しく分類されると成功する]
// }
#[test]
fn decimal_division_and_zero_division_report_correct_results() {
    let (_directory, document) =
        create_document(&[&["input", "quotient", "invalid"], &["5.0", "", ""]]);
    let mut engine = CalculationEngine::default();
    for (id, target, divisor) in [("quotient", "B2", 2), ("invalid", "C2", 0)] {
        engine
            .set_binding(
                &document,
                binding_id(id),
                CalculationTarget::Cell(cell(target)),
                binary(
                    CalculationExpression::Cell(cell("A2")),
                    CalculationOperator::Divide,
                    CalculationExpression::Literal(CalculationValue::Integer(divisor)),
                ),
                CalculationTrigger::DependencyChange,
            )
            .expect("division binding should be registered");
    }

    let report = engine
        .recalculate_all(&document)
        .expect("division results should be reported");

    assert_eq!(report.evaluated, vec![binding_id("quotient")]);
    assert_eq!(report.failed, vec![binding_id("invalid")]);
    assert_eq!(
        engine
            .result(&binding_id("quotient"))
            .and_then(|result| result.value.as_deref()),
        Some("2.5")
    );
    assert_eq!(
        engine
            .result(&binding_id("invalid"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Error(
            CalculationFailure::DivisionByZero
        ))
    );
}
