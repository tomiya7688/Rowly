use rowly::{
    process::{
        CellRef, CsvDocument,
        calculation::{
            CalculationBindingId, CalculationEngine, CalculationFailure, CalculationStatus,
            StructureEdit,
        },
    },
    rowly_dsl,
};
use tempfile::{TempDir, tempdir};

// {
//   責務: [setup: A列を条件列、B列を金額列にしたCSVへDSL宣言を適用して評価する。],
//   引数: [rows: A2以降の(条件列, 金額列)の組, dsl: 適用するRowly DSL。],
//   戻り値: [(TempDir, CsvDocument, CalculationEngine): 一時directory、document、評価済みengine。]
// }
fn setup(rows: &[(&str, &str)], dsl: &str) -> (TempDir, CsvDocument, CalculationEngine) {
    let directory = tempdir().unwrap();
    let mut table = vec![["A", "B", "C"].map(String::from).to_vec()];
    table.extend(
        rows.iter()
            .map(|(a, b)| vec![(*a).to_owned(), (*b).to_owned(), String::new()]),
    );
    let mut document = CsvDocument::create(directory.path().join("cond.csv"), table).unwrap();
    let mut engine = CalculationEngine::default();
    rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    (directory, document, engine)
}

// {
//   責務: [declare: C2へ式を1つ宣言するDSLを作る。],
//   引数: [expression: 式のDSL。],
//   戻り値: [String: DSL宣言。]
// }
fn declare(expression: &str) -> String {
    format!("SET This.Worksheet.Editor.Cell(\"C2\").Calculation.Expression = {expression}\n")
}

// {
//   責務: [c2: C2のbindingの値と状態を返す。],
//   引数: [engine: 評価済みengine。],
//   戻り値: [(CalculationStatus, Option<String>): 状態と値。]
// }
fn c2(engine: &CalculationEngine) -> (CalculationStatus, Option<String>) {
    let result = engine
        .result(&CalculationBindingId::new("dsl:C2").unwrap())
        .unwrap();
    (result.status.clone(), result.value.clone())
}

fn evaluated(value: &str) -> (CalculationStatus, Option<String>) {
    (CalculationStatus::Evaluated, Some(value.to_owned()))
}

fn failed(failure: CalculationFailure) -> (CalculationStatus, Option<String>) {
    (CalculationStatus::Error(failure), None)
}

const SALES: [(&str, &str); 4] = [("a", "10"), ("b", "20"), ("a", "5"), ("c", "")];

#[test]
// {
//   責務: [countif_and_sumif_follow_comparison_operators: 6種類の比較記号でCOUNTIFとSUMIFが期待の件数・合計を返すことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn countif_and_sumif_follow_comparison_operators() {
    let cases = [
        ("CountIf(CellRange(\"A2:A5\"), \"=\", String(\"a\"))", "2"),
        ("CountIf(CellRange(\"A2:A5\"), \"<>\", String(\"a\"))", "2"),
        ("CountIf(CellRange(\"A2:A5\"), \"<\", String(\"b\"))", "2"),
        ("CountIf(CellRange(\"A2:A5\"), \"<=\", String(\"b\"))", "3"),
        ("CountIf(CellRange(\"A2:A5\"), \">\", String(\"a\"))", "2"),
        ("CountIf(CellRange(\"A2:A5\"), \">=\", String(\"b\"))", "2"),
        (
            "SumIf(CellRange(\"A2:A5\"), \"=\", String(\"a\"), CellRange(\"B2:B5\"))",
            "15",
        ),
        (
            "SumIf(CellRange(\"A2:A5\"), \"<>\", String(\"a\"), CellRange(\"B2:B5\"))",
            "20",
        ),
    ];
    for (expression, expected) in cases {
        let (_directory, _document, engine) = setup(&SALES, &declare(expression));
        assert_eq!(c2(&engine), evaluated(expected), "{expression}");
    }
}

