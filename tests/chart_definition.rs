use rowly::process::{
    CsvDocument,
    aggregation::{
        AggregateSpec, CellFreshness, GroupByRequest, InputCell, InputRow, SourceRowRef, group_by,
        rows_from_document,
    },
    calculation::AggregateFunction,
    chart::{
        ChartDefinition, ChartError, ChartKind, ChartUnresolved, MissingReason, PointValue,
        SeriesDefinition, SourceTable, resolve,
    },
    pivot::{PivotRequest, pivot},
};
use tempfile::tempdir;

fn document() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let table = vec![
        vec!["dept", "month", "sales"],
        vec!["east", "1", "10"],
        vec!["west", "1", "20"],
        vec!["east", "2", "5"],
        vec!["west", "2", "7"],
    ]
    .into_iter()
    .map(|row| row.into_iter().map(String::from).collect())
    .collect();
    let document = CsvDocument::create(directory.path().join("c.csv"), table).unwrap();
    (directory, document)
}

fn headers() -> Vec<String> {
    ["dept", "month", "sales"].map(String::from).to_vec()
}

fn snapshot(document: &CsvDocument) -> Vec<Vec<String>> {
    (0..document.row_count())
        .map(|row| {
            (0..document.column_count())
                .map(|column| document.cell(row, column).unwrap_or_default().to_string())
                .collect()
        })
        .collect()
}

fn definition(kind: ChartKind) -> ChartDefinition {
    ChartDefinition {
        name: "sales".into(),
        kind,
        source_id: "c.csv".into(),
        category_header: Some("month".into()),
        series: vec![SeriesDefinition {
            name: Some("Sales".into()),
            header: "sales".into(),
        }],
    }
}

fn raw_table(document: &CsvDocument) -> SourceTable {
    let rows = rows_from_document(document, "c.csv", 1..document.row_count());
    SourceTable::from_input_rows("c.csv", headers(), &rows)
}

fn number(value: f64) -> PointValue {
    PointValue::Number(value)
}

#[test]
// {
//   責務: [raw_rows_become_points_with_provenance_and_the_csv_is_unchanged: raw rowからPointが作られ、各Pointが元rowを指し、解決してもCSVが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn raw_rows_become_points_with_provenance_and_the_csv_is_unchanged() {
    let (_directory, document) = document();
    let before = snapshot(&document);
    let chart = resolve(&definition(ChartKind::Line), &[raw_table(&document)]).unwrap();
    assert_eq!(chart.categories, ["1", "1", "2", "2"]);
    let series = &chart.series[0];
    assert_eq!(series.name, "Sales");
    let values: Vec<_> = series.points.iter().map(|p| p.value.clone()).collect();
    assert_eq!(
        values,
        [number(10.0), number(20.0), number(5.0), number(7.0)]
    );
    assert_eq!(
        series.points[2].provenance,
        [SourceRowRef {
            source: "c.csv".into(),
            row: 3
        }]
    );
    assert!(!chart.stale_indicator());
    assert_eq!(snapshot(&document), before);
}

#[test]
// {
//   責務: [definitions_are_re_resolved_by_source_id_and_header: 列の並びが変わっても同じ結果になり、見出し・sourceが無い/重複する定義は全部まとめてUnresolvedになり、戻れば再び解決できることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn definitions_are_re_resolved_by_source_id_and_header() {
    let (_directory, document) = document();
    let original = raw_table(&document);
    let expected = resolve(
        &definition(ChartKind::Column),
        std::slice::from_ref(&original),
    )
    .unwrap();

    // 列の並びを入れ替えても、見出しで引き直すので同じ点になる。
    let mut reordered = original.clone();
    reordered.headers = ["sales", "dept", "month"].map(String::from).to_vec();
    for row in &mut reordered.rows {
        row.cells.rotate_left(2);
    }
    let again = resolve(&definition(ChartKind::Column), &[reordered]).unwrap();
    assert_eq!(again.series[0].points, expected.series[0].points);

    // 見出しが無い: すべての欠けた見出しをまとめて報告する。
    let mut renamed = original.clone();
    renamed.headers[1] = "when".into();
    renamed.headers[2] = "amount".into();
    assert_eq!(
        resolve(&definition(ChartKind::Column), &[renamed]),
        Err(ChartError::Unresolved(ChartUnresolved {
            missing_columns: vec!["month".into(), "sales".into()],
            ..ChartUnresolved::default()
        }))
    );

    // 見出しが重複: どの列か決められない。
    let mut duplicated = original.clone();
    duplicated.headers[0] = "sales".into();
    let Err(ChartError::Unresolved(unresolved)) =
        resolve(&definition(ChartKind::Column), &[duplicated])
    else {
        panic!("duplicate header must be unresolved");
    };
    assert_eq!(unresolved.ambiguous_columns, ["sales"]);

    // source自体が無い・重複している。
    let Err(ChartError::Unresolved(unresolved)) = resolve(&definition(ChartKind::Column), &[])
    else {
        panic!("missing source must be unresolved");
    };
    assert!(unresolved.missing_source);
    let Err(ChartError::Unresolved(unresolved)) = resolve(
        &definition(ChartKind::Column),
        &[original.clone(), original.clone()],
    ) else {
        panic!("duplicate source must be unresolved");
    };
    assert!(unresolved.ambiguous_source);

    // 戻れば再び解決できる。定義は捨てられていない。
    assert!(resolve(&definition(ChartKind::Column), &[original]).is_ok());
}

