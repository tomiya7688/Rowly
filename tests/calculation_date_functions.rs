use rowly::{
    process::{
        CsvDocument,
        calculation::{CalculationBindingId, CalculationEngine, CalculationStatus},
    },
    rowly_dsl,
};
use tempfile::tempdir;

/// A列のISO日付からB〜E列へ日付関数の結果を割り当てるRowly DSL宣言。
const DATE_DSL: &str = "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Year(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"C2\").Calculation.Expression = Month(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"D2\").Calculation.Expression = Day(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"E2\").Calculation.Expression = Date(Integer(\"2024\"), Integer(\"2\"), Integer(\"29\"))\n";

// {
//   責務: [result_of: 指定cellのbindingのderived resultを状態付きで返す。],
//   引数: [engine: 評価済みengine。 cell: A1表記のtarget cell。],
//   戻り値: [Option<(CalculationStatus, Option<String>)>: 状態と値。bindingが無い場合はNone。]
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
//   責務: [date_functions_evaluate_from_dsl_and_keep_csv_unchanged: DSLから日付関数を評価しCSV本文が変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn date_functions_evaluate_from_dsl_and_keep_csv_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("date.csv");
    let rows = vec![
        ["d", "y", "m", "day", "made"].map(String::from).to_vec(),
        ["2023-12-05", "", "", "", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(path.clone(), rows).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut engine = CalculationEngine::default();

    rowly_dsl::run_with_calculation_engine(DATE_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();

    for (cell, value) in [
        ("B2", "2023"),
        ("C2", "12"),
        ("D2", "5"),
        ("E2", "2024-02-29"),
    ] {
        assert_eq!(
            result_of(&engine, cell),
            Some((CalculationStatus::Evaluated, Some(value.to_owned()))),
            "{cell}"
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
// {
//   責務: [locale_style_dates_are_reported_as_error_results: locale依存の日付形式を暗黙解釈せずerror結果にすることを検証する。],
//   戻り値: [(): Evaluated以外の状態で値が無ければ成功する。]
// }
fn locale_style_dates_are_reported_as_error_results() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("bad.csv");
    let rows = vec![
        ["d", "y"].map(String::from).to_vec(),
        ["03/09/2024", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(path, rows).unwrap();
    let mut engine = CalculationEngine::default();
    let dsl =
        "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Year(CellValue(\"A2\"))\n";

    rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).unwrap();
    let _ = engine.recalculate_all(&document);

    let (status, value) = result_of(&engine, "B2").unwrap();
    assert_ne!(status, CalculationStatus::Evaluated);
    assert_eq!(value, None);
}
