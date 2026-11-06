use rowly::process::{
    CsvDocument,
    aggregation::{
        AggregateSpec, AggregationError, CellFreshness, InputCell, InputRow, SourceRowRef,
        rows_from_document,
    },
    calculation::{AggregateFunction, CalculationValue},
    pivot::{PivotFilter, PivotRequest, pivot},
};
use tempfile::tempdir;

fn document(rows: &[[&str; 3]]) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let mut table = vec![["dept", "month", "sales"].map(String::from).to_vec()];
    table.extend(rows.iter().map(|row| row.map(String::from).to_vec()));
    let document = CsvDocument::create(directory.path().join("p.csv"), table).unwrap();
    (directory, document)
}

// 行0は見出し。データrowは1から。
const DATA: [[&str; 3]; 5] = [
    ["east", "1", "10"],
    ["west", "1", "20"],
    ["east", "2", "5"],
    ["East", "2", "7"],
    ["east", "1", "1"],
];

fn data_rows(document: &CsvDocument) -> Vec<InputRow> {
    rows_from_document(document, "p.csv", 1..document.row_count())
}

fn sum_sales() -> AggregateSpec {
    AggregateSpec::Column {
        function: AggregateFunction::Sum,
        column: 2,
    }
}

fn request() -> PivotRequest {
    PivotRequest {
        row_columns: vec![0],
        column_columns: vec![1],
        values: vec![sum_sales(), AggregateSpec::CountRows],
        filters: vec![],
    }
}

fn key(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

#[test]
// {
//   責務: [cross_tab_uses_raw_keys_first_appearance_order_and_empty_buckets: raw keyのクロスタブが初出順で並び、該当rowの無いbucketが0ではなく空になり、元documentが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn cross_tab_uses_raw_keys_first_appearance_order_and_empty_buckets() {
    let (_directory, document) = document(&DATA);
    let snapshot = |document: &CsvDocument| -> Vec<Vec<String>> {
        (0..document.row_count())
            .map(|row| {
                (0..document.column_count())
                    .map(|column| document.cell(row, column).unwrap_or_default().to_string())
                    .collect()
            })
            .collect()
    };
    let before = snapshot(&document);
    let result = pivot(&data_rows(&document), &request()).unwrap();

    assert_eq!(result.row_keys, [key("east"), key("west"), key("East")]);
    assert_eq!(result.column_keys, [key("1"), key("2")]);
    let value = |row: usize, column: usize, index: usize| {
        result.cells[row][column]
            .as_ref()
            .map(|cell| cell.values[index].clone())
    };
    // east/1 = 10 + 1、2件
    assert_eq!(value(0, 0, 0), Some(Ok(CalculationValue::Integer(11))));
    assert_eq!(value(0, 0, 1), Some(Ok(CalculationValue::Integer(2))));
    assert_eq!(value(0, 1, 0), Some(Ok(CalculationValue::Integer(5))));
    assert_eq!(value(1, 0, 0), Some(Ok(CalculationValue::Integer(20))));
    // west/2とEast/1は該当rowが無く、空のまま。
    assert!(result.cells[1][1].is_none());
    assert!(result.cells[2][0].is_none());
    assert_eq!(value(2, 1, 0), Some(Ok(CalculationValue::Integer(7))));
    assert_eq!(snapshot(&document), before);
}

#[test]
// {
//   責務: [cells_keep_source_row_references_and_results_are_deterministic: 各cellが元rowへの参照を持ち、同じ入力から常に同じ結果が得られることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn cells_keep_source_row_references_and_results_are_deterministic() {
    let (_directory, document) = document(&DATA);
    let rows = data_rows(&document);
    let first = pivot(&rows, &request()).unwrap();
    for _ in 0..5 {
        assert_eq!(pivot(&rows, &request()).unwrap(), first);
    }
    let cell = first.cells[0][0].as_ref().unwrap();
    assert_eq!(
        cell.source_rows,
        [
            SourceRowRef {
                source: "p.csv".into(),
                row: 1
            },
            SourceRowRef {
                source: "p.csv".into(),
                row: 5
            },
        ]
    );
}

