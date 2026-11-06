use std::fs;

use super::tests_staging::{create_project, generated_path};
use super::*;
use crate::process::{
    aggregation_definition::{
        AggregateDefinition, AggregationDefinition, AggregationKind, DormantReason,
        FilterDefinition, InputScope, ResolvedRequest,
    },
    calculation::AggregateFunction,
};

fn group_by() -> AggregationDefinition {
    AggregationDefinition {
        name: "by_name".into(),
        scope: InputScope::ViewFiltered,
        kind: AggregationKind::GroupBy {
            keys: vec!["Name".into()],
            aggregates: vec![
                AggregateDefinition::CountRows,
                AggregateDefinition::Column {
                    function: AggregateFunction::Sum,
                    header: "Score".into(),
                },
                AggregateDefinition::DistinctCount {
                    header: "Name".into(),
                },
            ],
        },
    }
}

fn pivot() -> AggregationDefinition {
    AggregationDefinition {
        name: "pivot \"quoted\"".into(),
        scope: InputScope::Selection,
        kind: AggregationKind::Pivot {
            rows: vec!["Name".into()],
            columns: vec!["Gone".into()],
            values: vec![AggregateDefinition::Column {
                function: AggregateFunction::Average,
                header: "Score".into(),
            }],
            filters: vec![FilterDefinition {
                header: "Name".into(),
                allowed: vec!["Alice".into(), "B,\"ob\"".into()],
            }],
        },
    }
}

#[test]
// {
//   責務: [definitions_are_staged_until_save_and_restored_after_reopen: 定義がsave前はdiskに出ず、save後に再openで同じ内容に復元され、user.rlyが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn definitions_are_staged_until_save_and_restored_after_reopen() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let before_generated = fs::read(&generated).unwrap();
    let before_user = fs::read(&user).unwrap();
    let definitions = vec![group_by(), pivot()];

    let mut session = ProjectSession::open(&manifest).unwrap();
    assert!(session.aggregation_views("data").unwrap().is_empty());
    session
        .save_generated_aggregation_views("data", &definitions)
        .unwrap();
    // 未保存の間はdiskを変えず、session内では読める。
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    let mut expected = definitions.clone();
    expected.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(session.aggregation_views("data").unwrap(), expected);

    session.save().unwrap();
    assert_eq!(fs::read(&user).unwrap(), before_user);
    let saved = fs::read_to_string(&generated).unwrap();
    assert!(saved.contains(".Worksheet.Aggregation(\"by_name\").Definition = GroupBy("));
    assert!(saved.contains("Type = String"), "既存のgenerated設定は残る");

    let reopened = ProjectSession::open(&manifest).unwrap();
    assert_eq!(reopened.aggregation_views("data").unwrap(), expected);
}

#[test]
// {
//   責務: [missing_columns_make_only_that_definition_dormant: 列が無い定義だけが休眠し、定義は保存されたまま、他の定義は有効なままであることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn missing_columns_make_only_that_definition_dormant() {
    let (_directory, manifest) = create_project();
    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_aggregation_views("data", &[group_by(), pivot()])
        .unwrap();
    session.save().unwrap();

    let reopened = ProjectSession::open(&manifest).unwrap();
    let document = reopened.open_source_document("data").unwrap();
    let headers: Vec<String> = (0..document.column_count())
        .map(|column| document.cell(0, column).unwrap_or_default().to_string())
        .collect();
    let definitions = reopened.aggregation_views("data").unwrap();
    assert_eq!(definitions.len(), 2);
    for definition in &definitions {
        let resolved = definition.resolve(&headers);
        if definition.name == "by_name" {
            assert!(matches!(resolved, Ok(ResolvedRequest::GroupBy(_))));
        } else {
            assert_eq!(
                resolved,
                Err(DormantReason {
                    missing: vec!["Gone".into()],
                    ambiguous: vec![],
                })
            );
        }
    }
    // 列が戻れば休眠した定義も有効になる。
    let mut with_gone = headers.clone();
    with_gone.push("Gone".into());
    assert!(definitions.iter().all(|d| d.resolve(&with_gone).is_ok()));
    // 見出しが重複すると、どの列か決められないので休眠する。
    let mut duplicated = headers.clone();
    duplicated.push("Score".into());
    let reason = definitions[0].resolve(&duplicated).unwrap_err();
    assert_eq!(reason.ambiguous, vec!["Score".to_owned()]);
}

