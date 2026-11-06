use rowly::process::{
    CsvDocument,
    aggregation::{
        AggregateSpec, AggregationError, CellFreshness, GroupByRequest, InputCell, InputRow,
        SourceRowRef, group_by, rows_from_document,
    },
    calculation::{AggregateFunction, CalculationFailure, CalculationValue},
};
use tempfile::tempdir;

fn document(rows: &[[&str; 3]]) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let mut table = vec![["dept", "month", "sales"].map(String::from).to_vec()];
    table.extend(rows.iter().map(|row| row.map(String::from).to_vec()));
    let document = CsvDocument::create(directory.path().join("g.csv"), table).unwrap();
    (directory, document)
}

const DATA: [[&str; 3]; 5] = [
    ["east", "1", "10"],
    ["west", "1", "20"],
    ["east", "2", "5.5"],
    ["East", "2", "7"],
    ["west", "1", ""],
];

fn request() -> GroupByRequest {
    GroupByRequest {
        key_columns: vec![0],
        aggregates: vec![
            AggregateSpec::CountRows,
            AggregateSpec::Column {
                function: AggregateFunction::Sum,
                column: 2,
            },
            AggregateSpec::Column {
                function: AggregateFunction::Average,
                column: 2,
            },
            AggregateSpec::Column {
                function: AggregateFunction::Min,
                column: 2,
            },
            AggregateSpec::Column {
                function: AggregateFunction::Max,
                column: 2,
            },
            AggregateSpec::Column {
                function: AggregateFunction::CountA,
                column: 2,
            },
            AggregateSpec::DistinctCount { column: 1 },
        ],
    }
}

fn int(value: i64) -> Result<CalculationValue, CalculationFailure> {
    Ok(CalculationValue::Integer(value))
}

#[test]
// {
//   責務: [group_by_uses_raw_keys_and_leaves_the_document_unchanged: raw keyでgroupを分け(大文字小文字も別)、集計値が期待どおりで、元documentが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn group_by_uses_raw_keys_and_leaves_the_document_unchanged() {
    let (_directory, document) = document(&DATA);
    let before: Vec<Vec<String>> = (0..document.row_count())
        .map(|row| {
            (0..3)
                .map(|column| document.cell(row, column).unwrap().to_owned())
                .collect()
        })
        .collect();
    let rows = rows_from_document(&document, "g.csv", 1..document.row_count());
    let result = group_by(&rows, &request()).unwrap();

    let keys: Vec<_> = result.groups.iter().map(|g| g.keys[0].as_str()).collect();
    assert_eq!(keys, ["east", "west", "East"]);
    // east: 10 + 5.5、west: 20(空は除外)、East: 7。
    let east = &result.groups[0];
    assert_eq!(east.values[0], int(2));
    assert_eq!(east.values[1], Ok(CalculationValue::Decimal(15.5)));
    assert_eq!(east.values[2], Ok(CalculationValue::Decimal(7.75)));
    assert_eq!(east.values[3], Ok(CalculationValue::Decimal(5.5)));
    assert_eq!(east.values[4], int(10));
    assert_eq!(east.values[5], int(2));
    assert_eq!(east.values[6], int(2));
    let west = &result.groups[1];
    assert_eq!(west.values[0], int(2));
    assert_eq!(west.values[1], int(20));
    assert_eq!(west.values[5], int(1));
    assert_eq!(west.values[6], int(1));

    let after: Vec<Vec<String>> = (0..document.row_count())
        .map(|row| {
            (0..3)
                .map(|column| document.cell(row, column).unwrap().to_owned())
                .collect()
        })
        .collect();
    assert_eq!(before, after);
    assert!(!document.is_dirty());
}

#[test]
// {
//   責務: [aggregate_cells_keep_source_row_provenance_across_sources: 各groupから元rowへ戻れ、複数sourceのprovenanceが保たれることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn aggregate_cells_keep_source_row_provenance_across_sources() {
    let (_d1, first) = document(&DATA[..2]);
    let (_d2, second) = document(&DATA[2..3]);
    let mut rows = rows_from_document(&first, "first.csv", 1..3);
    rows.extend(rows_from_document(&second, "second.csv", 1..2));
    let result = group_by(&rows, &request()).unwrap();
    let east = result.groups.iter().find(|g| g.keys[0] == "east").unwrap();
    assert_eq!(
        east.source_rows,
        [
            SourceRowRef {
                source: "first.csv".into(),
                row: 1
            },
            SourceRowRef {
                source: "second.csv".into(),
                row: 1
            },
        ]
    );
    assert_eq!(east.values[1], Ok(CalculationValue::Decimal(15.5)));
}