#[test]
// {
//   責務: [non_fresh_empty_and_non_numeric_cells_are_not_plotted_and_are_counted: stale・error・pending・空・数値でないcellが通常のseriesに描かれず、理由つきで診断に数えられ、stale indicatorが立つことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn non_fresh_empty_and_non_numeric_cells_are_not_plotted_and_are_counted() {
    let cell = |text: &str, freshness| InputCell {
        text: text.to_owned(),
        freshness,
    };
    let row = |row: usize, month: InputCell, sales: InputCell| InputRow {
        source_row: SourceRowRef {
            source: "d".into(),
            row,
        },
        cells: vec![month, sales],
    };
    let rows = vec![
        row(0, InputCell::fresh("1"), InputCell::fresh("10")),
        row(1, InputCell::fresh("2"), cell("999", CellFreshness::Stale)),
        row(2, InputCell::fresh("3"), cell("999", CellFreshness::Error)),
        row(
            3,
            InputCell::fresh("4"),
            cell("999", CellFreshness::Pending),
        ),
        row(4, InputCell::fresh("5"), InputCell::fresh("  ")),
        row(5, InputCell::fresh("6"), InputCell::fresh("abc")),
        row(6, cell("7", CellFreshness::Stale), InputCell::fresh("70")),
    ];
    let table =
        SourceTable::from_input_rows("d", ["month", "sales"].map(String::from).to_vec(), &rows);
    let mut definition = definition(ChartKind::Line);
    definition.source_id = "d".into();
    let chart = resolve(&definition, &[table]).unwrap();
    let values: Vec<_> = chart.series[0]
        .points
        .iter()
        .map(|point| point.value.clone())
        .collect();
    assert_eq!(
        values,
        [
            number(10.0),
            PointValue::Missing(MissingReason::NotFresh(CellFreshness::Stale)),
            PointValue::Missing(MissingReason::NotFresh(CellFreshness::Error)),
            PointValue::Missing(MissingReason::NotFresh(CellFreshness::Pending)),
            PointValue::Missing(MissingReason::Empty),
            PointValue::Missing(MissingReason::NotNumeric("abc".into())),
            PointValue::Missing(MissingReason::CategoryNotFresh),
        ]
    );
    let d = chart.diagnostics;
    assert_eq!(
        (
            d.stale,
            d.error,
            d.pending,
            d.empty,
            d.non_numeric,
            d.category_not_fresh
        ),
        (1, 1, 1, 1, 1, 1)
    );
    assert!(chart.stale_indicator());
}

#[test]
// {
//   責務: [group_by_source_keeps_the_group_rows_for_drill_down: Group By結果をsourceにでき、Pointが元row集合を持ち、集計errorがその点だけ欠損になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn group_by_source_keeps_the_group_rows_for_drill_down() {
    let (_directory, document) = document();
    let rows = rows_from_document(&document, "c.csv", 1..document.row_count());
    let result = group_by(
        &rows,
        &GroupByRequest {
            key_columns: vec![0],
            aggregates: vec![
                AggregateSpec::Column {
                    function: AggregateFunction::Sum,
                    column: 2,
                },
                // 数値でない列のSUMはerrorになる。
                AggregateSpec::Column {
                    function: AggregateFunction::Sum,
                    column: 0,
                },
            ],
        },
    )
    .unwrap();
    let table = SourceTable::from_group_by(
        "totals",
        &["dept".to_owned()],
        &["total".to_owned(), "broken".to_owned()],
        &result,
    );
    let mut definition = definition(ChartKind::Pie);
    definition.source_id = "totals".into();
    definition.category_header = Some("dept".into());
    definition.series = vec![SeriesDefinition {
        name: None,
        header: "total".into(),
    }];
    let chart = resolve(&definition, std::slice::from_ref(&table)).unwrap();
    assert_eq!(chart.categories, ["east", "west"]);
    let east = &chart.series[0].points[0];
    assert_eq!(east.value, number(15.0));
    let drill: Vec<usize> = east.provenance.iter().map(|row| row.row).collect();
    assert_eq!(drill, [1, 3]);

    definition.series[0].header = "broken".into();
    let chart = resolve(&definition, &[table]).unwrap();
    assert_eq!(
        chart.series[0].points[0].value,
        PointValue::Missing(MissingReason::NotFresh(CellFreshness::Error))
    );
    assert!(chart.stale_indicator());
}

