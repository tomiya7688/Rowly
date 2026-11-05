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
//   責務: [setup: A列に指定値を持つCSVとDSL宣言を適用したdocumentとengineを作り評価する。],
//   引数: [values: A2以降の値, dsl: 適用するRowly DSL。],
//   戻り値: [(TempDir, CsvDocument, CalculationEngine): 一時directory、document、評価済みengine。]
// }
fn setup(values: &[&str], dsl: &str) -> (TempDir, CsvDocument, CalculationEngine) {
    let directory = tempdir().unwrap();
    let mut rows = vec![["A", "B"].map(String::from).to_vec()];
    rows.extend(
        values
            .iter()
            .map(|value| vec![(*value).to_owned(), String::new()]),
    );
    let mut document = CsvDocument::create(directory.path().join("agg.csv"), rows).unwrap();
    let mut engine = CalculationEngine::default();
    rowly_dsl::run_with_calculation_engine(dsl, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    (directory, document, engine)
}

// {
//   責務: [declare: B2へ集計式を1つ宣言するDSLを作る。],
//   引数: [expression: 集計式のDSL。],
//   戻り値: [String: DSL宣言。]
// }
fn declare(expression: &str) -> String {
    format!("SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = {expression}\n")
}

// {
//   責務: [b2: B2のbindingの値と状態を返す。],
//   引数: [engine: 評価済みengine。],
//   戻り値: [(CalculationStatus, Option<String>): 状態と値。]
// }
fn b2(engine: &CalculationEngine) -> (CalculationStatus, Option<String>) {
    let result = engine
        .result(&CalculationBindingId::new("dsl:B2").unwrap())
        .unwrap();
    (result.status.clone(), result.value.clone())
}

fn evaluated(value: &str) -> (CalculationStatus, Option<String>) {
    (CalculationStatus::Evaluated, Some(value.to_owned()))
}

fn failed(failure: CalculationFailure) -> (CalculationStatus, Option<String>) {
    (CalculationStatus::Error(failure), None)
}

#[test]
// {
//   責務: [aggregates_follow_empty_and_numeric_rules: 空cellの扱いとCOUNT/COUNTAの違い、小数混在を検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn aggregates_follow_empty_and_numeric_rules() {
    let cases = [
        ("Sum(CellRange(\"A2:A4\"))", evaluated("3")),
        ("Count(CellRange(\"A2:A4\"))", evaluated("2")),
        ("CountA(CellRange(\"A2:A4\"))", evaluated("2")),
        ("Average(CellRange(\"A2:A4\"))", evaluated("1.5")),
        ("Min(CellRange(\"A2:A4\"))", evaluated("1")),
        ("Max(CellRange(\"A2:A4\"))", evaluated("2")),
    ];
    for (expression, expected) in cases {
        let (_directory, _document, engine) = setup(&["1", "2", ""], &declare(expression));
        assert_eq!(b2(&engine), expected, "{expression}");
    }
    // 小数が混ざると小数で合計し、平均も小数になる。
    let (_d, _doc, engine) = setup(&["1", "2.5"], &declare("Sum(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), evaluated("3.5"));
    // 数値が無いrange: SUMは0、COUNTは0、AVERAGE/MIN/MAXはEmptyAggregate。
    let (_d, _doc, engine) = setup(&["", " "], &declare("Sum(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), evaluated("0"));
    for function in ["Average", "Min", "Max"] {
        let expression = format!("{function}(CellRange(\"A2:A3\"))");
        let (_d, _doc, engine) = setup(&["", " "], &declare(&expression));
        assert_eq!(
            b2(&engine),
            failed(CalculationFailure::EmptyAggregate),
            "{function}"
        );
    }
    // CountAは空白だけのcellを「空でない」と数える。
    let (_d, _doc, engine) = setup(&["", " "], &declare("CountA(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), evaluated("1"));
}

#[test]
// {
//   責務: [text_is_never_treated_as_zero_and_missing_cells_are_errors: Textを暗黙0にせず、範囲外はMissingCell errorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn text_is_never_treated_as_zero_and_missing_cells_are_errors() {
    let (_d, _doc, engine) = setup(&["1", "abc"], &declare("Sum(CellRange(\"A2:A3\"))"));
    assert_eq!(
        b2(&engine),
        failed(CalculationFailure::ExpectedNumber("abc".to_owned()))
    );
    // COUNTは数値でないTextを数えず、errorにもしない。
    let (_d, _doc, engine) = setup(&["1", "abc"], &declare("Count(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), evaluated("1"));
    let (_d, _doc, engine) = setup(&["1", "2"], &declare("Sum(CellRange(\"A2:A9\"))"));
    assert_eq!(
        b2(&engine),
        failed(CalculationFailure::MissingCell(CellRef::new(3, 0)))
    );
}

#[test]
// {
//   責務: [integer_precision_contract_is_kept_by_sum: Integer合計がoverflowを黙って丸めないことを検証する。],
//   戻り値: [(): overflowがerrorになれば成功する。]
// }
fn integer_precision_contract_is_kept_by_sum() {
    let big = i64::MAX.to_string();
    let (_d, _doc, engine) = setup(&[&big, "1"], &declare("Sum(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), failed(CalculationFailure::ArithmeticOverflow));
}

#[test]
// {
//   責務: [only_changes_inside_the_range_update_the_binding: range内の変更だけがbindingの更新対象になることを検証する。],
//   戻り値: [(): range内変更で再評価され、range外変更では対象外なら成功する。]
// }
fn only_changes_inside_the_range_update_the_binding() {
    let (_directory, mut document, mut engine) =
        setup(&["1", "2", "3"], &declare("Sum(CellRange(\"A2:A3\"))"));
    assert_eq!(b2(&engine), evaluated("3"));

    document.set_cell_ref(CellRef::new(2, 0), "10").unwrap();
    let report = engine
        .recalculate_for_changes(&document, [CellRef::new(2, 0)])
        .unwrap();
    assert_eq!(report.evaluated.len(), 1);
    assert_eq!(b2(&engine), evaluated("11"));

    // A4はrange外なので対象にならず、結果もそのまま。
    document.set_cell_ref(CellRef::new(3, 0), "99").unwrap();
    let report = engine
        .recalculate_for_changes(&document, [CellRef::new(3, 0)])
        .unwrap();
    assert!(report.evaluated.is_empty() && report.stale.is_empty() && report.failed.is_empty());
    assert_eq!(b2(&engine), evaluated("11"));
}

#[test]
// {
//   責務: [fill_expands_a_running_total_with_mixed_endpoints: 始点を固定し終点を相対にしたrangeがFillで累計になることを検証する。],
//   戻り値: [(): 各行の累計が期待どおりなら成功する。]
// }
fn fill_expands_a_running_total_with_mixed_endpoints() {
    let dsl = "SET This.Worksheet.Editor.Range(\"B2:B4\").Calculation.Expression = Sum(CellRange(\"$A$2:A2\"))\n";
    let (_directory, document, engine) = setup(&["1", "2", "3"], dsl);
    for (cell, expected) in [("B2", "1"), ("B3", "3"), ("B4", "6")] {
        let id = CalculationBindingId::new(format!("dsl:{cell}")).unwrap();
        assert_eq!(
            engine.result(&id).unwrap().value.as_deref(),
            Some(expected),
            "{cell}"
        );
    }
    drop(document);
}

#[test]
// {
//   責務: [structure_edits_resize_or_delete_ranges: 行挿入でrangeが拡張し、削除で縮小し、全削除で参照が失われることを検証する。],
//   戻り値: [(): range式の保存形式と評価が期待どおりなら成功する。]
// }
fn structure_edits_resize_or_delete_ranges() {
    let source = |engine: &CalculationEngine, id: &str| {
        engine
            .binding(&CalculationBindingId::new(id).unwrap())
            .unwrap()
            .expression()
            .to_dsl_source()
    };
    // A2:A3の内側(行index 2の前)へ挿入するとA2:A4に拡張する。
    let (_directory, mut document, mut engine) =
        setup(&["1", "2"], &declare("Sum(CellRange(\"A2:A3\"))"));
    document.insert_rows(2, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::InsertRows { index: 2, count: 1 })
        .unwrap();
    assert_eq!(source(&engine, "dsl:B2"), "Sum(CellRange(\"A2:A4\"))");

    // 先頭行を削除すると、始点は残る行へ寄って縮小する。
    let (_directory, mut document, mut engine) =
        setup(&["1", "2", "3"], &declare("Sum(CellRange(\"A2:A4\"))"));
    document.delete_rows(2, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 2, count: 1 })
        .unwrap();
    assert_eq!(source(&engine, "dsl:B2"), "Sum(CellRange(\"A2:A3\"))");
    engine.recalculate_all(&document).unwrap();
    assert_eq!(b2(&engine), evaluated("4"));

    // rangeの全行を削除すると参照は失われる。
    let (_directory, mut document, mut engine) = setup(
        &["1", "2", "3"],
        "SET This.Worksheet.Editor.Cell(\"B2\").Calculation.Expression = Sum(CellRange(\"A3:A4\"))\n",
    );
    document.delete_rows(2, 2).unwrap();
    engine
        .apply_structure_edit(StructureEdit::DeleteRows { index: 2, count: 2 })
        .unwrap();
    engine.recalculate_all(&document).unwrap();
    assert_eq!(b2(&engine), failed(CalculationFailure::DeletedReference));
}

#[test]
// {
//   責務: [oversized_or_misshapen_ranges_are_rejected_at_declaration: 上限超過・逆順・形が違う集計宣言が宣言時にerrorになることを検証する。],
//   戻り値: [(): 不正な宣言がすべてerrorになれば成功する。]
// }
fn oversized_or_misshapen_ranges_are_rejected_at_declaration() {
    let directory = tempdir().unwrap();
    let rows = vec![
        ["A", "B"].map(String::from).to_vec(),
        ["1", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(directory.path().join("r.csv"), rows).unwrap();
    for expression in [
        "Sum(CellRange(\"A1:A200000\"))",
        "Sum(CellRange(\"A9:A2\"))",
        "Sum(CellRange(\"A2\"))",
        "Sum(CellValue(\"A2\"))",
        "CellRange(\"A2:A3\")",
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
