use rowly::{
    process::{
        CsvDocument,
        calculation::{CalculationBindingId, CalculationEngine, CalculationStatus},
    },
    rowly_dsl,
};
use tempfile::tempdir;

/// A列の文字列からB〜H列へ文字列関数の結果を割り当てるRowly DSL宣言。
const TEXT_DSL: &str = "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Len(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"C2\").Calculation.Expression = Concat(CellValue(\"A2\"), String(\"!\"))\n\
SET This.Worksheet.Editor.Cell(\"D2\").Calculation.Expression = Left(CellValue(\"A2\"), Integer(\"2\"))\n\
SET This.Worksheet.Editor.Cell(\"E2\").Calculation.Expression = Right(CellValue(\"A2\"), Integer(\"2\"))\n\
SET This.Worksheet.Editor.Cell(\"F2\").Calculation.Expression = Mid(CellValue(\"A2\"), Integer(\"2\"), Integer(\"9\"))\n\
SET This.Worksheet.Editor.Cell(\"G2\").Calculation.Expression = Trim(CellValue(\"A2\"))\n\
SET This.Worksheet.Editor.Cell(\"H2\").Calculation.Expression = Upper(String(\"abc\"))\n";

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
//   責務: [text_functions_evaluate_from_dsl_and_keep_csv_unchanged: DSLから文字列関数を評価しCSV本文が変わらないことを検証する。],
//   処理: [日本語を含む文字列に各関数を宣言し、文字単位の結果とCSV本文を確認する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn text_functions_evaluate_from_dsl_and_keep_csv_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("text.csv");
    let mut row = vec!["日本語 ".to_owned()];
    row.extend(std::iter::repeat_n(String::new(), 7));
    let header = ["x", "len", "cat", "left", "right", "mid", "trim", "upper"]
        .map(String::from)
        .to_vec();
    let mut document = CsvDocument::create(path.clone(), vec![header, row]).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut engine = CalculationEngine::default();

    rowly_dsl::run_with_calculation_engine(TEXT_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();

    let expected = [
        ("B2", "4"),
        ("C2", "日本語 !"),
        ("D2", "日本"),
        ("E2", "語 "),
        ("F2", "本語 "),
        ("G2", "日本語"),
        ("H2", "ABC"),
    ];
    for (cell, value) in expected {
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
//   責務: [invalid_text_index_is_reported_as_error_result: 不正な位置指定が値を作らずerror結果になることを検証する。],
//   戻り値: [(): Evaluated以外の状態で値が無ければ成功する。]
// }
fn invalid_text_index_is_reported_as_error_result() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("bad.csv");
    let rows = vec![
        vec!["x".into(), "m".into()],
        vec!["abc".into(), String::new()],
    ];
    let mut document = CsvDocument::create(path, rows).unwrap();
    let mut engine = CalculationEngine::default();
    let dsl = "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Mid(CellValue(\"A2\"), Integer(\"0\"), Integer(\"1\"))\n";

    rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).unwrap();
    let _ = engine.recalculate_all(&document);

    let (status, value) = result_of(&engine, "B2").unwrap();
    assert_ne!(status, CalculationStatus::Evaluated);
    assert_eq!(value, None);
}
