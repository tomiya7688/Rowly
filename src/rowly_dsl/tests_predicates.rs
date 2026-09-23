use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("predicates.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn parser_uses_dedicated_ast_for_standard_namespace_calls() {
    let program = parse(r#"VAR result = Text.Contains("abc", "b")"#).unwrap();

    match &program.statements()[0] {
        Statement::Declare {
            value:
                Expression::StandardCall {
                    namespace: StandardNamespace::Text,
                    name,
                    arguments,
                },
            ..
        } => {
            assert_eq!(name, "Contains");
            assert_eq!(arguments.len(), 2);
        }
        other => panic!("expected namespaced standard call, got {other:?}"),
    }
}

#[test]
fn namespaced_string_predicates_can_be_used_directly_as_conditions() {
    let (_directory, mut document) = open("値\nold\n");
    run(
        r#"
            VAR value = "東京都"
            If Text.Contains(value, "東京") And Text.StartsWith(value, "東") And Text.EndsWith(value, "都") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "matched"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("matched"));
}

#[test]
fn text_is_japanese_matches_mixed_text_containing_japanese() {
    let (_directory, mut document) = open("値\nold\n");
    let report = run(
        r#"
            VAR mixed = Text.IsJapanese("abc日本語123")
            VAR latin = Text.IsJapanese("abc123")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("mixed"), Some("true"));
    assert_eq!(report.variable("latin"), Some("false"));
}

#[test]
fn namespaced_type_predicates_accept_text_and_typed_values() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR integerText = Number.IsInteger("42")
            VAR integerTyped = Number.IsInteger(Integer("42"))
            VAR decimalText = Number.IsDecimal("12.5")
            VAR decimalInteger = Number.IsDecimal(Integer("12"))
            VAR booleanText = Boolean.IsValid("TRUE")
            VAR badInteger = Number.IsInteger("12.5")
            VAR badBoolean = Boolean.IsValid("yes")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("integertext"), Some("true"));
    assert_eq!(report.variable("integertyped"), Some("true"));
    assert_eq!(report.variable("decimaltext"), Some("true"));
    assert_eq!(report.variable("decimalinteger"), Some("true"));
    assert_eq!(report.variable("booleantext"), Some("true"));
    assert_eq!(report.variable("badinteger"), Some("false"));
    assert_eq!(report.variable("badboolean"), Some("false"));
}

#[test]
fn boolean_return_values_from_standard_namespaces_can_drive_if_conditions() {
    let (_directory, mut document) = open("値\nold\n");
    run(
        r#"
            Def Valid(value)
                Return Number.IsInteger(value)
            End Def

            If Valid("123") Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "ok"
            End If
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("ok"));
}

#[test]
fn standard_namespaces_and_functions_are_case_insensitive() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            VAR textMatch = text.contains("ABC", "B")
            VAR numberMatch = NUMBER.isinteger("42")
            VAR booleanMatch = boolean.ISVALID("false")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("textmatch"), Some("true"));
    assert_eq!(report.variable("numbermatch"), Some("true"));
    assert_eq!(report.variable("booleanmatch"), Some("true"));
}

#[test]
fn non_boolean_expression_condition_is_rejected() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            If "text" Then
                This.Worksheet.Editor.Cell(A2).Value.Set = "x"
            End If
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("condition requires a Boolean value")
    );
}

#[test]
fn namespaced_string_predicates_require_text_arguments() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Text.Contains(Integer("12"), "1")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Text.Contains first argument requires a text value")
    );
}

#[test]
fn namespaced_predicate_argument_count_is_validated() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Text.Contains("abc")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("function `Text.Contains` expects 2 arguments but received 1")
    );
}

#[test]
fn old_global_predicate_builtins_are_not_available() {
    for old_call in [
        r#"Contains("abc", "a")"#,
        r#"StartsWith("abc", "a")"#,
        r#"EndsWith("abc", "c")"#,
        r#"IsJapanese("日本語")"#,
        r#"IsInteger("1")"#,
        r#"IsDecimal("1.5")"#,
        r#"IsBoolean("true")"#,
    ] {
        let (_directory, mut document) = open("値\n1\n");
        let source = format!("VAR result = {old_call}");
        let error = run(&source, &mut document).unwrap_err();
        assert!(
            matches!(error, DslError::Execute(ExecutionError::UnknownFunction(_))),
            "{old_call}: {error}"
        );
    }
}

#[test]
fn unknown_standard_namespace_function_is_explicit() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            VAR value = Text.Unknown("abc")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(error.to_string().contains("Text.Unknown"));
    assert!(error.to_string().contains("unknown Rowly DSL standard function"));
}

#[test]
fn standard_namespace_names_are_reserved_for_bindings() {
    for name in ["Text", "Number", "Boolean"] {
        let error = parse(&format!("VAR {name} = \"value\"")).unwrap_err();
        assert!(error.to_string().contains("reserved binding name"), "{name}");

        let error = parse(&format!("Def F({name})\nReturn \"x\"\nEnd Def")).unwrap_err();
        assert!(error.to_string().contains("reserved binding name"), "{name}");

        let error =
            parse(&format!("For {name} = Integer(\"1\") To Integer(\"1\")\nNext {name}"))
                .unwrap_err();
        assert!(error.to_string().contains("reserved binding name"), "{name}");
    }
}

#[test]
fn legacy_predicate_names_can_still_be_user_defined_functions() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Def IsInteger(value)
                Return "user"
            End Def

            VAR result = IsInteger("not-a-number")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("user"));
}

#[test]
fn user_classes_with_namespace_names_do_not_replace_standard_namespaces() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Text
                Def Contains(value, needle)
                    Return "user-method"
                End Def
            End Class

            VAR tool = New Text()
            VAR userResult = tool.Contains("a", "b")
            VAR standardResult = Text.Contains("abc", "b")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("userresult"), Some("user-method"));
    assert_eq!(report.variable("standardresult"), Some("true"));
}
