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
fn parses_class_fields_methods_and_new_expression() {
    let program = parse(
        r#"
            Class Formatter
                Field prefix = "@"

                Def Format(value)
                    Return Self.prefix
                End Def
            End Class

            Let formatter = New Formatter()
        "#,
    )
    .unwrap();

    assert_eq!(program.classes().len(), 1);
    let class = &program.classes()[0];
    assert_eq!(class.name(), "Formatter");
    assert_eq!(class.fields().len(), 1);
    assert_eq!(class.fields()[0].name(), "prefix");
    assert_eq!(class.methods().len(), 1);
    assert_eq!(class.methods()[0].name(), "Format");
    assert!(matches!(
        &program.statements()[0],
        Statement::Let {
            value: Expression::New { class_name },
            ..
        } if class_name == "Formatter"
    ));
}

#[test]
fn instance_fields_can_be_read_and_written() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Box
                Field value = "initial"
            End Class

            Let box = New Box()
            box.value = "updated"
            Let copied = box.value
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("copied"), Some("updated"));
    assert_eq!(report.object_field("box", "value"), Some("updated"));
    assert!(report.events().iter().any(|event| matches!(
        event,
        ExecutionEvent::FieldSet {
            target,
            field,
            value,
        } if target == "box" && field == "value" && value == "updated"
    )));
}

#[test]
fn methods_use_self_and_can_mutate_instance_fields() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Counter
                Field value = "0"

                Def Set(value)
                    Self.value = value
                    Return Self.value
                End Def
            End Class

            Let counter = New Counter()
            Let result = counter.Set("8")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("8"));
    assert_eq!(report.object_field("counter", "value"), Some("8"));
    assert!(report.events().iter().any(|event| matches!(
        event,
        ExecutionEvent::MethodCalled {
            class_name,
            name,
            return_value: Some(value),
            ..
        } if class_name == "Counter" && name == "Set" && value == "8"
    )));
}

#[test]
fn method_return_values_can_feed_csv_edits() {
    let (_directory, mut document) = open("値\n1\n2\n");
    run(
        r#"
            Class Formatter
                Field replacement = "9"

                Def Value()
                    Return Self.replacement
                End Def
            End Class

            Let formatter = New Formatter()
            This.Worksheet.Editor.Cell(A2 To A3).Value.Set = formatter.Value()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(document.cell_a1("A2").unwrap(), Some("9"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("9"));
    assert!(document.undo().unwrap());
    assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
    assert_eq!(document.cell_a1("A3").unwrap(), Some("2"));
}

#[test]
fn object_aliases_share_instance_identity() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            Let first = New Box()
            Let second = first
            second.value = "b"
            Let result = first.value
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("b"));
    assert_eq!(report.object_field("first", "value"), Some("b"));
    assert_eq!(report.object_field("second", "value"), Some("b"));
}

#[test]
fn unknown_method_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            Let box = New Box()
            box.Missing()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::UnknownMethod { class_name, method })
            if class_name == "Box" && method == "Missing"
    ));
}

#[test]
fn objects_cannot_be_written_directly_to_csv_cells() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            Let box = New Box()
            This.Worksheet.Editor.Cell(A2).Value.Set = box
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ExpectedText { .. })
    ));
}

#[test]
fn reports_missing_end_class_at_opening_line() {
    let error = parse(
        r#"
            Class Box
                Field value = "a"
        "#,
    )
    .unwrap_err();

    assert_eq!(error.line(), 2);
    assert!(error.message().contains("End Class"));
}
