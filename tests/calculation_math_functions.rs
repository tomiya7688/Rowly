use rowly::{
    process::{
        CsvDocument,
        calculation::{CalculationBindingId, CalculationEngine, CalculationStatus},
    },
    rowly_dsl,
};
use tempfile::tempdir;

/// A列の数値からB列へRound/Floor/Ceil/Modの結果を割り当てるRowly DSL宣言。
const MATH_DSL: &str = "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Round(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"C2\").Calculation.Expression = Floor(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"D2\").Calculation.Expression = Ceil(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"E2\").Calculation.Expression = Mod(CellValue(\"A2\"), Integer(\"3\"))\n";

// {
//   責務: [evaluated: 指定cellのbindingのderived resultを状態付きで返す。],
//   引数: [engine: 評価済みengine。 cell: A1表記のtarget cell。],
//   戻り値: [(CalculationStatus, Option<String>): 状態と値。bindingが無い場合はNone。]
// }
fn result_of(
    engine: &CalculationEngine,
    cell: &str,
) -> Option<(CalculationStatus, Option<String>)> {
    let id = CalculationBindingId::new(format!("dsl:{cell}")).unwrap();
    engine
        .result(&id)
        .map(|result| (result.status.clone(), result.value.clone()))
}

#[test]
// {
//   責務: [math_functions_evaluate_from_dsl_and_keep_csv_unchanged: DSLから4関数を評価しCSV本文が変わらないことを検証する。],
//   処理: [-7.5を入力にRound/Floor/Ceil/Modを宣言し、derived resultとCSV本文を確認する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn math_functions_evaluate_from_dsl_and_keep_csv_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("math.csv");
    let rows = vec![
        vec!["x".into(), "r".into(), "f".into(), "c".into(), "m".into()],
        vec!["-7.5".into(), "".into(), "".into(), "".into(), "".into()],
    ];
    let mut document = CsvDocument::create(path.clone(), rows).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut engine = CalculationEngine::default();

    rowly_dsl::run_with_calculation_engine(MATH_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();

    let value = |cell| result_of(&engine, cell).unwrap();
    assert_eq!(
        value("B2"),
        (CalculationStatus::Evaluated, Some("-8".into()))
    );
    assert_eq!(
        value("C2"),
        (CalculationStatus::Evaluated, Some("-8".into()))
    );
    assert_eq!(
        value("D2"),
        (CalculationStatus::Evaluated, Some("-7".into()))
    );
    assert_eq!(
        value("E2"),
        (CalculationStatus::Evaluated, Some("1.5".into()))
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
// {
//   責務: [mod_by_zero_is_reported_as_error_result: 0除数のModが値を作らずerror結果になることを検証する。],
//   戻り値: [(): ErrorステータスでCSVが変わらなければ成功する。]
// }
fn mod_by_zero_is_reported_as_error_result() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("zero.csv");
    let rows = vec![vec!["x".into(), "m".into()], vec!["5".into(), "".into()]];
    let mut document = CsvDocument::create(path.clone(), rows).unwrap();
    let mut engine = CalculationEngine::default();
    let dsl = "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Mod(CellValue(\"A2\"), Integer(\"0\"))\n";

    rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).unwrap();
    let _ = engine.recalculate_all(&document);

    let (status, value) = result_of(&engine, "B2").unwrap();
    assert_ne!(status, CalculationStatus::Evaluated);
    assert_eq!(value, None);
}
