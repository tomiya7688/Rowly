use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

// {
//   責務: [open: テスト用CSVを一時作成し、documentとTempDirを返してfile lifetimeを保つ。]
//   処理: [TempDirを作成しsourceをCSV fileへ書き込み、CsvDocumentとして開く。]
//   引数: [source: テストで開くCSVの初期内容。]
//   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
//   副作用: [一時ディレクトリにCSV fileを作成する。]
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
//   責務: [parses_class_fields_methods_and_new_expression: classのfield・method・New式が期待するASTになることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn parses_class_fields_methods_and_new_expression() {
    let program = parse(
        r#"
            Class Formatter
                Field prefix = "@"

                Def Format(value)
                    Return Self.prefix
                End Def
            End Class

            VAR formatter = New Formatter()
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
        Statement::Declare {
            kind: DeclarationKind::Var,
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
// {
//   責務: [parses_single_inheritance: 単一継承classのparent名がASTへ記録されることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn parses_single_inheritance() {
    let program = parse(
        r#"
            Class Base
                Field value = "base"
            End Class

            Class Child Extends Base
                Field extra = "child"
            End Class

            VAR child = New Child()
        "#,
    )
    .unwrap();

    assert_eq!(program.classes()[1].name(), "Child");
    assert_eq!(program.classes()[1].parent(), Some("Base"));
}

#[test]
// {
//   責務: [inherited_fields_are_initialized_and_child_fields_override_parent_fields: 親fieldの初期化・継承と子classによる同名fieldのoverrideを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child()
            VAR value = child.value
            VAR inherited = child.inherited
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
// {
//   責務: [inherited_methods_are_available_and_child_methods_override_parent_methods: 親methodの継承と子classによるmethod overrideを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child()
            VAR name = child.Name()
            VAR inherited = child.Inherited()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("name"), Some("child"));
    assert_eq!(report.variable("inherited"), Some("inherited"));
}

#[test]
// {
//   責務: [inherited_init_is_used_when_child_does_not_override_it: 子classがInitを定義しない場合に親Initでfieldを初期化することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child("from-parent")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("child", "value"), Some("from-parent"));
    assert_eq!(report.object_field("child", "extra"), Some("child"));
}

#[test]
// {
//   責務: [super_calls_parent_method_and_keeps_self_bound_to_child_instance: Superで親methodを呼び、Selfが子instanceを参照し続けることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child()
            VAR result = child.Set("updated")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("updated"));
    assert_eq!(report.object_field("child", "value"), Some("updated"));
}

#[test]
// {
//   責務: [super_resolution_starts_at_the_defining_class_parent: Super解決が呼出元method定義classのparentから始まることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child()
            VAR from_child = child.FromChild()
            VAR from_base = child.ParentName()
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("from_child"), Some("base"));
    assert_eq!(report.variable("from_base"), Some("grand"));
}

#[test]
// {
//   責務: [super_init_can_initialize_parent_part_of_child_instance: Super.Initが子instanceの親fieldを初期化し、子fieldも保つことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR child = New Child("base", "child")
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("child", "base_value"), Some("base"));
    assert_eq!(report.object_field("child", "child_value"), Some("child"));
}

#[test]
// {
//   責務: [super_outside_method_is_reported: method外のSuper呼び出しをSuperOutsideMethodとして拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [super_on_root_class_is_reported: parentを持たないroot classでSuperを使うとNoSuperClassになることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn super_on_root_class_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Root
                Def CallParent()
                    Super.Missing()
                End Def
            End Class

            VAR root = New Root()
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
// {
//   責務: [unknown_parent_class_is_reported: 未定義のparent classをUnknownParentClassとして拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn unknown_parent_class_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Child Extends Missing
            End Class

            VAR child = New Child()
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
// {
//   責務: [inheritance_cycles_are_reported: class継承cycleをInheritanceCycleとして拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn inheritance_cycles_are_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class First Extends Second
            End Class

            Class Second Extends First
            End Class

            VAR value = New First()
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
// {
//   責務: [constructor_arguments_are_parsed_and_init_runs_automatically: New式のconstructor引数がparseされInitが自動実行されることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR box = New Box("constructed")
            VAR copied = box.value
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
// {
//   責務: [constructor_arguments_can_use_typed_expressions: typed arithmetic expressionをconstructor引数として評価することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR counter = New Counter(Integer("2") + Integer("3"))
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.object_field("counter", "value"), Some("5"));
}

#[test]
// {
//   責務: [constructor_argument_count_is_reported: Init parameterより少ないconstructor引数をConstructorArgumentCountとして拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR box = New Box()
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
// {
//   責務: [class_without_init_rejects_constructor_arguments: Initを持たないclassへのconstructor引数を拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn class_without_init_rejects_constructor_arguments() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "initial"
            End Class

            VAR box = New Box("unexpected")
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
// {
//   責務: [instance_fields_can_be_read_and_written: instance fieldの読み書きとFieldSet eventを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn instance_fields_can_be_read_and_written() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Box
                Field value = "initial"
            End Class

            VAR box = New Box()
            box.value = "updated"
            VAR copied = box.value
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
// {
//   責務: [methods_use_self_and_can_mutate_instance_fields: method内のSelfでinstance fieldを更新し、その戻り値とeventを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR counter = New Counter()
            VAR result = counter.Set("8")
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
// {
//   責務: [method_return_values_can_feed_csv_edits: methodの戻り値を複数cell編集に渡し、編集をundoできることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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

            VAR formatter = New Formatter()
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
// {
//   責務: [object_aliases_share_instance_identity: object aliasからのfield更新が同一instanceへ反映されることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn object_aliases_share_instance_identity() {
    let (_directory, mut document) = open("値\n1\n");
    let report = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            VAR first = New Box()
            VAR second = first
            second.value = "b"
            VAR result = first.value
        "#,
        &mut document,
    )
    .unwrap();

    assert_eq!(report.variable("result"), Some("b"));
    assert_eq!(report.object_field("first", "value"), Some("b"));
    assert_eq!(report.object_field("second", "value"), Some("b"));
}

#[test]
// {
//   責務: [unknown_method_is_reported: 未定義method呼び出しをUnknownMethodとして拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn unknown_method_is_reported() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            VAR box = New Box()
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
// {
//   責務: [objects_cannot_be_written_directly_to_csv_cells: object値をCSV cellへ直接書き込めないことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn objects_cannot_be_written_directly_to_csv_cells() {
    let (_directory, mut document) = open("値\n1\n");
    let error = run(
        r#"
            Class Box
                Field value = "a"
            End Class

            VAR box = New Box()
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
// {
//   責務: [reports_missing_end_class_at_opening_line: End Class欠落をClass開始行の位置情報付きで報告することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
