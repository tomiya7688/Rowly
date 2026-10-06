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
    let path = directory.path().join("predicates.csv");
    fs::write(&path, source).unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [parser_uses_dedicated_ast_for_standard_namespace_calls: 標準namespace function呼び出しが専用AST nodeとしてparseされることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [namespaced_string_predicates_can_be_used_directly_as_conditions: namespaced string predicateのBoolean結果をIf条件に直接使えることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [text_is_japanese_matches_mixed_text_containing_japanese: Text.IsJapaneseが日本語を含む文字列をtrueと判定することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [namespaced_type_predicates_accept_text_and_typed_values: namespaced type predicateをText値とtyped valueへ適用できることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [boolean_return_values_from_standard_namespaces_can_drive_if_conditions: standard namespaceのBoolean戻り値でIf分岐を選べることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [standard_namespaces_and_functions_are_case_insensitive: standard namespaceとfunction名の大文字小文字を区別せずに解決することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [non_boolean_expression_condition_is_rejected: Boolean以外のexpressionを条件として使えないことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [namespaced_string_predicates_require_text_arguments: namespaced string predicateがString以外の引数を拒否することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [namespaced_predicate_argument_count_is_validated: namespaced predicateで必須引数が不足した場合のerrorを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [old_global_predicate_builtins_are_not_available: 廃止されたglobal predicate builtin名を関数として呼べないことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [unknown_standard_namespace_function_is_explicit: 未定義のstandard namespace functionを明示的に診断することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
    assert!(
        error
            .to_string()
            .contains("unknown Rowly DSL standard function")
    );
}

#[test]
// {
//   責務: [standard_namespace_names_are_reserved_for_bindings: standard namespace名をbindingとして宣言できないことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn standard_namespace_names_are_reserved_for_bindings() {
    for name in ["Text", "Number", "Boolean"] {
        let error = parse(&format!("VAR {name} = \"value\"")).unwrap_err();
        assert!(
            error.to_string().contains("reserved binding name"),
            "{name}"
        );

        let error = parse(&format!("Def F({name})\nReturn \"x\"\nEnd Def")).unwrap_err();
        assert!(
            error.to_string().contains("reserved binding name"),
            "{name}"
        );

        let error = parse(&format!(
            "For {name} = Integer(\"1\") To Integer(\"1\")\nNext {name}"
        ))
        .unwrap_err();
        assert!(
            error.to_string().contains("reserved binding name"),
            "{name}"
        );
    }
}

#[test]
// {
//   責務: [legacy_predicate_names_can_still_be_user_defined_functions: 旧predicate名をuser-defined functionとして宣言・呼出しできることを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
// {
//   責務: [user_classes_with_namespace_names_do_not_replace_standard_namespaces: 同名user classがstandard namespace解決を置き換えないことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
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
