use rowly::{
    process::{
        CsvDocument,
        calculation::{CalculationBindingId, CalculationEngine, CalculationStatus},
    },
    rowly_dsl,
};
use tempfile::tempdir;

/// range宣言から2行へFillし、4種類の参照modeを混ぜたRowly DSL宣言。
const MODES_DSL: &str = "SET This.Worksheet.Editor.Range(\"C2:C3\").Calculation.Expression = CellValue(\"$A$2\") + CellValue(\"B2\")\n\
SET This.Worksheet.Editor.Range(\"D2:D3\").Calculation.Expression = CellValue(\"$A2\")\n\
SET This.Worksheet.Editor.Range(\"E2:E3\").Calculation.Expression = CellValue(\"A$2\")\n\
SET This.Worksheet.Editor.Range(\"F2:F3\").Calculation.Expression = CellValue(\"A2\")\n";

// {
//   責務: [value_of: 指定cellのEvaluated結果の値を返す。],
//   引数: [engine: 評価済みengine。 cell: A1表記のtarget cell。],
//   戻り値: [Option<String>: 評価済みの値。未評価やbinding無しはNone。]
// }
fn value_of(engine: &CalculationEngine, cell: &str) -> Option<String> {
    let id = CalculationBindingId::new(format!("dsl:{cell}")).unwrap();
    let result = engine.result(&id)?;
    (result.status == CalculationStatus::Evaluated)
        .then(|| result.value.clone())
        .flatten()
}

#[test]
// {
//   責務: [fill_keeps_absolute_axes_and_moves_relative_axes: Fill時に絶対軸が固定され相対軸だけが移動することと、CSVが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn fill_keeps_absolute_axes_and_moves_relative_axes() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("modes.csv");
    let rows = vec![
        ["A", "B", "C", "D", "E", "F"].map(String::from).to_vec(),
        ["10", "1", "", "", "", ""].map(String::from).to_vec(),
        ["20", "2", "", "", "", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(path.clone(), rows).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut engine = CalculationEngine::default();

    rowly_dsl::run_with_calculation_engine(MODES_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();

    // $A$2は固定、B2はrowだけ移動: 10+1, 10+2。
    assert_eq!(value_of(&engine, "C2").as_deref(), Some("11"));
    assert_eq!(value_of(&engine, "C3").as_deref(), Some("12"));
    // $A2はcolumn固定でrowが移動: 10, 20。
    assert_eq!(value_of(&engine, "D2").as_deref(), Some("10"));
    assert_eq!(value_of(&engine, "D3").as_deref(), Some("20"));
    // A$2はrow固定: 10, 10（縦Fillではcolumn移動なし）。
    assert_eq!(value_of(&engine, "E2").as_deref(), Some("10"));
    assert_eq!(value_of(&engine, "E3").as_deref(), Some("10"));
    // A2は両軸相対: 10, 20。
    assert_eq!(value_of(&engine, "F2").as_deref(), Some("10"));
    assert_eq!(value_of(&engine, "F3").as_deref(), Some("20"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
// {
//   責務: [malformed_dollar_placement_is_rejected_at_declaration: ``$``の配置が不正な参照が宣言時に拒否されることを検証する。],
//   戻り値: [(): 不正な宣言がerrorになれば成功する。]
// }
fn malformed_dollar_placement_is_rejected_at_declaration() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("bad.csv");
    let rows = vec![
        ["A", "B"].map(String::from).to_vec(),
        ["1", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(path, rows).unwrap();
    let mut engine = CalculationEngine::default();
    let dsl =
        "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = CellValue(\"A$$2\")\n";

    assert!(rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).is_err());
}
