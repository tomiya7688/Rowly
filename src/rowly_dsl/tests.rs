use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::{CsvDocument, DocumentError};

// {
//   責務: [open: DSLテスト用CSVを一時ディレクトリに作成してCsvDocumentを開く。TempDirも返し、テスト中のfile lifetimeを保つ。]
// }
fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("data.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [parses_basic_style_if_column_checks_and_range_set: BASIC風のIf、列型・日本語チェック、範囲代入が期待するASTを生成することを確認する。]
// }
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
    let Statement::If {
        condition,
        body,
        else_body,
    } = &program.statements()[0]
    else {
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
    assert!(else_body.is_empty());
    assert_eq!(
        program.statements()[1],
        Statement::SetRangeValue {
            range: "A2:A3".parse().unwrap(),
            value: Expression::Literal("8".into()),
        }
    );
}

#[test]
// {
//   責務: [executes_column_type_and_japanese_checks: 列型・日本語検査のevent順、検査件数、cell参照を確認する。]
// }
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
// {
//   責務: [false_if_condition_skips_body: false条件ではIf bodyを実行せず、条件eventだけが記録されることを確認する。]
// }
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
// {
//   責務: [header_selector_and_range_set_use_process_boundary: header列選択と範囲編集がprocess経由で動き、複数cellをundoできることを確認する。]
// }
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
// {
//   責務: [transaction_commit_groups_multiple_dsl_edits_into_one_undo: DSLの複数編集をcommitすると1回のundo/redo単位になることを確認する。]
// }
fn transaction_commit_groups_multiple_dsl_edits_into_one_undo() {
    let (_directory, mut document) = open("Name,Score\nAlice,10\nBob,20\n");

    run(
        r#"
            BeginTransaction()
            SetCellValueAt(Integer("2"), Integer("2"), "42")
            SetCellValueAt(Integer("3"), Integer("2"), "99")
            CommitTransaction()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("99"));
    assert!(!document.transaction_active());

    assert!(document.undo().unwrap());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    assert!(!document.can_undo());

    assert!(document.redo().unwrap());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("99"));
}

#[test]
// {
//   責務: [transaction_rollback_restores_dsl_edits_without_history: rollbackが編集を復元し、transaction・history・dirty状態を残さないことを確認する。]
// }
fn transaction_rollback_restores_dsl_edits_without_history() {
    let (_directory, mut document) = open("Name,Score\nAlice,10\nBob,20\n");

    run(
        r#"
            BeginTransaction()
            This.Worksheet.Editor.Cell(B2 To B3).Value.Set = "changed"
            RollbackTransaction()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    assert!(!document.transaction_active());
    assert!(!document.can_undo());
    assert!(!document.is_dirty());
}

#[test]
// {
//   責務: [transaction_control_requires_zero_arguments_and_process_state_rules: transaction builtinの引数数と未開始transactionへのcommit失敗を確認する。]
// }
fn transaction_control_requires_zero_arguments_and_process_state_rules() {
    let (_directory, mut document) = open("Name,Score\nAlice,10\n");

    let error = run("BeginTransaction(\"unexpected\")", &mut document).unwrap_err();
    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ArgumentCount {
            name,
            expected: 0,
            actual: 1,
        }) if name == "BeginTransaction"
    ));

    let error = run("CommitTransaction()", &mut document).unwrap_err();
    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::Document(DocumentError::Transaction(_)))
    ));
}

#[test]
// {
//   責務: [variables_can_feed_edits_and_conditions: variable値をcell編集と条件へ渡し、編集結果を確認する。]
// }
fn variables_can_feed_edits_and_conditions() {
    let (_directory, mut document) = open("名前\n田中\n山田\n");
    let report = run(
        r#"
            VAR replacement = "佐藤"
            VAR expected = "名前"
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
// {
//   責務: [functions_accept_arguments_return_values_and_edit_through_process_api: function引数・戻り値とprocess経由の編集を組み合わせて確認する。]
// }
fn functions_accept_arguments_return_values_and_edit_through_process_api() {
    let (_directory, mut document) = open("値\n1\n2\n");
    let report = run(
        r#"
            Def Fill(value)
                This.Worksheet.Editor.Cell(A2 To A3).Value.Set = value
                Return value
            End Def

            VAR result = Fill("8")
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
// {
//   責務: [function_scope_does_not_overwrite_global_variables: function local bindingがglobal bindingを上書きしないことを確認する。]
// }
fn function_scope_does_not_overwrite_global_variables() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR value = "global"

            Def Echo(value)
                value = "local"
                Return value
            End Def

            VAR result = Echo("argument")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("value"), Some("global"));
    assert_eq!(report.variable("result"), Some("local"));
}

#[test]
// {
//   責務: [return_inside_nested_if_exits_function: nested If内のReturnが関数を終了させ、値を返すことを確認する。]
// }
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

            VAR result = Choose("ok")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("ok"));
}

#[test]
// {
//   責務: [function_used_as_value_requires_return_value: 値式に使ったfunctionに戻り値がないと明示エラーになることを確認する。]
// }
fn function_used_as_value_requires_return_value() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Def NoReturn()
                This.Worksheet.Column(1).Type = String
            End Def

            VAR result = NoReturn()
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
// {
//   責務: [argument_count_is_checked: 実引数数とparameter数が異なる呼び出しのerrorを確認する。]
// }
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
// {
//   責務: [reports_missing_end_if_at_opening_line: End If欠落を開始行番号付きで報告することを確認する。]
// }
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
// {
//   責務: [reports_missing_end_def_at_opening_line: End Def欠落を開始行番号付きで報告することを確認する。]
// }
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
