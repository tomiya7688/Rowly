use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("arithmetic.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn arithmetic_precedence_and_parentheses_work() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR two = Integer("2")
            VAR three = Integer("3")
            VAR four = Integer("4")
            VAR first = two + three * four
            VAR second = (two + three) * four
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("first"), Some("14"));
    assert_eq!(report.variable("second"), Some("20"));
}

#[test]
fn subtraction_is_left_associative_and_unary_minus_works() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR ten = Integer("10")
            VAR three = Integer("3")
            VAR two = Integer("2")
            VAR result = ten - three - two
            VAR negative = -result
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("5"));
    assert_eq!(report.variable("negative"), Some("-5"));
}

#[test]
fn mixed_numeric_arithmetic_promotes_to_decimal() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR integer = Integer("5")
            VAR decimal = Decimal("2.5")
            VAR sum = integer + decimal
            VAR product = decimal * integer
            VAR quotient = integer / Integer("2")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("sum"), Some("7.5"));
    assert_eq!(report.variable("product"), Some("12.5"));
    assert_eq!(report.variable("quotient"), Some("2.5"));
}

#[test]
fn arithmetic_can_feed_comparisons_and_csv_edits() {
    let (_directory, mut document) = open("値\nold\n");
    let report = run(
        r#"
            VAR left = Integer("6")
            VAR right = Integer("4")
            VAR total = left + right

            If total >= Integer("10") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = total / Integer("4")
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("total"), Some("10"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("2.5"));
}

#[test]
fn function_and_method_results_participate_in_arithmetic() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Def Double(value)
                Return value * Integer("2")
            End Def

            Class Counter
                Field value = Integer("3")

                Def Next()
                    Return Self.value + Integer("1")
                End Def
            End Class

            VAR counter = New Counter()
            VAR result = Double(counter.Next()) + Integer("1")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("9"));
}

#[test]
fn division_by_zero_is_explicit() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Integer("10") / Integer("0")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("division by zero"));
}

#[test]
fn arithmetic_rejects_non_numeric_values() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = "10" + Integer("2")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot apply arithmetic operator `+` to String and Integer")
    );
}

#[test]
fn unary_minus_rejects_non_numeric_values() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = -"10"
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot apply unary operator `-` to String")
    );
}

#[test]
fn integer_overflow_is_explicit() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR max = Integer("9223372036854775807")
            VAR value = max + Integer("1")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("arithmetic overflow"));
}