#[test]
// {
//   責務: [numeric_criterion_compares_numbers_and_sum_range_defaults_to_criteria_range: 両方が数値なら数値比較になり、合計対象を省略すると判定range自身を合計することを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn numeric_criterion_compares_numbers_and_sum_range_defaults_to_criteria_range() {
    let rows = [("2", ""), ("10", ""), ("9", ""), ("x", "")];
    // 数値同士は数値順（10 > 9）。文字列順なら"10" < "9"で数えない。
    let (_d, _doc, engine) = setup(
        &rows,
        &declare("CountIf(CellRange(\"A2:A4\"), \">\", CellValue(\"A4\"))"),
    );
    assert_eq!(c2(&engine), evaluated("1"));
    let (_d, _doc, engine) = setup(
        &rows,
        &declare("CountIf(CellRange(\"A2:A4\"), \">\", Integer(\"5\"))"),
    );
    assert_eq!(c2(&engine), evaluated("2"));
    // 合計対象を省略すると判定range自身を合計する。
    let (_d, _doc, engine) = setup(
        &rows,
        &declare("SumIf(CellRange(\"A2:A4\"), \">\", Integer(\"5\"))"),
    );
    assert_eq!(c2(&engine), evaluated("19"));
}

#[test]
// {
//   責務: [non_numeric_sum_cell_is_an_error_but_blank_is_skipped: 条件に合うcellの合計対象が数値でないTextならerror、空白なら飛ばすことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn non_numeric_sum_cell_is_an_error_but_blank_is_skipped() {
    let (_d, _doc, engine) = setup(
        &[("a", "1"), ("a", "oops")],
        &declare("SumIf(CellRange(\"A2:A3\"), \"=\", String(\"a\"), CellRange(\"B2:B3\"))"),
    );
    assert_eq!(
        c2(&engine),
        failed(CalculationFailure::ExpectedNumber("oops".to_owned()))
    );
    // 条件に合わないcellの合計対象はTextでも読まない。
    let (_d, _doc, engine) = setup(
        &[("a", "1"), ("b", "oops"), ("a", "")],
        &declare("SumIf(CellRange(\"A2:A4\"), \"=\", String(\"a\"), CellRange(\"B2:B4\"))"),
    );
    assert_eq!(c2(&engine), evaluated("1"));
}