#[test]
// {
//   責務: [input_scope_is_chosen_by_the_caller: 全row入力とView filter後の部分入力で結果が変わることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn input_scope_is_chosen_by_the_caller() {
    let (_directory, document) = document(&DATA);
    let filtered = rows_from_document(&document, "g.csv", [1, 3]);
    let result = group_by(&filtered, &request()).unwrap();
    assert_eq!(result.groups.len(), 1);
    assert_eq!(
        result.groups[0].values[1],
        int(15).map(|_| CalculationValue::Decimal(15.5))
    );
}

#[test]
// {
//   責務: [stale_error_pending_values_are_excluded_and_reported_not_zeroed: non-freshな値を集計から除き診断に数え、non-freshなkeyのrowはgroupへ入れないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn stale_error_pending_values_are_excluded_and_reported_not_zeroed() {
    let cell = |text: &str, freshness| InputCell {
        text: text.to_owned(),
        freshness,
    };
    let row = |row: usize, key: InputCell, value: InputCell| InputRow {
        source_row: SourceRowRef {
            source: "d".into(),
            row,
        },
        cells: vec![key, value],
    };
    let rows = vec![
        row(0, InputCell::fresh("a"), InputCell::fresh("10")),
        row(1, InputCell::fresh("a"), cell("999", CellFreshness::Stale)),
        row(2, InputCell::fresh("a"), cell("999", CellFreshness::Error)),
        row(
            3,
            InputCell::fresh("a"),
            cell("999", CellFreshness::Pending),
        ),
        row(4, cell("a", CellFreshness::Stale), InputCell::fresh("1")),
    ];
    let request = GroupByRequest {
        key_columns: vec![0],
        aggregates: vec![
            AggregateSpec::CountRows,
            AggregateSpec::Column {
                function: AggregateFunction::Sum,
                column: 1,
            },
        ],
    };
    let result = group_by(&rows, &request).unwrap();
    assert_eq!(result.groups.len(), 1);
    let group = &result.groups[0];
    assert_eq!(group.values[0], int(4));
    assert_eq!(group.values[1], int(10));
    assert_eq!(
        (
            group.diagnostics.stale,
            group.diagnostics.error,
            group.diagnostics.pending
        ),
        (1, 1, 1)
    );
    assert_eq!(result.excluded_rows.len(), 1);
    assert_eq!(result.excluded_rows[0].row, 4);
}

#[test]
// {
//   責務: [errors_stay_inside_their_aggregate_and_bad_columns_are_rejected: 数値でない値のSUMがその集計だけのerrorになり、列範囲外の要求を拒否し、keyなしで全体を1 groupにできることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn errors_stay_inside_their_aggregate_and_bad_columns_are_rejected() {
    let (_directory, document) = document(&[["a", "1", "x"], ["a", "1", "2"]]);
    let rows = rows_from_document(&document, "g.csv", 1..3);
    let request = GroupByRequest {
        key_columns: vec![],
        aggregates: vec![
            AggregateSpec::CountRows,
            AggregateSpec::Column {
                function: AggregateFunction::Sum,
                column: 2,
            },
        ],
    };
    let result = group_by(&rows, &request).unwrap();
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].values[0], int(2));
    assert_eq!(
        result.groups[0].values[1],
        Err(CalculationFailure::ExpectedNumber("x".to_owned()))
    );
    let bad = GroupByRequest {
        key_columns: vec![9],
        aggregates: vec![],
    };
    assert_eq!(
        group_by(&rows, &bad),
        Err(AggregationError::ColumnOutOfRange { column: 9, row: 0 })
    );
    assert!(group_by(&[], &request).unwrap().groups.is_empty());
}

#[test]
// {
//   責務: [shared_cells_are_counted_once_and_duplicate_row_indices_are_ignored: 同じnon-fresh cellを複数の集計列が参照しても診断は1回だけ数え、重複したrow indexは二重計上しないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn shared_cells_are_counted_once_and_duplicate_row_indices_are_ignored() {
    let rows = vec![InputRow {
        source_row: SourceRowRef {
            source: "d".into(),
            row: 0,
        },
        cells: vec![
            InputCell::fresh("a"),
            InputCell {
                text: "9".to_owned(),
                freshness: CellFreshness::Stale,
            },
        ],
    }];
    let request = GroupByRequest {
        key_columns: vec![0],
        aggregates: vec![
            AggregateSpec::Column {
                function: AggregateFunction::CountA,
                column: 1,
            },
            AggregateSpec::Column {
                function: AggregateFunction::Count,
                column: 1,
            },
            AggregateSpec::DistinctCount { column: 1 },
        ],
    };
    let result = group_by(&rows, &request).unwrap();
    assert_eq!(result.groups[0].diagnostics.stale, 1);

    let (_directory, document) = document(&DATA);
    let rows = rows_from_document(&document, "g.csv", [1, 1, 3, 1, 99]);
    assert_eq!(rows.len(), 2);
}