#[test]
// {
//   責務: [input_filters_narrow_rows_before_aggregation: 入力filterの一致するrowだけが集計され、複数filterはすべて一致が必要で、filterで落ちたrowはexcluded_rowsに入らないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn input_filters_narrow_rows_before_aggregation() {
    let (_directory, document) = document(&DATA);
    let mut request = request();
    request.filters = vec![PivotFilter {
        column: 0,
        allowed: vec!["east".into(), "West".into()],
    }];
    let result = pivot(&data_rows(&document), &request).unwrap();
    // "West"は大文字小文字が違うので一致しない。"East"も同様に落ちる。
    assert_eq!(result.row_keys, [key("east")]);
    assert_eq!(result.column_keys, [key("1"), key("2")]);
    assert!(result.excluded_rows.is_empty());

    request.filters.push(PivotFilter {
        column: 1,
        allowed: vec!["2".into()],
    });
    let result = pivot(&data_rows(&document), &request).unwrap();
    assert_eq!(result.row_keys, [key("east")]);
    assert_eq!(result.column_keys, [key("2")]);
    assert_eq!(
        result.cells[0][0].as_ref().unwrap().values[0],
        Ok(CalculationValue::Integer(5))
    );
}

#[test]
// {
//   責務: [non_fresh_cells_are_excluded_and_reported: non-freshなfilter cell・key cellのrowはexcluded_rowsに入り、non-freshな値cellは集計から除いて診断に数えることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn non_fresh_cells_are_excluded_and_reported() {
    let cell = |text: &str, freshness| InputCell {
        text: text.to_owned(),
        freshness,
    };
    let row = |row: usize, cells: Vec<InputCell>| InputRow {
        source_row: SourceRowRef {
            source: "d".into(),
            row,
        },
        cells,
    };
    // 列: 0=region(filter) 1=dept(row) 2=month(column) 3=sales
    let rows = vec![
        row(
            0,
            vec![
                InputCell::fresh("n"),
                InputCell::fresh("a"),
                InputCell::fresh("1"),
                InputCell::fresh("10"),
            ],
        ),
        row(
            1,
            vec![
                InputCell::fresh("n"),
                InputCell::fresh("a"),
                InputCell::fresh("1"),
                cell("999", CellFreshness::Stale),
            ],
        ),
        row(
            2,
            vec![
                cell("n", CellFreshness::Error),
                InputCell::fresh("a"),
                InputCell::fresh("1"),
                InputCell::fresh("5"),
            ],
        ),
        row(
            3,
            vec![
                InputCell::fresh("n"),
                cell("a", CellFreshness::Pending),
                InputCell::fresh("1"),
                InputCell::fresh("5"),
            ],
        ),
    ];
    let request = PivotRequest {
        row_columns: vec![1],
        column_columns: vec![2],
        values: vec![AggregateSpec::Column {
            function: AggregateFunction::Sum,
            column: 3,
        }],
        filters: vec![PivotFilter {
            column: 0,
            allowed: vec!["n".into()],
        }],
    };
    let result = pivot(&rows, &request).unwrap();
    let cell = result.cells[0][0].as_ref().unwrap();
    assert_eq!(cell.values[0], Ok(CalculationValue::Integer(10)));
    assert_eq!(cell.diagnostics.stale, 1);
    let excluded: Vec<usize> = result.excluded_rows.iter().map(|row| row.row).collect();
    assert_eq!(excluded, [2, 3]);
}

#[test]
// {
//   責務: [empty_dimensions_and_bad_columns: Rows・Columnsが空なら1つのcellになり、列範囲外のkey・filter・値の指定を拒否することを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn empty_dimensions_and_bad_columns() {
    let (_directory, document) = document(&DATA);
    let rows = data_rows(&document);
    let result = pivot(
        &rows,
        &PivotRequest {
            row_columns: vec![],
            column_columns: vec![],
            values: vec![sum_sales()],
            filters: vec![],
        },
    )
    .unwrap();
    assert_eq!(result.row_keys, [Vec::<String>::new()]);
    assert_eq!(result.column_keys, [Vec::<String>::new()]);
    assert_eq!(
        result.cells[0][0].as_ref().unwrap().values[0],
        Ok(CalculationValue::Integer(43))
    );

    // 残るrowが無ければ格子は空。keyは実在するrowからだけ作る。
    let mut none = request();
    none.filters = vec![PivotFilter {
        column: 0,
        allowed: vec!["nobody".into()],
    }];
    for rows in [&rows[..0], &rows[..]] {
        let result = pivot(rows, &none).unwrap();
        assert!(result.row_keys.is_empty() && result.column_keys.is_empty());
        assert!(result.cells.is_empty() && result.excluded_rows.is_empty());
    }

    for request in [
        PivotRequest {
            row_columns: vec![9],
            ..request()
        },
        PivotRequest {
            column_columns: vec![9],
            ..request()
        },
        PivotRequest {
            filters: vec![PivotFilter {
                column: 9,
                allowed: vec![],
            }],
            ..request()
        },
        PivotRequest {
            values: vec![AggregateSpec::DistinctCount { column: 9 }],
            ..request()
        },
    ] {
        assert!(matches!(
            pivot(&rows, &request),
            Err(AggregationError::ColumnOutOfRange { column: 9, .. })
        ));
    }
}
