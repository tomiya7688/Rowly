use tempfile::{TempDir, tempdir};

use super::{ProfileAccumulator, ProfileOptions, ProfileScope};
use crate::process::{ColumnType, CsvDocument, DocumentError, ValidationRule, ValidationTarget};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// {
// 責務: [
// document: 指定行から未編集のCSV documentを作る
// ]
// 引数: [
// rows: &[&[&str]]
// ]
// 戻り値: [
// Result<(TempDir, CsvDocument), Box<dyn Error>>: 一時directoryと未編集document
// ]
// }
fn document(rows: &[&[&str]]) -> Result<(TempDir, CsvDocument), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let rows = rows
        .iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect();
    let document = CsvDocument::create(directory.path().join("t.csv"), rows)?;
    Ok((directory, document))
}

#[test]
fn counts_rows_empty_distinct_and_duplicates_without_changing_the_csv() -> TestResult {
    let (_d, doc) = document(&[&["v"], &["a"], &["b"], &["a"], &[""], &["a"]])?;
    let before = doc.csv_text()?;
    let profile = doc.profile_column(0, &ProfileOptions::default())?;
    assert_eq!(profile.rows, 5);
    assert_eq!(profile.empty, 1);
    assert_eq!(profile.non_empty, 4);
    assert_eq!(profile.distinct, Some(3));
    assert_eq!(profile.duplicate, Some(2));
    assert_eq!(profile.column_type, None);
    assert_eq!(profile.parse_failures, None);
    assert_eq!(doc.csv_text()?, before);
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    Ok(())
}

#[test]
fn raw_distinct_separates_leading_zero_values() -> TestResult {
    let (_d, doc) = document(&[&["id"], &["00123"], &["123"], &["123"]])?;
    let profile = doc.profile_column(0, &ProfileOptions::default())?;
    assert_eq!(profile.distinct, Some(2));
    assert_eq!(profile.frequent[0].value, "123");
    assert_eq!(profile.frequent[0].count, 2);
    assert_eq!(profile.frequent[1].value, "00123");
    Ok(())
}

#[test]
fn frequent_values_are_ordered_by_count_then_value_and_truncated() -> TestResult {
    let (_d, doc) = document(&[&["v"], &["b"], &["a"], &["c"], &["c"], &["b"]])?;
    let options = ProfileOptions {
        top_values: 2,
        ..ProfileOptions::default()
    };
    let profile = doc.profile_column(0, &options)?;
    let values: Vec<&str> = profile.frequent.iter().map(|f| f.value.as_str()).collect();
    assert_eq!(values, ["b", "c"]);
    Ok(())
}

#[test]
fn lengths_count_unicode_characters() -> TestResult {
    let (_d, doc) = document(&[&["v"], &["あい"], &["a"], &["日本語です"]])?;
    let profile = doc.profile_column(0, &ProfileOptions::default())?;
    assert_eq!(profile.min_length, Some(1));
    assert_eq!(profile.max_length, Some(5));
    Ok(())
}

#[test]
fn explicit_integer_type_reports_min_max_and_parse_failures_separately() -> TestResult {
    let (_d, doc) = document(&[&["n"], &["10"], &["-3"], &["x"], &[""], &["7"], &["1.5"]])?;
    let options = ProfileOptions {
        column_type: Some(ColumnType::Integer),
        ..ProfileOptions::default()
    };
    let profile = doc.profile_column(0, &options)?;
    assert_eq!(profile.parse_failures, Some(2));
    assert_eq!(profile.empty, 1);
    let range = profile.numeric_range.ok_or("range")?;
    assert_eq!((range.min.as_str(), range.max.as_str()), ("-3", "10"));
    Ok(())
}

#[test]
fn explicit_decimal_type_keeps_raw_text_of_min_and_max() -> TestResult {
    let (_d, doc) = document(&[&["n"], &["1.50"], &["0.25"], &["NaN"], &["2"]])?;
    let options = ProfileOptions {
        column_type: Some(ColumnType::Decimal),
        ..ProfileOptions::default()
    };
    let profile = doc.profile_column(0, &options)?;
    assert_eq!(profile.parse_failures, Some(1));
    let range = profile.numeric_range.ok_or("range")?;
    assert_eq!((range.min.as_str(), range.max.as_str()), ("0.25", "2"));
    Ok(())
}

#[test]
fn boolean_type_counts_true_false_and_failures() -> TestResult {
    let (_d, doc) = document(&[&["b"], &["true"], &["FALSE"], &["true"], &["maybe"]])?;
    let options = ProfileOptions {
        column_type: Some(ColumnType::Boolean),
        ..ProfileOptions::default()
    };
    let profile = doc.profile_column(0, &options)?;
    assert_eq!(profile.boolean_counts, Some((2, 1)));
    assert_eq!(profile.parse_failures, Some(1));
    assert_eq!(profile.numeric_range, None);
    Ok(())
}

