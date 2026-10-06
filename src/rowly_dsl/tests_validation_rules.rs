use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

// {
//   責務: [open: テスト用CSVを一時作成し、documentとTempDirを返してfile lifetimeを保つ。]
//   処理: [TempDirを作成し固定のvalidation用CSV fixtureを書き込み、CsvDocumentとして開く。]
//   引数: []
//   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
//   副作用: [一時ディレクトリにCSV fileを作成する。]
// }
fn open() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("validation.csv");
    fs::write(&path, "名前,状態\n田中,未着手\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
// {
//   責務: [allowed_values_support_multiline_lists_and_report_ordered_configuration: 複数行AllowedValuesの順序・候補照合・設定eventを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn allowed_values_support_multiline_lists_and_report_ordered_configuration() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            SET This.Worksheet.Editor.Column("状態").Validation.AllowedValues = [
                "未着手",
                "進行中",
                "完了"
            ]
        "#,
        &mut document,
    )
    .unwrap();

    let definitions = report.validation_rules();
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions[0].selector(),
        &ColumnSelector::Header("状態".to_owned())
    );
    let rule = definitions[0].rule();
    assert!(rule.matches("未着手"));
    assert!(rule.matches("進行中"));
    assert!(!rule.matches("保留"));
    assert!(matches!(
        report.events(),
        [ExecutionEvent::ValidationRuleSet { .. }]
    ));
}

#[test]
// {
//   責務: [expression_supports_multiline_boolean_logic_and_candidate_value: 複数行のValidation.ExpressionがValue候補をBoolean logicで評価することを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn expression_supports_multiline_boolean_logic_and_candidate_value() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            SET This.Worksheet.Editor.Column("状態").Validation.Expression =
                Value = "未着手" OR
                Value = "進行中" OR
                Value = "完了"
        "#,
        &mut document,
    )
    .unwrap();

    let rule = report.validation_rules()[0].rule();
    assert!(rule.matches("未着手"));
    assert!(rule.matches("進行中"));
    assert!(rule.matches("完了"));
    assert!(!rule.matches("保留"));
}

#[test]
// {
//   責務: [expression_supports_not_and_and_boolean_comparisons: Validation.ExpressionでNOT・AND・等値比較を組み合わせて候補を判定する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn expression_supports_not_and_and_boolean_comparisons() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            SET This.Worksheet.Editor.Column("状態").Validation.Expression = NOT Value = "保留" AND Value != ""
        "#,
        &mut document,
    )
    .unwrap();

    let rule = report.validation_rules()[0].rule();
    assert!(rule.matches("未着手"));
    assert!(!rule.matches("保留"));
    assert!(!rule.matches(""));
}

#[test]
// {
//   責務: [allowed_values_and_expression_have_matching_behavior: AllowedValuesとValidation.Expressionが対象候補群に同じ結果を返すことを確認する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn allowed_values_and_expression_have_matching_behavior() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            SET This.Worksheet.Editor.Column("状態").Validation.AllowedValues = ["未着手", "完了"]
            SET This.Worksheet.Editor.Column("状態").Validation.Expression = Value = "未着手" OR Value = "完了"
        "#,
        &mut document,
    )
    .unwrap();

    let definitions = report.validation_rules();
    assert_eq!(definitions.len(), 2);
    for candidate in ["", "未着手", "完了", "進行中"] {
        assert_eq!(
            definitions[0].rule().matches(candidate),
            definitions[1].rule().matches(candidate),
            "candidate {candidate:?}"
        );
    }
}

#[test]
// {
//   責務: [validation_expression_rejects_calls_variables_and_non_boolean_expressions: Validation.Expressionがfunction call・外部variable・Boolean以外の式を拒否する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn validation_expression_rejects_calls_variables_and_non_boolean_expressions() {
    for source in [
        r#"SET This.Worksheet.Editor.Column("状態").Validation.Expression = Text.Trim(Value) = "x""#,
        r#"SET This.Worksheet.Editor.Column("状態").Validation.Expression = other = Value"#,
        r#"SET This.Worksheet.Editor.Column("状態").Validation.Expression = "文字列""#,
        r#"SET This.Worksheet.Editor.Column("状態").Validation.Expression = "固定" = "固定""#,
        r#"SET This.Worksheet.Editor.Column("状態").Validation.Expression = Value = "x" OR "a" = "a""#,
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.message().contains("Validation.Expression"), "{error}");
    }
}

#[test]
// {
//   責務: [allowed_values_reject_dynamic_items_and_bad_targets: AllowedValuesのdynamic itemと不正なSET target pathをparse時に拒否する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn allowed_values_reject_dynamic_items_and_bad_targets() {
    let dynamic_value =
        parse(r#"SET This.Worksheet.Editor.Column("状態").Validation.AllowedValues = [other]"#)
            .unwrap_err();
    assert!(dynamic_value.message().contains("literal"));

    let bad_target =
        parse(r#"SET This.Worksheet.Editor.Column("状態").Validation.Other = ["x"]"#).unwrap_err();
    assert!(bad_target.message().contains("SET target"));

    let bad_path =
        parse(r#"SET This.Worksheet.Column("状態").Validation.AllowedValues = ["x"]"#).unwrap_err();
    assert!(bad_path.message().contains("SET target"));
}

#[test]
// {
//   責務: [runtime_rejects_missing_validation_target: 存在しないcolumnをvalidation targetにしたruntime設定を拒否する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn runtime_rejects_missing_validation_target() {
    let (_directory, mut document) = open();
    let result = run(
        r#"SET This.Worksheet.Editor.Column("存在しない").Validation.AllowedValues = ["x"]"#,
        &mut document,
    );
    assert!(result.unwrap_err().to_string().contains("was not found"));
}

#[test]
// {
//   責務: [repeated_declarations_remain_ordered_configuration_actions: 同一targetへの複数validation宣言を順序付きactionとして保持する。]
//   処理: [固定入力で対象のparse/runtime APIを実行し、AST・report・document状態またはerrorを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
fn repeated_declarations_remain_ordered_configuration_actions() {
    let (_directory, mut document) = open();
    let report = run(
        r#"
            SET This.Worksheet.Editor.Column(2).Validation.AllowedValues = ["未着手"]
            SET This.Worksheet.Editor.Column(2).Validation.AllowedValues = ["完了"]
        "#,
        &mut document,
    )
    .unwrap();

    let definitions = report.validation_rules();
    assert_eq!(definitions.len(), 2);
    assert!(definitions[0].rule().matches("未着手"));
    assert!(definitions[1].rule().matches("完了"));
    assert!(matches!(
        report.events(),
        [
            ExecutionEvent::ValidationRuleSet { .. },
            ExecutionEvent::ValidationRuleSet { .. }
        ]
    ));
}
