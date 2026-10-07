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
    let initial_revision = engine
        .result(&binding_id("second"))
        .expect("initial downstream result should exist")
        .derived_revision;

    register(&mut engine, 5);

    let dependent = engine
        .result(&binding_id("second"))
        .expect("downstream result should exist");
    assert_eq!(dependent.status, CalculationStatus::Stale);
    assert_eq!(dependent.value.as_deref(), Some("20"));
    assert!(dependent.derived_revision > initial_revision);
    engine
        .recalculate_all(&document)
        .expect("replacement should recalculate");
    assert_eq!(
        engine
            .result(&binding_id("second"))
            .and_then(|result| result.value.as_deref()),
        Some("28")
    );

    let evaluated_revision = engine
        .result(&binding_id("second"))
        .expect("recalculated downstream result should exist")
        .derived_revision;
    assert!(
        engine
            .remove_binding(&binding_id("first"))
            .expect("binding removal should succeed")
    );
    assert_eq!(
        engine
            .result(&binding_id("second"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Stale)
    );
    assert!(
        engine
            .result(&binding_id("second"))
            .expect("stale downstream result should exist")
            .derived_revision
            > evaluated_revision
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

// {
//   責務: [fractional_integer_division_uses_quotient_and_remainder: 大きな整数の小数除算で入力値を丸めない]
//   処理: [商と余りから小数結果を作り、表現可能なケースを正確に出力する]
//   戻り値: [(): 整数を先にf64へ丸めず結果を計算できれば成功する]
// }
#[test]
fn fractional_integer_division_uses_quotient_and_remainder() {
    let (_directory, document) = create_document(&[
        &[
            "exact-fraction",
            "unrepresentable",
            "exact-reduced-fraction",
        ],
        &["", "", ""],
    ]);
    let numerator = 9_007_199_254_740_993_i64;
    let mut engine = CalculationEngine::default();
    for (id, target, numerator, divisor) in [
        ("exact-fraction", "A2", numerator, 6_i64),
        ("unrepresentable", "B2", i64::MAX, 3_i64),
        (
            "exact-reduced-fraction",
            "C2",
            numerator,
            18_014_398_509_481_986_i64,
        ),
    ] {
        engine
            .set_binding(
                &document,
                binding_id(id),
                CalculationTarget::Cell(cell(target)),
                binary(
                    CalculationExpression::Literal(CalculationValue::Integer(numerator)),
                    CalculationOperator::Divide,
                    CalculationExpression::Literal(CalculationValue::Integer(divisor)),
                ),
                CalculationTrigger::DependencyChange,
            )
            .expect("integer division should be registered");
    }

    let report = engine
        .recalculate_all(&document)
        .expect("division outcomes should be reported");

    assert_eq!(
        report.evaluated,
        vec![
            binding_id("exact-fraction"),
            binding_id("exact-reduced-fraction")
        ]
    );
    assert_eq!(report.failed, vec![binding_id("unrepresentable")]);
    assert_eq!(
        engine
            .result(&binding_id("exact-fraction"))
            .and_then(|result| result.value.as_deref()),
        Some("1501199875790165.5")
    );
    assert_eq!(
        engine
            .result(&binding_id("exact-reduced-fraction"))
            .and_then(|result| result.value.as_deref()),
        Some("0.5")
    );
    assert_eq!(
        engine
            .result(&binding_id("unrepresentable"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Error(CalculationFailure::PrecisionLoss))
    );
}

// {
//   責務: [mixed_integer_decimal_arithmetic_rejects_precision_loss: integerからDecimalへのlossy conversionを明示的に拒否する]
//   処理: [2^53を超えるintegerと相殺するdecimalの加算を行い、PrecisionLossを確認する]
//   戻り値: [(): 混合演算の入力精度を黙って落とさなければ成功する]
// }
#[test]
fn mixed_integer_decimal_arithmetic_rejects_precision_loss() {
    let (_directory, document) = create_document(&[&["output"], &[""]]);
    let mut engine = CalculationEngine::default();
    engine
        .set_binding(
            &document,
            binding_id("output"),
            CalculationTarget::Cell(cell("A2")),
            binary(
                CalculationExpression::Literal(CalculationValue::Integer(9_007_199_254_740_993)),
                CalculationOperator::Add,
                CalculationExpression::Literal(CalculationValue::Decimal(-9_007_199_254_740_992.0)),
            ),
            CalculationTrigger::DependencyChange,
        )
        .expect("mixed numeric binding should be registered");

    let report = engine
        .recalculate_all(&document)
        .expect("precision error should be returned in the result status");

    assert_eq!(report.failed, vec![binding_id("output")]);
    assert_eq!(
        engine
            .result(&binding_id("output"))
            .map(|result| &result.status),
        Some(&CalculationStatus::Error(CalculationFailure::PrecisionLoss))
    );
}
