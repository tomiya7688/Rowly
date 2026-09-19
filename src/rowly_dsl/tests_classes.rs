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
            value:
                Expression::New {
                    class_name,
                    arguments,
                },
            ..
        } if class_name == "Formatter" && arguments.is_empty()
    ));
}

#[test]
fn parses_single_inheritance() {
    let program = parse(
        r#"
            Class Base
                Field value = "base"
            End Class

            Class Child Extends Base
                Field extra = "child"
            End Class

            Let child = New Child()
        "#,
    )
    .unwrap();

    assert_eq!(program.classes()[1].name(), "Child");
    assert_eq!(program.classes()[1].parent(), Some("Base"));
}

#[test]
fn inherited_fields_are_initialized_and_child_fields_override_parent_fields() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Base
                Field value = "base"
                Field inherited = "yes"
            End Class

            Class Child Extends Base
                Field value = "child"
            End Class

            Let child = New Child()
            Let value = child.value
            Let inherited = child.inherited
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("value"), Some("child"));
    assert_eq!(report.variable("inherited"), Some("yes"));
    assert_eq!(report.object_field("child", "value"), Some("child"));
    assert_eq!(report.object_field("child", "inherited"), Some("yes"));
}

#[test]
fn inherited_methods_are_available_and_child_methods_override_parent_methods() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Base
                Def Name()
                    Return "base"
                End Def

                Def Inherited()
                    Return "inherited"
                End Def
            End Class

            Class Child Extends Base
                Def Name()
                    Return "child"
                End Def
            End Class

            Let child = New Child()
            Let name = child.Name()
            Let inherited = child.Inherited()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("name"), Some("child"));
    assert_eq!(report.variable("inherited"), Some("inherited"));
}

#[test]
fn inherited_init_is_used_when_child_does_not_override_it() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Base
                Field value = "initial"

                Def Init(value)
                    Self.value = value
                End Def
            End Class

            Class Child Extends Base
                Field extra = "child"
            End Class

            Let child = New Child("from-parent")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("child", "value"), Some("from-parent"));
    assert_eq!(report.object_field("child", "extra"), Some("child"));
}

#[test]
fn super_calls_parent_method_and_keeps_self_bound_to_child_instance() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Base
                Field value = "base"

                Def Set(value)
                    Self.value = value
                    Return Self.value
                End Def
            End Class

            Class Child Extends Base
                Def Set(value)
                    Return Super.Set(value)
                End Def
            End Class

            Let child = New Child()
            Let result = child.Set("updated")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("updated"));
    assert_eq!(report.object_field("child", "value"), Some("updated"));
}

#[test]
fn super_resolution_starts_at_the_defining_class_parent() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Grand
                Def Name()
                    Return "grand"
                End Def
            End Class

            Class Base Extends Grand
                Def Name()
                    Return "base"
                End Def

                Def ParentName()
                    Return Super.Name()
                End Def
            End Class

            Class Child Extends Base
                Def Name()
                    Return "child"
                End Def

                Def FromChild()
                    Return Super.Name()
                End Def
            End Class

            Let child = New Child()
            Let from_child = child.FromChild()
            Let from_base = child.ParentName()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("from_child"), Some("base"));
    assert_eq!(report.variable("from_base"), Some("grand"));
}

#[test]
fn super_init_can_initialize_parent_part_of_child_instance() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Base
                Field base_value = "unset"

                Def Init(value)
                    Self.base_value = value
                End Def
            End Class

            Class Child Extends Base
                Field child_value = "unset"

                Def Init(base_value, child_value)
                    Super.Init(base_value)
                    Self.child_value = child_value
                End Def
            End Class

            Let child = New Child("base", "child")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("child", "base_value"), Some("base"));
    assert_eq!(report.object_field("child", "child_value"), Some("child"));
}

#[test]
fn super_outside_method_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Super.Missing()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::SuperOutsideMethod)
    ));
}

#[test]
fn super_on_root_class_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Root
                Def CallParent()
                    Super.Missing()
                End Def
            End Class

            Let root = New Root()
            root.CallParent()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::NoSuperClass { class_name })
            if class_name == "Root"
    ));
}

#[test]
fn unknown_parent_class_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Child Extends Missing
            End Class

            Let child = New Child()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::UnknownParentClass {
            class_name,
            parent,
        }) if class_name == "Child" && parent == "Missing"
    ));
}

#[test]
fn inheritance_cycles_are_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class First Extends Second
            End Class

            Class Second Extends First
            End Class

            Let value = New First()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::InheritanceCycle(_))
    ));
}

#[test]
fn constructor_arguments_are_parsed_and_init_runs_automatically() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Box
                Field value = "initial"

                Def Init(value)
                    Self.value = value
                End Def
            End Class

            Let box = New Box("constructed")
            Let copied = box.value
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("copied"), Some("constructed"));
    assert_eq!(report.object_field("box", "value"), Some("constructed"));
    assert!(report.events().iter().any(|event| matches!(
        event,
        ExecutionEvent::MethodCalled {
            class_name,
            name,
            arguments,
            ..
        } if class_name == "Box" && name == "Init" && arguments == &["constructed"]
    )));
}

#[test]
fn constructor_arguments_can_use_typed_expressions() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Counter
                Field value = "0"

                Def Init(value)
                    Self.value = value
                End Def
            End Class

            Let counter = New Counter(Integer("2") + Integer("3"))
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("counter", "value"), Some("5"));
}

#[test]
fn constructor_argument_count_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "initial"

                Def Init(value)
                    Self.value = value
                End Def
            End Class

            Let box = New Box()
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ConstructorArgumentCount {
            class_name,
            expected: 1,
            actual: 0,
        }) if class_name == "Box"
    ));
}

#[test]
fn class_without_init_rejects_constructor_arguments() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "initial"
            End Class

            Let box = New Box("unexpected")
        "#,
        &mut document,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ConstructorArgumentCount {
            class_name,
            expected: 0,
            actual: 1,
        }) if class_name == "Box"
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