#[test]
// {
//   責務: [pivot_source_gives_one_series_per_column_key_with_cell_provenance: Pivot結果が列keyごとのseriesになり、空のbucketは欠損、各点がそのbucketの元rowを持つことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn pivot_source_gives_one_series_per_column_key_with_cell_provenance() {
    let (_directory, document) = document();
    let mut rows = rows_from_document(&document, "c.csv", 1..document.row_count());
    rows.truncate(3); // east/1, west/1, east/2 → west/2は空のbucket
    let result = pivot(
        &rows,
        &PivotRequest {
            row_columns: vec![0],
            column_columns: vec![1],
            values: vec![AggregateSpec::Column {
                function: AggregateFunction::Sum,
                column: 2,
            }],
            filters: vec![],
        },
    )
    .unwrap();
    let table = SourceTable::from_pivot("pivot", &["dept".to_owned()], &result, 0);
    assert_eq!(table.headers, ["dept", "1", "2"]);
    let definition = ChartDefinition {
        name: "p".into(),
        kind: ChartKind::Column,
        source_id: "pivot".into(),
        category_header: Some("dept".into()),
        series: ["1", "2"]
            .map(|header| SeriesDefinition {
                name: None,
                header: header.into(),
            })
            .to_vec(),
    };
    let chart = resolve(&definition, &[table]).unwrap();
    assert_eq!(chart.categories, ["east", "west"]);
    assert_eq!(chart.series[0].points[0].value, number(10.0));
    assert_eq!(chart.series[1].points[0].value, number(5.0));
    assert_eq!(
        chart.series[1].points[1].value,
        PointValue::Missing(MissingReason::Empty)
    );
    let drill: Vec<usize> = chart.series[0].points[0]
        .provenance
        .iter()
        .map(|row| row.row)
        .collect();
    assert_eq!(drill, [1]);
}

#[test]
// {
//   責務: [invalid_definitions_and_kind_rules: seriesが無い・円グラフに複数series・散布図にx列が無い定義を拒否し、円グラフの負の値と散布図の数値でないxが描かれないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn invalid_definitions_and_kind_rules() {
    let (_directory, document) = document();
    let table = raw_table(&document);

    let mut none = definition(ChartKind::Line);
    none.series.clear();
    assert!(matches!(
        resolve(&none, std::slice::from_ref(&table)),
        Err(ChartError::InvalidDefinition(_))
    ));

    let mut two = definition(ChartKind::Pie);
    two.series.push(two.series[0].clone());
    assert!(matches!(
        resolve(&two, std::slice::from_ref(&table)),
        Err(ChartError::InvalidDefinition(_))
    ));

    let mut no_x = definition(ChartKind::Scatter);
    no_x.category_header = None;
    assert!(matches!(
        resolve(&no_x, std::slice::from_ref(&table)),
        Err(ChartError::InvalidDefinition(_))
    ));

    // 円グラフの負の値は描かない。
    let negative = SourceTable::from_input_rows(
        "n",
        ["k", "v"].map(String::from).to_vec(),
        &[InputRow {
            source_row: SourceRowRef {
                source: "n".into(),
                row: 0,
            },
            cells: vec![InputCell::fresh("a"), InputCell::fresh("-3")],
        }],
    );
    let mut pie = definition(ChartKind::Pie);
    pie.source_id = "n".into();
    pie.category_header = Some("k".into());
    pie.series[0].header = "v".into();
    let chart = resolve(&pie, std::slice::from_ref(&negative)).unwrap();
    assert_eq!(
        chart.series[0].points[0].value,
        PointValue::Missing(MissingReason::Negative)
    );
    assert_eq!(chart.diagnostics.negative, 1);

    // 散布図: categoryが数値ならxになり、数値でなければ点を描かない。
    let mut scatter = pie.clone();
    scatter.kind = ChartKind::Scatter;
    scatter.series[0].header = "v".into();
    let chart = resolve(&scatter, &[negative]).unwrap();
    assert_eq!(chart.series[0].points[0].x, None);
    assert_eq!(
        chart.series[0].points[0].value,
        PointValue::Missing(MissingReason::NotNumeric("a".into()))
    );
    let chart = resolve(&definition(ChartKind::Scatter), &[table]).unwrap();
    assert_eq!(chart.series[0].points[0].x, Some(1.0));
    assert_eq!(chart.series[0].points[0].value, number(10.0));
}