#[test]
fn numbers_are_not_inferred_without_an_explicit_type() -> TestResult {
    let (_d, doc) = document(&[&["n"], &["1"], &["2"]])?;
    let profile = doc.profile_column(0, &ProfileOptions::default())?;
    assert_eq!(profile.numeric_range, None);
    assert_eq!(profile.parse_failures, None);
    assert!(doc.column_type_declarations().next().is_none());
    Ok(())
}

#[test]
fn validation_violations_match_the_validation_report_per_scope() -> TestResult {
    let (_d, mut doc) = document(&[&["v"], &["a"], &["b"], &["a"]])?;
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["a".to_owned(), "b".to_owned()]),
    )?;
    assert_eq!(
        doc.profile_column(0, &ProfileOptions::default())?
            .validation_violations,
        0
    );
    // 規則を外部変更で破った状態を、規則の差し替えで作る
    doc.set_validation_rule(
        ValidationTarget::Index(0),
        ValidationRule::AllowedValues(vec!["a".to_owned()]),
    )?;
    let report = doc.validation_report();
    let all = doc.profile_column(0, &ProfileOptions::default())?;
    assert_eq!(all.validation_violations, report.violations().len());
    assert_eq!(all.validation_violations, 1);
    let scoped = ProfileOptions {
        scope: ProfileScope::Rows(vec![1, 3]),
        ..ProfileOptions::default()
    };
    let part = doc.profile_column(0, &scoped)?;
    let expected = report
        .violations()
        .iter()
        .filter(|v| [1, 3].contains(&v.cell().row()))
        .count();
    assert_eq!(part.validation_violations, expected);
    assert_eq!(part.validation_violations, 0);
    Ok(())
}

#[test]
fn selection_scope_counts_only_those_rows_and_dedups() -> TestResult {
    let (_d, doc) = document(&[&["v"], &["a"], &["b"], &["c"]])?;
    let options = ProfileOptions {
        scope: ProfileScope::Rows(vec![3, 1, 3]),
        ..ProfileOptions::default()
    };
    let profile = doc.profile_column(0, &options)?;
    assert_eq!(profile.rows, 2);
    assert_eq!(profile.distinct, Some(2));
    Ok(())
}

#[test]
fn invalid_scope_or_column_is_rejected() -> TestResult {
    let (_d, doc) = document(&[&["v"], &["a"]])?;
    for rows in [vec![0], vec![5]] {
        let options = ProfileOptions {
            scope: ProfileScope::Rows(rows),
            ..ProfileOptions::default()
        };
        assert!(matches!(
            doc.profile_column(0, &options),
            Err(DocumentError::Edit(_))
        ));
    }
    assert!(matches!(
        doc.profile_column(3, &ProfileOptions::default()),
        Err(DocumentError::Edit(_))
    ));
    Ok(())
}

#[test]
fn multi_source_merge_equals_profiling_the_combined_values() -> TestResult {
    let (_a, first) = document(&[&["n"], &["1"], &["x"], &["1"]])?;
    let (_b, second) = document(&[&["n"], &["9"], &[""], &["1"]])?;
    let options = ProfileOptions {
        column_type: Some(ColumnType::Integer),
        ..ProfileOptions::default()
    };
    let mut merged = first.profile_accumulator(0, &options)?;
    merged.merge(&second.profile_accumulator(0, &options)?);
    let profile = merged.finish(0, 10, 0);

    let (_c, combined) = document(&[&["n"], &["1"], &["x"], &["1"], &["9"], &[""], &["1"]])?;
    let expected = combined.profile_column(0, &options)?;
    assert_eq!(profile, expected);
    assert_eq!(profile.rows, 6);
    assert_eq!(profile.parse_failures, Some(1));
    Ok(())
}

#[test]
fn exceeding_the_distinct_limit_reports_not_measured_instead_of_approximating() {
    let mut accumulator = ProfileAccumulator::new(None, 2);
    for value in ["a", "b", "c", "a"] {
        accumulator.add(value);
    }
    let profile = accumulator.finish(0, 5, 0);
    assert_eq!(profile.rows, 4);
    assert_eq!(profile.distinct, None);
    assert_eq!(profile.duplicate, None);
    assert!(profile.frequent.is_empty());
}

#[test]
fn merging_past_the_limit_also_reports_not_measured() {
    let mut left = ProfileAccumulator::new(None, 2);
    left.add("a");
    left.add("b");
    let mut right = ProfileAccumulator::new(None, 2);
    right.add("c");
    left.merge(&right);
    assert_eq!(left.finish(0, 5, 0).distinct, None);
}
