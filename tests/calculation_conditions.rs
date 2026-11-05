use rowly::{
    process::{
        CsvDocument,
        calculation::{
            CalculationBindingId, CalculationEngine, CalculationFailure, CalculationStatus,
        },
    },
    rowly_dsl,
};
use tempfile::{TempDir, tempdir};

// {
//   責務: [run: A2=5, A3="abc"のCSVへB2の式を宣言して評価し、B2の値と状態を返す。],
//   引数: [expression: B2へ宣言する式のDSL。],
//   戻り値: [(TempDir, Result<(CalculationStatus, Option<String>), String>): 一時directoryと結果。宣言がerrorなら文字列。]
// }
fn run(expression: &str) -> (TempDir, Result<(CalculationStatus, Option<String>), String>) {
    let directory = tempdir().unwrap();
    let rows = vec![
        ["A", "B"].map(String::from).to_vec(),
        ["5", ""].map(String::from).to_vec(),
        ["abc", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(directory.path().join("c.csv"), rows).unwrap();
    let mut engine = CalculationEngine::default();
    let dsl =
        format!("SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = {expression}\n");
    let outcome = rowly_dsl::run_with_calculation_engine(&dsl, &mut document, &mut engine)
        .map_err(|error| error.to_string())
        .map(|_| {
            engine.recalculate_all(&document).unwrap();
            let result = engine
                .result(&CalculationBindingId::new("dsl:B2").unwrap())
                .unwrap();
            (result.status.clone(), result.value.clone())
        });
    (directory, outcome)
}

fn evaluated(value: &str) -> Result<(CalculationStatus, Option<String>), String> {
    Ok((CalculationStatus::Evaluated, Some(value.to_owned())))
}

fn failed(failure: CalculationFailure) -> Result<(CalculationStatus, Option<String>), String> {
    Ok((CalculationStatus::Error(failure), None))
}

#[test]
// {
//   責務: [if_selects_a_branch_and_nested_if_works: IF(A2>0, ...)とネストしたIFが期待どおり分岐することを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn if_selects_a_branch_and_nested_if_works() {
    let positive =
        "If(Greater(CellValue(\"A2\"), Integer(\"0\")), String(\"pos\"), String(\"non\"))";
    assert_eq!(run(positive).1, evaluated("pos"));
    let nested = "If(Less(CellValue(\"A2\"), Integer(\"0\")), String(\"neg\"), If(Equals(CellValue(\"A2\"), Integer(\"5\")), String(\"five\"), String(\"other\")))";
    assert_eq!(run(nested).1, evaluated("five"));
}

#[test]
// {
//   責務: [logic_operators_and_boolean_literal_work: And/Or/NotとBoolean literalの結果を検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn logic_operators_and_boolean_literal_work() {
    let cases = [
        ("And(Boolean(\"true\"), Boolean(\"false\"))", "FALSE"),
        ("Or(Boolean(\"false\"), Boolean(\"true\"))", "TRUE"),
        ("Not(Boolean(\"false\"))", "TRUE"),
        ("GreaterOrEqual(CellValue(\"A2\"), Integer(\"5\"))", "TRUE"),
        ("NotEquals(CellValue(\"A3\"), String(\"abc\"))", "FALSE"),
        ("Less(CellValue(\"A3\"), String(\"abd\"))", "TRUE"),
    ];
    for (expression, expected) in cases {
        assert_eq!(run(expression).1, evaluated(expected), "{expression}");
    }
}

#[test]
// {
//   責務: [short_circuit_skips_the_unneeded_branch_errors: 使われない分岐・右辺のerrorを評価しないことと、使われる側のerrorは伝わることを検証する。],
//   戻り値: [(): 短絡評価の仕様どおりなら成功する。]
// }
fn short_circuit_skips_the_unneeded_branch_errors() {
    // A3("abc")を数値として足すとerrorだが、選ばれない分岐なので評価されない。
    let bad = "CellValue(\"A3\") + Integer(\"1\")";
    let skip_else = format!("If(Boolean(\"true\"), Integer(\"1\"), {bad})");
    assert_eq!(run(&skip_else).1, evaluated("1"));
    let skip_and = format!("And(Boolean(\"false\"), Equals({bad}, Integer(\"1\")))");
    assert_eq!(run(&skip_and).1, evaluated("FALSE"));
    let skip_or = format!("Or(Boolean(\"true\"), Equals({bad}, Integer(\"1\")))");
    assert_eq!(run(&skip_or).1, evaluated("TRUE"));
    // 選ばれる側のerrorはそのまま伝わる。
    let taken = format!("If(Boolean(\"false\"), Integer(\"1\"), {bad})");
    assert_eq!(
        run(&taken).1,
        failed(CalculationFailure::ExpectedNumber("abc".to_owned()))
    );
}

#[test]
// {
//   責務: [non_boolean_conditions_and_mixed_comparisons_are_errors: 数値・Textを条件にできず、Booleanと他の型は比較できないことを検証する。],
//   戻り値: [(): 暗黙変換せずerrorになれば成功する。]
// }
fn non_boolean_conditions_and_mixed_comparisons_are_errors() {
    assert_eq!(
        run("If(Integer(\"1\"), Integer(\"1\"), Integer(\"2\"))").1,
        failed(CalculationFailure::ExpectedBoolean("1".to_owned()))
    );
    assert_eq!(
        run("And(CellValue(\"A2\"), Boolean(\"true\"))").1,
        failed(CalculationFailure::ExpectedBoolean("5".to_owned()))
    );
    assert_eq!(
        run("Equals(Boolean(\"true\"), String(\"TRUE\"))").1,
        failed(CalculationFailure::IncomparableValues)
    );
}

#[test]
// {
//   責務: [malformed_boolean_and_arity_are_rejected_at_declaration: Boolean literalの不正値と引数個数の誤りが宣言時にerrorになることを検証する。],
//   戻り値: [(): 不正な宣言がerrorになれば成功する。]
// }
fn malformed_boolean_and_arity_are_rejected_at_declaration() {
    for expression in [
        "Boolean(\"yes\")",
        "If(Boolean(\"true\"), Integer(\"1\"))",
        "Not(Boolean(\"true\"), Boolean(\"true\"))",
    ] {
        assert!(run(expression).1.is_err(), "{expression}");
    }
}
