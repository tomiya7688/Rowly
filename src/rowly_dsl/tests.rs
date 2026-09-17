use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("data.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn parses_basic_style_if_column_checks_and_range_set() {
    let program = parse(
        r#"
            If This.Worksheet.Column(1).Title = "名前" Then
                This.Worksheet.Column(1).Type = String
                This.Worksheet.Column(1).Check.Japanese
            End If

            This.Worksheet.Editor.Cell(A2 To A3).Value.Set = 8
        "#,
    )
    .unwrap();

    assert!(program.functions().is_empty());
    assert_eq!(program.statements().len(), 2);
    let Statement::If { condition, body } = &program.statements()[0] else {
        panic!("expected if statement");
    };
    assert_eq!(
        condition,
        &Condition::ColumnTitleEquals {
            selector: ColumnSelector::Index(0),
            expected: Expression::Literal("名前".into()),
        }
    );
    assert_eq!(body.len(), 2);
    assert_eq!(
        program.statements()[1],
        Statement::SetRangeValue {
            range: "A2:A3".parse().unwrap(),
            value: Expression::Literal("8".into()),
        }
    );
}

#[test]
fn executes_column_type_and_japanese_checks() {
    let (_directory, mut document) = open("名前,年齢\n田中太郎,20\nAlice,21\n");
    let execution = run(
        r#"
            If This.Worksheet.Column(1).Title = "名前" Then
                This.Worksheet.Column(1).Type = String
                This.Worksheet.Column(1).Check.Japanese
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(execution.events().len(), 3);
    assert!(matches!(
        &execution.events()[0],
        ExecutionEvent::ConditionEvaluated { result: true, .. }
    ));
    let ExecutionEvent::ColumnTypeChecked {
        report: type_report,
        ..
    } = &execution.events()[1]
    else {
        panic!("expected type report");
    };
    assert!(type_report.is_valid());
    assert_eq!(type_report.checked_cells(), 2);

    let ExecutionEvent::JapaneseChecked {
        report: japanese_report,
        ..
    } = &execution.events()[2]
    else {
        panic!("expected Japanese report");
    };
    assert_eq!(japanese_report.matches().len(), 1);
    assert_eq!(japanese_report.mismatches().len(), 1);
    assert_eq!(
        japanese_report.mismatches()[0].reference().to_string(),
        "A3"
    );
}

#[test]
fn false_if_condition_skips_body() {
    let (_directory, mut document) = open("氏名\n田中\n");
    let report = run(
        r#"
            If This.Worksheet.Column(1).Title = "名前" Then
                This.Worksheet.Column(1).Check.Japanese
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.events().len(), 1);
    assert!(matches!(
        &report.events()[0],
        ExecutionEvent::ConditionEvaluated { result: false, .. }
    ));
}

#[test]
fn header_selector_and_range_set_use_process_boundary() {
    let (_directory, mut document) = open("名前,年齢\n田中,20\n山田,21\n");
    let report = run(
        r#"
            This.Worksheet.Column("年齢").Type = Integer
            This.Worksheet.Editor.Cell("B2:B3").Value.Set = 30
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.events().len(), 2);
    assert_eq!(document.cell_a1("B2").unwrap(), Some("30"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("30"));
    assert!(document.can_undo());
    assert!(document.undo().unwrap());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("20"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("21"));
}

#[test]
fn variables_can_feed_edits_and_conditions() {
    let (_directory, mut document) = open("名前\n田中\n山田\n");
    let report = run(
        r#"
            Let replacement = "佐藤"
            Let expected = "名前"
            If expected = "名前" Then
                This.Worksheet.Editor.Cell(A2 To A3).Value.Set = replacement
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("REPLACEMENT"), Some("佐藤"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("佐藤"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("佐藤"));
}

#[test]
fn functions_accept_arguments_return_values_and_edit_through_process_api() {
    let (_directory, mut document) = open("値\n1\n2\n");
    let report = run(
        r#"
            Def Fill(value)
                This.Worksheet.Editor.Cell(A2 To A3).Value.Set = value
                Return value
            End Def

            Let result = Fill("8")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("8"));
    assert_eq!(document.cell_a1("A2").unwrap(), Some("8"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("8"));
    assert!(report.events().iter().any(|event| matches!(
        event,
        ExecutionEvent::FunctionCalled {
            name,
            return_value: Some(value),
            ..
        } if name == "Fill" && value == "8"
    )));
    assert!(document.undo().unwrap());
    assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("2"));
}

#[test]
fn function_scope_does_not_overwrite_global_variables() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Let value = "global"

            Def Echo(value)
                Let value = "local"
                Return value
            End Def

            Let result = Echo("argument")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("value"), Some("global"));
    assert_eq!(report.variable("result"), Some("local"));
}

#[test]
fn return_inside_nested_if_exits_function() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Def Choose(value)
                If This.Worksheet.Column(1).Exists Then
                    Return value
                End If
                Return "fallback"
            End Def

            Let result = Choose("ok")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("ok"));
}

#[test]
fn function_used_as_value_requires_return_value() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Def NoReturn()
                This.Worksheet.Column(1).Type = String
            End Def

            Let result = NoReturn()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::MissingReturnValue(name)) if name == "NoReturn"
    ));
}

#[test]
fn argument_count_is_checked() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Def Echo(value)
                Return value
            End Def

            Echo()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ArgumentCount {
            expected: 1,
            actual: 0,
            ..
        })
    ));
}

#[test]
fn reports_missing_end_if_at_opening_line() {
    let error = parse(
        r#"
            If This.Worksheet.Column("名前").Exists Then
                This.Worksheet.Column("名前").Type = String
        "#,
    )
    .unwrap_err();

    assert_eq!(error.line(), 2);
    assert!(error.message().contains("End If"));
}

#[test]
fn reports_missing_end_def_at_opening_line() {
    let error = parse(
        r#"
            Def Fill(value)
                Return value
        "#,
    )
    .unwrap_err();

    assert_eq!(error.line(), 2);
    assert!(error.message().contains("End Def"));
}
