use std::fs;

use super::tests_staging::{create_project, generated_path};
use super::*;
use crate::process::chart::{
    ChartDefinition, ChartError, ChartKind, SeriesDefinition, SourceTable, resolve,
};

fn column_chart() -> ChartDefinition {
    ChartDefinition {
        name: "scores".into(),
        kind: ChartKind::Column,
        source_id: "data".into(),
        category_header: Some("Name".into()),
        series: vec![
            SeriesDefinition {
                name: Some("Total \"score\"".into()),
                header: "Score".into(),
            },
            SeriesDefinition {
                name: None,
                header: "Gone".into(),
            },
        ],
    }
}

fn pie_without_category() -> ChartDefinition {
    ChartDefinition {
        name: "share".into(),
        kind: ChartKind::Pie,
        source_id: "view:East only".into(),
        category_header: None,
        series: vec![SeriesDefinition {
            name: None,
            header: "Score".into(),
        }],
    }
}

#[test]
// {
//   責務: [charts_are_staged_until_save_and_restored_after_reopen: Chart定義がsave前はdiskに出ず、save後に再openで同じ内容(全種類のkind・任意のcategory・series名)に復元され、user.rlyが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn charts_are_staged_until_save_and_restored_after_reopen() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let before_generated = fs::read(&generated).unwrap();
    let before_user = fs::read(&user).unwrap();
    let mut definitions = vec![column_chart(), pie_without_category()];
    for (index, kind) in [
        ChartKind::Line,
        ChartKind::Bar,
        ChartKind::Area,
        ChartKind::Scatter,
    ]
    .into_iter()
    .enumerate()
    {
        let mut chart = column_chart();
        chart.name = format!("kind{index}");
        chart.kind = kind;
        definitions.push(chart);
    }

    let mut session = ProjectSession::open(&manifest).unwrap();
    assert!(session.chart_views("data").unwrap().is_empty());
    session
        .save_generated_chart_views("data", &definitions)
        .unwrap();
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    let mut expected = definitions.clone();
    expected.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(session.chart_views("data").unwrap(), expected);

    session.save().unwrap();
    assert_eq!(fs::read(&user).unwrap(), before_user);
    let saved = fs::read_to_string(&generated).unwrap();
    assert!(saved.contains(".Worksheet.Chart(\"scores\").Definition = Column("));
    assert!(saved.contains("Type = String"), "既存のgenerated設定は残る");

    let reopened = ProjectSession::open(&manifest).unwrap();
    assert_eq!(reopened.chart_views("data").unwrap(), expected);
}

#[test]
// {
//   責務: [missing_source_and_columns_keep_the_definition_and_only_fail_to_resolve: 定義のdata sourceや列が現在のCSVに無くても保存・復元で定義が消えず、解決だけがUnresolvedになり、sourceが戻れば再び解決できることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn missing_source_and_columns_keep_the_definition_and_only_fail_to_resolve() {
    let (_directory, manifest) = create_project();
    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_chart_views("data", &[column_chart(), pie_without_category()])
        .unwrap();
    session.save().unwrap();

    let reopened = ProjectSession::open(&manifest).unwrap();
    let definitions = reopened.chart_views("data").unwrap();
    assert_eq!(definitions.len(), 2);
    // どのsourceも現在は使えない: 定義は残り、解決だけが失敗する。
    for definition in &definitions {
        assert!(matches!(
            resolve(definition, &[]),
            Err(ChartError::Unresolved(unresolved)) if unresolved.missing_source
        ));
    }
    // sourceは戻るが、列"Gone"は無い: その定義だけが列の欠落で失敗する。
    let table = SourceTable {
        id: "data".into(),
        headers: ["Name", "Score"].map(String::from).to_vec(),
        rows: Vec::new(),
    };
    let scores = definitions.iter().find(|d| d.name == "scores").unwrap();
    assert!(matches!(
        resolve(scores, std::slice::from_ref(&table)),
        Err(ChartError::Unresolved(unresolved)) if unresolved.missing_columns == ["Gone"]
    ));
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
    let user_text = "' keep user config\nSET This.Project.Source(\"data\").Worksheet.Chart(\"scores\").Definition = Pie(Source(\"data\"), Series([Series(\"Score\")]))\n";
    fs::write(&user, user_text).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_chart_views("data", &[column_chart()])
        .unwrap();
    session.save().unwrap();

    assert_eq!(fs::read_to_string(&user).unwrap(), user_text);
    assert!(
        !fs::read_to_string(&generated)
            .unwrap()
            .contains("Worksheet.Chart(")
    );
    let reopened = ProjectSession::open(&manifest).unwrap();
    let definitions = reopened.chart_views("data").unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].kind, ChartKind::Pie);
}

#[test]
// {
//   責務: [bad_definitions_are_rejected_and_save_failure_keeps_staging: 空・重複の名前、空のdata source、不明なsourceを拒否し、保存失敗でもstagingが残って再試行できることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn bad_definitions_are_rejected_and_save_failure_keeps_staging() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let before_generated = fs::read(&generated).unwrap();
    let mut session = ProjectSession::open(&manifest).unwrap();

    let mut empty_name = column_chart();
    empty_name.name = " ".into();
    let mut no_source = column_chart();
    no_source.source_id = String::new();
    for rejected in [vec![empty_name], vec![no_source]] {
        assert!(
            session
                .save_generated_chart_views("data", &rejected)
                .is_err()
        );
    }
    assert!(
        session
            .save_generated_chart_views("data", &[column_chart(), column_chart()])
            .is_err()
    );
    assert!(
        session
            .save_generated_chart_views("nope", &[column_chart()])
            .is_err()
    );
    assert!(!session.dirty_state().structure_or_config_is_dirty());

    session
        .save_generated_chart_views("data", &[column_chart()])
        .unwrap();
    session.project_mut().name.clear();
    assert!(session.save().is_err());
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    assert_eq!(session.chart_views("data").unwrap().len(), 1);

    session.project_mut().name = "Recovered".into();
    session.save().unwrap();
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("Chart(\"scores\")")
    );
}

#[test]
// {
//   責務: [malformed_chart_declarations_are_reported_instead_of_ignored: 壊れたChart宣言を黙って捨てず、行番号つきのerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn malformed_chart_declarations_are_reported_instead_of_ignored() {
    for body in [
        "Cube(Source(\"data\"), Series([Series(\"Score\")]))",
        "Line(Series([Series(\"Score\")]))",
        "Line(Source(\"data\"))",
        "Line(Source(\"data\"), Series([Median(\"Score\")]))",
        "Line(Source(\"data\"), Series([Series(\"a\", \"b\", \"c\")]))",
        "Line(Source(\"data\"), Series([Series(\"Score\")])) trailing",
    ] {
        let (_directory, manifest) = create_project();
        let generated = generated_path(&manifest);
        fs::write(
            &generated,
            format!(
                "SET This.Project.Source(\"data\").Worksheet.Chart(\"x\").Definition = {body}\n"
            ),
        )
        .unwrap();
        let session = ProjectSession::open(&manifest).unwrap();
        let error = session.chart_views("data").unwrap_err().to_string();
        assert!(error.contains("line 1"), "{body}: {error}");
    }
}
