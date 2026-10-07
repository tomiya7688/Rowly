use super::*;

// {
//   責務: [binding_graph_changes_invalidate_downstream_results: Bindingの置換・削除で下流resultをstale化する]
//   処理: [中間Bindingの式を変更して再評価し、そのBindingを削除して依存errorを確認する]
//   戻り値: [(): graph変更前のderived値がcurrent扱いされなければ成功する]
// }
#[test]
fn binding_graph_changes_invalidate_downstream_results() {
    let (_directory, document) = create_document(&[&["input", "first", "second"], &["2", "", ""]]);
    let mut engine = CalculationEngine::default();
    let register = |engine: &mut CalculationEngine, increment| {
        engine
            .set_binding(
                &document,
                binding_id("first"),
                CalculationTarget::Cell(cell("B2")),
                binary(
                    CalculationExpression::Cell(cell("A2")),
                    CalculationOperator::Add,
                    CalculationExpression::Literal(CalculationValue::Integer(increment)),
                ),
                CalculationTrigger::DependencyChange,
            )
            .expect("first binding should be registered");
    };
    register(&mut engine, 3);
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
        .expect("initial results should evaluate");

    register(&mut engine, 5);

    let dependent = engine
        .result(&binding_id("second"))
        .expect("downstream result should exist");
    assert_eq!(dependent.status, CalculationStatus::Stale);
    assert_eq!(dependent.value.as_deref(), Some("20"));
    engine
        .recalculate_all(&document)
        .expect("replacement should recalculate");
    assert_eq!(
        engine
            .result(&binding_id("second"))
            .and_then(|result| result.value.as_deref()),
        Some("28")
    );

    assert!(engine.remove_binding(&binding_id("first")));
    assert_eq!(
        engine
            .result(&binding_id("second"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Stale)
    );
    let report = engine
        .recalculate_all(&document)
        .expect("missing raw numeric value should be reported");
    assert_eq!(report.failed, vec![binding_id("second")]);
}

// {
//   責務: [removed_targets_fail_before_evaluating_bindings: CSV構造変更後に消えたtargetをmissing errorへする]
//   処理: [登録後にtarget rowを除き、再計算時のstatusと元CSVの状態を確認する]
//   戻り値: [(): 存在しないtargetへEvaluatedを返さなければ成功する]
// }
#[test]
fn removed_targets_fail_before_evaluating_bindings() {
    let (_directory, mut document) = create_document(&[&["input", "output"], &["2", "keep-me"]]);
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
            CalculationTrigger::Manual,
        )
        .expect("binding should be registered");
    document
        .apply_csv_text("input,output\n")
        .expect("CSV row should be removed");

    let report = engine
        .recalculate_all(&document)
        .expect("missing target should be reported as a result");

    assert_eq!(report.failed, vec![binding_id("output")]);
    assert_eq!(
        engine
            .result(&binding_id("output"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Error(CalculationFailure::MissingCell(
            cell("B2")
        )))
    );
    assert_eq!(document.cell_ref(cell("A1")), Some("input"));
}

// {
//   責務: [large_integer_comparison_and_exact_division_keep_precision: i64範囲のMin/Maxと整数除算の精度を検証する]
//   処理: [2^53を超える隣接整数を比較し、1で割った整数を正確に保持する]
//   戻り値: [(): scalar結果が整数精度を保てば成功する]
// }
#[test]
fn large_integer_comparison_and_exact_division_keep_precision() {
    let (_directory, document) =
        create_document(&[&["minimum", "maximum", "quotient"], &["", "", ""]]);
    let smaller = 9_007_199_254_740_992_i64;
    let larger = 9_007_199_254_740_993_i64;
    let mut engine = CalculationEngine::default();
    let formulas = [
        (
            "minimum",
            "A2",
            CalculationExpression::Call {
                function: PureCalculationFunction::Min,
                arguments: vec![
                    CalculationExpression::Literal(CalculationValue::Integer(larger)),
                    CalculationExpression::Literal(CalculationValue::Integer(smaller)),
                ],
            },
        ),
        (
            "maximum",
            "B2",
            CalculationExpression::Call {
                function: PureCalculationFunction::Max,
                arguments: vec![
                    CalculationExpression::Literal(CalculationValue::Integer(larger)),
                    CalculationExpression::Literal(CalculationValue::Integer(smaller)),
                ],
            },
        ),
        (
            "quotient",
            "C2",
            binary(
                CalculationExpression::Literal(CalculationValue::Integer(larger)),
                CalculationOperator::Divide,
                CalculationExpression::Literal(CalculationValue::Integer(1)),
            ),
        ),
    ];
    for (id, target, expression) in formulas {
        engine
            .set_binding(
                &document,
                binding_id(id),
                CalculationTarget::Cell(cell(target)),
                expression,
                CalculationTrigger::DependencyChange,
            )
            .expect("integer calculation should be registered");
    }

    engine
        .recalculate_all(&document)
        .expect("integer expressions should evaluate");

    assert_eq!(
        engine
            .result(&binding_id("minimum"))
            .and_then(|result| result.value.as_deref()),
        Some("9007199254740992")
    );
    assert_eq!(
        engine
            .result(&binding_id("maximum"))
            .and_then(|result| result.value.as_deref()),
        Some("9007199254740993")
    );
    assert_eq!(
        engine
            .result(&binding_id("quotient"))
            .and_then(|result| result.value.as_deref()),
        Some("9007199254740993")
    );
}
