use std::fs;

use tempfile::tempdir;

use super::*;

fn open() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("bindings.csv");
    fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn ast_distinguishes_var_const_and_assignment() {
    let program = parse("VAR count = 0\nCONST title = \"名前\"\ncount = 1").unwrap();
    assert_eq!(
        program.statements(),
        &[
            Statement::Declare {
                kind: DeclarationKind::Var,
                name: "count".into(),
                value: Expression::Literal("0".into()),
            },
            Statement::Declare {
                kind: DeclarationKind::Const,
                name: "title".into(),
                value: Expression::Literal("名前".into()),
            },
            Statement::Assign {
                name: "count".into(),
                value: Expression::Literal("1".into()),
            },
        ]
    );
}

#[test]
fn declaration_keywords_handle_case_whitespace_and_identifier_boundaries() {
    let (_directory, mut document) = open();
    let report = run(
        "vAr\tCount = Integer(\"1\")\ncOnSt\t増分 = Integer(\"2\")\nCOUNT = count + 増分\nVAR variable = \"a\"\nvariable = \"b\"",
        &mut document,
    )
    .unwrap();
    assert_eq!(report.variable("count"), Some("3"));
    assert_eq!(report.variable("増分"), Some("2"));
    assert_eq!(report.variable("variable"), Some("b"));
}

#[test]
fn var_reassignment_preserves_typed_values_and_csv_process_history() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            CONST increment = Integer("2")
            VAR count = Integer("1")
            count = count + increment
            If count = Integer("3") Then
                count = count * Integer("2")
            Else
                count = Integer("0")
            End If
            BeginTransaction()
            SetCellValueByHeader(Integer("2"), "Score", count)
            count = count / Integer("2")
            SetCellValueByHeader(Integer("3"), "Score", count)
            CommitTransaction()
            CONST text = "001"
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.variable("count"), Some("3"));
    assert_eq!(report.variable("text"), Some("001"));
    assert_eq!(document.cell_a1("B2").unwrap(), Some("6"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("3"));
    assert!(document.undo().unwrap());
    assert!(!document.is_dirty());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    assert!(!document.can_undo());
    assert!(document.redo().unwrap());
}

#[test]
fn const_and_unknown_assignment_reject_before_rhs_side_effects() {
    for (declaration, expected_constant) in [("CONST value = \"fixed\"", true), ("", false)] {
        let (_directory, mut document) = open();
        let before = fs::read(document.path()).unwrap();
        let script = format!(
            r#"
                Def Touch()
                    This.Worksheet.Editor.Cell(B2).Value.Set = "unexpected"
                    Return "changed"
                End Def
                {declaration}
                VALUE = Touch()
            "#
        );
        let error = run(&script, &mut document).unwrap_err();
        if expected_constant {
            assert!(matches!(
                error,
                DslError::Execute(ExecutionError::ConstantAssignment(name)) if name == "VALUE"
            ));
        } else {
            assert!(matches!(
                error,
                DslError::Execute(ExecutionError::UnknownVariable(name)) if name == "VALUE"
            ));
        }
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert_eq!(fs::read(document.path()).unwrap(), before);
    }
}

#[test]
fn duplicate_declarations_cannot_change_mutability_or_execute_initializers() {
    for first in ["VAR", "CONST"] {
        for second in ["VAR", "CONST"] {
            let (_directory, mut document) = open();
            let script = format!(
                r#"
                    Def Touch()
                        This.Worksheet.Editor.Cell(B2).Value.Set = "unexpected"
                        Return "changed"
                    End Def
                    {first} value = "initial"
                    {second} VALUE = Touch()
                "#
            );
            let error = run(&script, &mut document).unwrap_err();
            assert!(matches!(
                error,
                DslError::Execute(ExecutionError::DuplicateVariable(name)) if name == "VALUE"
            ));
            assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
            assert!(!document.is_dirty());
        }
    }
}

#[test]
fn local_declarations_and_parameters_shadow_without_mutating_outer_const() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            CONST value = "global"
            Def Local()
                VAR value = "local"
                value = "updated"
                CONST private = value
                Return private
            End Def
            Def Parameter(value)
                value = "argument-updated"
                Return value
            End Def
            VAR first = Local()
            VAR second = Local()
            VAR third = Parameter("argument")
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.variable("value"), Some("global"));
    assert_eq!(report.variable("first"), Some("updated"));
    assert_eq!(report.variable("second"), Some("updated"));
    assert_eq!(report.variable("third"), Some("argument-updated"));
    assert_eq!(report.variable("private"), None);
    assert!(!document.is_dirty());
}

#[test]
fn assignment_resolves_the_nearest_visible_binding() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            VAR count = Integer("0")
            Def Increment()
                count = count + Integer("1")
            End Def
            Increment()
            For i = Integer("1") To Integer("2")
                CONST addition = i
                count = count + addition
            Next i
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.variable("count"), Some("4"));
    assert_eq!(report.variable("i"), None);
    assert_eq!(report.variable("addition"), None);
}

#[test]
fn outer_const_cannot_be_assigned_from_functions_methods_or_loops() {
    for body in [
        "Def Change()\nvalue = \"changed\"\nEnd Def\nChange()",
        "Class Tool\nDef Change()\nvalue = \"changed\"\nEnd Def\nEnd Class\nVAR tool = New Tool()\ntool.Change()",
        "For i = Integer(\"1\") To Integer(\"2\")\nvalue = \"changed\"\nNext i",
        "If \"x\" = \"x\" Then\nvalue = \"changed\"\nEnd If",
    ] {
        let (_directory, mut document) = open();
        let script = format!("CONST value = \"fixed\"\n{body}");
        let error = run(&script, &mut document).unwrap_err();
        assert!(matches!(
            error,
            DslError::Execute(ExecutionError::ConstantAssignment(name)) if name == "value"
        ));
    }
}