#[test]
// {
//   責務: [misshapen_or_invalid_conditionals_are_rejected_at_declaration: 大きさが違う範囲・不明な比較記号・上限超過が宣言時にerrorになることを検証する。],
//   戻り値: [(): 不正な宣言がすべてerrorになれば成功する。]
// }
fn misshapen_or_invalid_conditionals_are_rejected_at_declaration() {
    let directory = tempdir().unwrap();
    let rows = vec![
        ["A", "B", "C"].map(String::from).to_vec(),
        ["1", "2", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(directory.path().join("r.csv"), rows).unwrap();
    for expression in [
        "SumIf(CellRange(\"A2:A3\"), \"=\", Integer(\"1\"), CellRange(\"B2:B4\"))",
        "CountIf(CellRange(\"A2:A3\"), \"==\", Integer(\"1\"))",
        "CountIf(CellRange(\"A1:A200000\"), \"=\", Integer(\"1\"))",
        "CountIf(CellValue(\"A2\"), \"=\", Integer(\"1\"))",
        "CountIf(CellRange(\"A2:A3\"), \"=\")",
        "CountIf(CellRange(\"A2:A3\"), \"=\", Integer(\"1\"), CellRange(\"B2:B3\"))",
    ] {
        let mut engine = CalculationEngine::default();
        assert!(
            rowly_dsl::run_with_calculation_engine(
                &declare(expression),
                &mut document,
                &mut engine
            )
            .is_err(),
            "{expression}"
        );
    }
}

#[test]
// {
//   責務: [changes_in_either_range_or_value_refresh_the_binding: 判定range・合計range・比較値cellの変更が再評価対象になり、範囲外は対象外になることを検証する。],
//   戻り値: [(): 結果の更新とreportが期待どおりなら成功する。]
// }
fn changes_in_either_range_or_value_refresh_the_binding() {
    let (_directory, mut document, mut engine) = setup(
        &SALES,
        &declare("SumIf(CellRange(\"A2:A4\"), \"=\", CellValue(\"C5\"), CellRange(\"B2:B4\"))"),
    );
    // C5は空なので空文字との一致を数える: 該当cellが無く0。
    assert_eq!(c2(&engine), evaluated("0"));
    for (cell, text, expected) in [
        (CellRef::new(4, 2), "a", "15"),
        (CellRef::new(3, 1), "100", "110"),
        (CellRef::new(2, 0), "a", "130"),
    ] {
        document.set_cell_ref(cell, text).unwrap();
        let report = engine.recalculate_for_changes(&document, [cell]).unwrap();
        assert_eq!(report.evaluated.len(), 1, "{cell:?}");
        assert_eq!(c2(&engine), evaluated(expected), "{cell:?}");
    }
    // どのrangeにも比較値にも入らないC3の変更は対象外。
    document.set_cell_ref(CellRef::new(2, 2), "z").unwrap();
    let report = engine
        .recalculate_for_changes(&document, [CellRef::new(2, 2)])
        .unwrap();
    assert!(report.evaluated.is_empty() && report.stale.is_empty() && report.failed.is_empty());
}

#[test]
// {
//   責務: [fill_shifts_relative_conditional_ranges: Fillで相対rangeが行ごとにずれることを検証する。],
//   戻り値: [(): 各行の結果が期待どおりなら成功する。]
// }
fn fill_shifts_relative_conditional_ranges() {
    let dsl = "SET This.Worksheet.Editor.Range(\"C2:C4\").Calculation.Expression = CountIf(CellRange(\"A2:A2\"), \"=\", String(\"a\"))\n";
    let (_directory, _document, engine) = setup(&SALES, dsl);
    for (cell, expected) in [("C2", "1"), ("C3", "0"), ("C4", "1")] {
        let id = CalculationBindingId::new(format!("dsl:{cell}")).unwrap();
        assert_eq!(
            engine.result(&id).unwrap().value.as_deref(),
            Some(expected),
            "{cell}"
        );
    }
}

#[test]
// {
//   責務: [structure_edits_keep_conditional_ranges_in_step: 行挿入で両rangeが同じ大きさに広がり、全行削除で参照が失われ、保存形式が往復することを検証する。],
//   戻り値: [(): 保存形式と評価が期待どおりなら成功する。]
// }
fn structure_edits_keep_conditional_ranges_in_step() {
    let source = |engine: &CalculationEngine| {
        engine
            .binding(&CalculationBindingId::new("dsl:C2").unwrap())
            .unwrap()
            .expression()
            .to_dsl_source()
    };
    let expression = "SumIf(CellRange(\"A2:A3\"), \"=\", String(\"a\"), CellRange(\"B2:B3\"))";
    let (_directory, mut document, mut engine) = setup(&SALES, &declare(expression));
    assert_eq!(source(&engine), expression);

    document.insert_rows(2, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::InsertRows { index: 2, count: 1 })
        .unwrap();
    assert_eq!(
        source(&engine),
        "SumIf(CellRange(\"A2:A4\"), \"=\", String(\"a\"), CellRange(\"B2:B4\"))"
    );
    engine.recalculate_all(&document).unwrap();
    assert_eq!(c2(&engine), evaluated("10"));

    // 判定rangeの全行を削除すると参照は失われる。
    let (_directory, mut document, mut engine) = setup(
        &SALES,
        &declare("CountIf(CellRange(\"A3:A4\"), \"=\", String(\"a\"))"),
    );
    document.delete_rows(2, 2).unwrap();
    engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 2, count: 2 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();
    assert_eq!(c2(&engine), failed(CalculationFailure::DeletedReference));
}