#[test]
// {
//   責務: [user_definition_wins_and_user_script_is_never_written: user.rlyの同名定義が優先され、保存してもuser.rlyが書き換わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn user_definition_wins_and_user_script_is_never_written() {
    let (_directory, manifest) = create_project();
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let generated = generated_path(&manifest);
    let user_text = "' keep user config\nSET This.Project.Source(\"data\").Worksheet.Aggregation(\"by_name\").Definition = GroupBy(Scope(\"AllRows\"), Keys([\"Score\"]), Aggregates([CountRows()]))\n";
    fs::write(&user, user_text).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_aggregation_views("data", &[group_by()])
        .unwrap();
    session.save().unwrap();

    assert_eq!(fs::read_to_string(&user).unwrap(), user_text);
    // user側が同名を持つので、generatedには同名の定義を書かない。
    assert!(
        !fs::read_to_string(&generated)
            .unwrap()
            .contains("Aggregation(")
    );
    let reopened = ProjectSession::open(&manifest).unwrap();
    let definitions = reopened.aggregation_views("data").unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].scope, InputScope::AllRows);
}

#[test]
// {
//   責務: [bad_names_and_unknown_sources_are_rejected_and_save_failure_keeps_staging: 空・重複の名前と不明なsourceを拒否し、保存失敗でもstagingが残って再試行できることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn bad_names_and_unknown_sources_are_rejected_and_save_failure_keeps_staging() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let before_generated = fs::read(&generated).unwrap();
    let mut session = ProjectSession::open(&manifest).unwrap();

    let mut empty = group_by();
    empty.name = "  ".into();
    assert!(
        session
            .save_generated_aggregation_views("data", &[empty])
            .is_err()
    );
    assert!(
        session
            .save_generated_aggregation_views("data", &[group_by(), group_by()])
            .is_err()
    );
    assert!(
        session
            .save_generated_aggregation_views("nope", &[group_by()])
            .is_err()
    );
    assert!(!session.dirty_state().structure_or_config_is_dirty());

    session
        .save_generated_aggregation_views("data", &[group_by()])
        .unwrap();
    session.project_mut().name.clear();
    assert!(session.save().is_err());
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    assert_eq!(session.aggregation_views("data").unwrap().len(), 1);

    session.project_mut().name = "Recovered".into();
    session.save().unwrap();
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("Aggregation(\"by_name\")")
    );
}

#[test]
// {
//   責務: [malformed_declarations_are_reported_instead_of_ignored: 壊れた定義宣言を黙って捨てず、行番号つきのerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn malformed_declarations_are_reported_instead_of_ignored() {
    for body in [
        "GroupBy(Scope(\"Nowhere\"), Keys([\"Name\"]), Aggregates([]))",
        "GroupBy(Scope(\"AllRows\"), Keys([\"Name\"]))",
        "GroupBy(Scope(\"AllRows\"), Keys([\"Name\"]), Aggregates([Median(\"Score\")]))",
        "Cube(Scope(\"AllRows\"))",
        "GroupBy(Scope(\"AllRows\"), Keys([\"Name\"]), Aggregates([])) trailing",
    ] {
        let (_directory, manifest) = create_project();
        let generated = generated_path(&manifest);
        fs::write(
            &generated,
            format!(
                "SET This.Project.Source(\"data\").Worksheet.Aggregation(\"x\").Definition = {body}\n"
            ),
        )
        .unwrap();
        let session = ProjectSession::open(&manifest).unwrap();
        let error = session.aggregation_views("data").unwrap_err().to_string();
        assert!(error.contains("line 1"), "{body}: {error}");
    }
}
