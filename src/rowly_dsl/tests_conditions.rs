use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("conditions.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn else_executes_when_condition_is_false() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" = "b" Then
                Let result = "then"
            Else
                Let result = "else"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("else"));
}

#[test]
fn logical_precedence_is_not_then_and_then_or() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If Not "a" = "b" And "x" = "x" Or "z" = "q" Then
                Let result = "yes"
            Else
                Let result = "no"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("yes"));
}

#[test]
fn and_and_or_short_circuit_rhs() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" != "a" And missing = "boom" Then
                Let first = "bad"
            Else
                Let first = "ok"
            End If

            If "a" = "a" Or missing = "boom" Then
                Let second = "ok"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("first"), Some("ok"));
    assert_eq!(report.variable("second"), Some("ok"));
}

#[test]
fn comparison_operators_use_text_ordering() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "a" < "b" And "b" <= "b" And "c" > "b" And "c" >= "c" And "a" != "z" Then
                Let result = "ordered"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("ordered"));
}

#[test]
fn parenthesized_conditions_override_precedence() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If ("a" = "b" Or "x" = "x") And Not ("q" = "q") Then
                Let result = "bad"
            Else
                Let result = "good"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("good"));
}

#[test]
fn class_methods_use_the_same_condition_evaluator() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Choice
                Field value = "b"

                Def Pick()
                    If Self.value >= "b" And Not Self.value = "z" Then
                        Return "match"
                    Else
                        Return "fallback"
                    End If
                End Def
            End Class

            Let choice = New Choice()
            Let result = choice.Pick()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("match"));
}

#[test]
fn else_supports_nested_if_blocks() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            If "outer" = "outer" Then
                If "inner" != "inner" Then
                    Let result = "bad"
                Else
                    Let result = "nested"
                End If
            Else
                Let result = "bad"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("nested"));
}
