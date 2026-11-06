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

const ROWS: [(&str, &str); 4] = [("k1", "x"), ("k2", "y"), ("10", "num"), ("", "blank")];

fn lookup(key: &str) -> String {
    format!("Lookup(CellRange(\"A2:A5\"), \"=\", {key}, CellRange(\"B2:B5\"))")
}

#[test]
// {
//   責務: [lookup_returns_the_single_exact_match: 完全一致する1件の結果を返し、数値は数値として照合し、空keyは空文字と一致することを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn lookup_returns_the_single_exact_match() {
    for (key, expected) in [
        ("String(\"k2\")", "y"),
        ("String(\"k1\")", "x"),
        ("Integer(\"10\")", "num"),
        ("Decimal(\"10.0\")", "num"),
        ("String(\"\")", "blank"),
    ] {
        let (_d, _doc, engine) = setup(&ROWS, &declare(&lookup(key)));
        assert_eq!(c2(&engine), evaluated(expected), "{key}");
    }
}

#[test]
// {
//   責務: [lookup_does_not_guess_when_missing_or_duplicated: 見つからない場合と重複する場合は先頭を採用せずerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn lookup_does_not_guess_when_missing_or_duplicated() {
    let (_d, _doc, engine) = setup(&ROWS, &declare(&lookup("String(\"zzz\")")));
    assert_eq!(c2(&engine), failed(CalculationFailure::LookupNotFound));
    let rows = [("k", "first"), ("k", "second")];
    let dsl = declare("Lookup(CellRange(\"A2:A3\"), \"=\", String(\"k\"), CellRange(\"B2:B3\"))");
    let (_d, _doc, engine) = setup(&rows, &dsl);
    assert_eq!(c2(&engine), failed(CalculationFailure::LookupDuplicate));
}

#[test]
// {
//   責務: [lookup_rejects_invalid_declarations: 大きさの違うrange・=以外の記号・result省略・上限超過が宣言時にerrorになることを検証する。],
//   戻り値: [(): 不正な宣言がすべてerrorになれば成功する。]
// }
fn lookup_rejects_invalid_declarations() {
    let directory = tempdir().unwrap();
    let rows = vec![
        ["A", "B", "C"].map(String::from).to_vec(),
        ["1", "2", ""].map(String::from).to_vec(),
    ];
    let mut document = CsvDocument::create(directory.path().join("r.csv"), rows).unwrap();
    for expression in [
        "Lookup(CellRange(\"A2:A3\"), \"=\", String(\"k\"), CellRange(\"B2:B4\"))",
        "Lookup(CellRange(\"A2:A3\"), \">\", String(\"k\"), CellRange(\"B2:B3\"))",
        "Lookup(CellRange(\"A2:A3\"), \"=\", String(\"k\"))",
        "Lookup(CellRange(\"A1:A200000\"), \"=\", String(\"k\"), CellRange(\"B1:B200000\"))",
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
//   責務: [lookup_stays_fresh_and_follows_structure_edits: key・result・検索値cellの変更で再評価され、行挿入でrangeが広がり保存形式が往復することを検証する。],
//   戻り値: [(): 結果とreportが期待どおりなら成功する。]
// }
fn lookup_stays_fresh_and_follows_structure_edits() {
    let expression = "Lookup(CellRange(\"A2:A3\"), \"=\", CellValue(\"C5\"), CellRange(\"B2:B3\"))";
    let (_directory, mut document, mut engine) = setup(&ROWS, &declare(expression));
    // C5は空: A2:A3に空keyは無くLookupNotFound。
    assert_eq!(c2(&engine), failed(CalculationFailure::LookupNotFound));
    for (cell, text, expected) in [
        (CellRef::new(4, 2), "k2", evaluated("y")),
        (CellRef::new(2, 1), "changed", evaluated("changed")),
        (
            CellRef::new(1, 0),
            "k2",
            failed(CalculationFailure::LookupDuplicate),
        ),
    ] {
        document.set_cell_ref(cell, text).unwrap();
        let report = engine.recalculate_for_changes(&document, [cell]).unwrap();
        assert_eq!(report.evaluated.len() + report.failed.len(), 1, "{cell:?}");
        assert_eq!(c2(&engine), expected, "{cell:?}");
    }
    document.insert_rows(2, 1).unwrap();
    engine
        .apply_structure_edit(StructureEdit::InsertRows { index: 2, count: 1 })
        .unwrap();
    let source = engine
        .binding(&CalculationBindingId::new("dsl:C2").unwrap())
        .unwrap()
        .expression()
        .to_dsl_source();
    assert_eq!(
        source,
        "Lookup(CellRange(\"A2:A4\"), \"=\", CellValue(\"C6\"), CellRange(\"B2:B4\"))"
    );
}
