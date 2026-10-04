use std::fs;

use tempfile::tempdir;

use super::*;
use crate::process::CsvDocument;

fn open() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("validation.csv");
    fs::write(&path, "名前,状態\n田中,未着手\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
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
fn runtime_rejects_missing_validation_target() {
    let (_directory, mut document) = open();
    let result = run(
        r#"SET This.Worksheet.Editor.Column("存在しない").Validation.AllowedValues = ["x"]"#,
        &mut document,
    );
    assert!(result.unwrap_err().to_string().contains("was not found"));
}

#[test]
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