#[test]
fn nested_loop_declarations_are_fresh_each_iteration_and_return_unwinds_scopes() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            CONST i = "outer"
            Def Total()
                VAR sum = Integer("0")
                For i = Integer("1") To Integer("2")
                    CONST part = i
                    For j = Integer("1") To Integer("2")
                        VAR local = part
                        local = local + j
                        sum = sum + local
                    Next j
                Next i
                Return sum
            End Def
            Def Early()
                For i = Integer("1") To Integer("3")
                    CONST local = i
                    Return local
                Next i
                Return "unreachable"
            End Def
            VAR total = Total()
            VAR early = Early()
            VAR again = Early()
            For unused = Integer("2") To Integer("1")
                CONST never = "not-created"
            Next unused
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.variable("total"), Some("12"));
    assert_eq!(report.variable("early"), Some("1"));
    assert_eq!(report.variable("again"), Some("1"));
    assert_eq!(report.variable("i"), Some("outer"));
    for name in ["j", "part", "local", "sum", "unused", "never"] {
        assert_eq!(report.variable(name), None, "{name}");
    }
}

#[test]
fn constructors_and_inherited_methods_keep_local_binding_rules() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            CONST value = "global"
            Class Base
                Field value = "unset"
                Def Init(value)
                    CONST initial = value
                    Self.value = initial
                End Def
                Def Update(value)
                    VAR local = value
                    local = "updated"
                    Self.value = local
                    Return Self.value
                End Def
            End Class
            Class Child Extends Base
                Def Init(value)
                    CONST argument = value
                    Super.Init(argument)
                End Def
                Def Update(value)
                    CONST argument = value
                    Return Super.Update(argument)
                End Def
            End Class
            CONST child = New Child("initial")
            VAR result = child.Update("argument")
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.object_field("child", "value"), Some("updated"));
    assert_eq!(report.variable("result"), Some("updated"));
    assert_eq!(report.variable("value"), Some("global"));
    for name in ["initial", "argument", "local", "Self"] {
        assert_eq!(report.variable(name), None);
    }
}

#[test]
fn const_protects_the_binding_not_the_aliased_object_fields() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            Class Box
                Field value = "initial"
            End Class
            CONST fixed = New Box()
            VAR alias = fixed
            alias.value = "through-alias"
            fixed.value = "through-constant"
            VAR other = New Box()
            alias = other
            alias.value = "other"
        "#,
        &mut document,
    )
    .unwrap();
    assert_eq!(report.object_field("fixed", "value"), Some("through-constant"));
    assert_eq!(report.object_field("alias", "value"), Some("other"));
    assert_eq!(report.object_field("other", "value"), Some("other"));

    let error = run(
        "Class Box\nEnd Class\nCONST fixed = New Box()\nfixed = New Box()",
        &mut document,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::ConstantAssignment(name)) if name == "fixed"
    ));
}

#[test]
fn legacy_declarations_are_parse_errors_even_in_unexecuted_blocks() {
    for declaration in ["Let value = 1", "LET\tvalue = 1", "Dim value = 1", "let", "DIM"] {
        for (source, line) in [
            (format!("' comment\n{declaration}"), 2),
            (format!("Def Unused()\n{declaration}\nEnd Def"), 2),
            (format!("If \"a\" = \"b\" Then\n{declaration}\nEnd If"), 2),
            (format!("Class Box\nDef Unused()\n{declaration}\nEnd Def\nEnd Class"), 3),
        ] {
            let error = parse(&source).unwrap_err();
            assert_eq!(error.line(), line);
            assert!(error.message().contains("VAR or CONST"));
        }
    }
    let (_directory, mut document) = open();
    let report = run("Rem Let is only a comment\nCONST text = \"Let Dim\"", &mut document).unwrap();
    assert_eq!(report.variable("text"), Some("Let Dim"));
}

#[test]
fn malformed_declarations_and_reserved_bindings_are_rejected() {
    for source in [
        "VAR", "CONST", "VAR name", "CONST name", "VAR name =", "CONST name =",
        "VAR 1name = 1", "CONST = 1", "VAR name == 1", "name == 1",
        "VAR Self = 1", "CONST super = 1", "Self = 1", "Super = 1",
        "VAR true = 1", "CONST FALSE = 1", "VAR CONST = 1",
        "Def Bad(Self)\nEnd Def",
        "Class Bad\nDef Init(Super)\nEnd Def\nEnd Class",
        "For Self = Integer(\"1\") To Integer(\"2\")\nNext Self",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn declarations_require_initialized_visible_values() {
    for source in ["VAR value = value", "CONST value = missing"] {
        let (_directory, mut document) = open();
        assert!(matches!(
            run(source, &mut document),
            Err(DslError::Execute(ExecutionError::UnknownVariable(_)))
        ));
        assert!(!document.is_dirty());
    }
    let (_directory, mut document) = open();
    let error = run(
        "Def Echo(value)\nCONST value = \"again\"\nEnd Def\nEcho(\"argument\")",
        &mut document,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DslError::Execute(ExecutionError::DuplicateVariable(_))
    ));
}
