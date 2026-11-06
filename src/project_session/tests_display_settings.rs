use std::fs;

use super::tests_staging::{create_project, generated_path};
use super::*;
use crate::process::conditional_format::{
    CellInput, CompareOp, ConditionalRule, Literal, Predicate, ValueSource,
};
use crate::process::display_format::{DateStyle, DisplayFormat, NoCustomFormats};
use crate::process::display_settings::{ColumnFormat, DisplaySettings};
use crate::process::view_style::{
    BorderEdge, BorderEdgeSetting, BorderLine, BorderStyle, CellStyle, Color, HorizontalAlign,
    StyleRule, StyleTarget, TextOverflow, VerticalAlign,
};

fn color(text: &str) -> Color {
    Color::parse(text).unwrap()
}

fn rich_style() -> CellStyle {
    let mut style = CellStyle {
        horizontal: Some(HorizontalAlign::Right),
        vertical: Some(VerticalAlign::Middle),
        overflow: Some(TextOverflow::Ellipsis),
        bold: Some(true),
        italic: Some(false),
        underline: Some(true),
        text_color: Some(color("#112233")),
        background: Some(color("#FFEEDD")),
        ..CellStyle::default()
    };
    for (index, edge) in BorderEdge::ALL.into_iter().enumerate() {
        let setting = match index % 3 {
            0 => BorderEdgeSetting::Clear,
            1 => BorderEdgeSetting::Line(BorderLine {
                style: BorderStyle::Double,
                color: None,
            }),
            _ => BorderEdgeSetting::Line(BorderLine {
                style: BorderStyle::Dashed,
                color: Some(color("#abcdef")),
            }),
        };
        style.set_border(edge, setting);
    }
    style
}

fn sample_settings() -> DisplaySettings {
    // 見出し順（Gone < Name < Score < Zdate）で並べる。保存から読むと見出し順になる。
    let formats = vec![
        ColumnFormat {
            column: "Gone".into(),
            format: DisplayFormat::Boolean {
                true_label: "はい\"".into(),
                false_label: "いいえ".into(),
            },
        },
        ColumnFormat {
            column: "Name".into(),
            format: DisplayFormat::Custom("my.fmt".into()),
        },
        ColumnFormat {
            column: "Score".into(),
            format: DisplayFormat::Decimal {
                places: 2,
                thousands_separator: true,
            },
        },
        ColumnFormat {
            column: "Zdate".into(),
            format: DisplayFormat::DateTime {
                style: DateStyle::Japanese,
                show_seconds: true,
            },
        },
    ];
    let mut settings = DisplaySettings {
        formats,
        ..DisplaySettings::default()
    };
    settings
        .style
        .add_rule(StyleRule {
            target: StyleTarget::Column("Score".into()),
            style: rich_style(),
        })
        .unwrap();
    settings
        .style
        .add_rule(StyleRule {
            target: StyleTarget::Range {
                columns: vec!["Name".into(), "Gone".into()],
                first_row: 1,
                last_row: 3,
            },
            style: CellStyle::default(),
        })
        .unwrap();
    settings
        .style
        .add_rule(StyleRule {
            target: StyleTarget::Cell {
                column: "Name".into(),
                row: 2,
            },
            style: CellStyle {
                bold: Some(false),
                ..CellStyle::default()
            },
        })
        .unwrap();
    settings.style.set_row_height(0, 12.5).unwrap();
    settings.style.set_row_height(7, 30.0).unwrap();
    let high = ConditionalRule {
        target: StyleTarget::Column("Score".into()),
        source: ValueSource::Number,
        predicate: Predicate::And(vec![
            Predicate::Compare {
                op: CompareOp::Ge,
                literal: Literal::Number("10".into()),
            },
            Predicate::Not(Box::new(Predicate::Between {
                low: Literal::Number("50".into()),
                high: Literal::Number("60.5".into()),
            })),
        ]),
        style: CellStyle {
            background: Some(color("#ff0000")),
            ..CellStyle::default()
        },
        stop_if_true: true,
    };
    let texty = ConditionalRule {
        target: StyleTarget::Column("Name".into()),
        source: ValueSource::Raw,
        predicate: Predicate::Or(vec![
            Predicate::Contains("a\"b".into()),
            Predicate::StartsWith("x".into()),
            Predicate::IsEmpty,
            Predicate::IsNotEmpty,
            Predicate::Compare {
                op: CompareOp::Ne,
                literal: Literal::Text("q".into()),
            },
        ]),
        style: CellStyle::default(),
        stop_if_true: false,
    };
    let dated = ConditionalRule {
        target: StyleTarget::Column("Zdate".into()),
        source: ValueSource::Date,
        predicate: Predicate::Compare {
            op: CompareOp::Lt,
            literal: Literal::Date("2026-01-02".into()),
        },
        style: CellStyle {
            italic: Some(true),
            ..CellStyle::default()
        },
        stop_if_true: false,
    };
    for rule in [high, texty, dated] {
        settings.conditional.add_rule(rule).unwrap();
    }
    settings
}

#[test]
// {
//   責務: [display_settings_are_staged_until_save_and_restored_after_reopen: 表示設定がsave前はdiskに出ず、save後に再openで全種類の書式・style・罫線6方向・行高・条件付き書式が同じ内容に復元され、user.rlyとCSVが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn display_settings_are_staged_until_save_and_restored_after_reopen() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let before_generated = fs::read(&generated).unwrap();
    let before_user = fs::read(&user).unwrap();
    let mut csv_files: Vec<_> = fs::read_dir(manifest.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    csv_files.sort();
    let expected = sample_settings();

    let mut session = ProjectSession::open(&manifest).unwrap();
    assert_eq!(
        session.display_settings("data").unwrap(),
        DisplaySettings::default()
    );
    session
        .save_generated_display_settings("data", &expected)
        .unwrap();
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    assert_eq!(session.display_settings("data").unwrap(), expected);

    session.save().unwrap();
    assert_eq!(fs::read(&user).unwrap(), before_user);
    let saved = fs::read_to_string(&generated).unwrap();
    assert!(saved.contains("Worksheet.Display.Format(\"Score\") = Decimal("));
    assert!(saved.contains("Type = String"), "既存のgenerated設定は残る");
    let reopened = ProjectSession::open(&manifest).unwrap();
    assert_eq!(reopened.display_settings("data").unwrap(), expected);

    // 保存で別のfile（CSVのsidecar等）は増えない。
    let mut after: Vec<_> = fs::read_dir(manifest.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    after.sort();
    assert_eq!(after, csv_files);
}

#[test]
// {
//   責務: [quoted_text_that_looks_like_another_declaration_does_not_misroute: chart名や集計名の引用文字列に別の宣言の中間句が含まれていても、その行を別の種類の宣言として誤って読まず、元の種類のまま読めることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn quoted_text_that_looks_like_another_declaration_does_not_misroute() {
    let (_directory, manifest) = create_project();
    let prefix = "SET This.Project.Source(\"data\").Worksheet.";
    fs::write(
        generated_path(&manifest),
        format!(
            "{prefix}Chart(\"x).Worksheet.Display.Format(\").Definition = Line(Source(\"data\"), Series([Series(\"Score\")]))\n\
             {prefix}Aggregation(\"y).Worksheet.Chart(\").Definition = GroupBy(Scope(\"AllRows\"), Keys([\"Name\"]), Aggregates([CountRows()]))\n"
        ),
    )
    .unwrap();
    let session = ProjectSession::open(&manifest).unwrap();
    assert_eq!(session.chart_views("data").unwrap().len(), 1);
    assert_eq!(session.aggregation_views("data").unwrap().len(), 1);
    assert_eq!(
        session.display_settings("data").unwrap(),
        DisplaySettings::default()
    );
}

#[test]
// {
//   責務: [missing_columns_stay_dormant_and_other_settings_keep_working: 現在のCSVに無い見出しの設定が保存・復元で消えず、解決で休眠するだけで、他の列の書式・styleは有効なままであり、見出しが戻れば再び有効になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn missing_columns_stay_dormant_and_other_settings_keep_working() {
    let (_directory, manifest) = create_project();
    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_display_settings("data", &sample_settings())
        .unwrap();
    session.save().unwrap();

    let reopened = ProjectSession::open(&manifest).unwrap();
    let settings = reopened.display_settings("data").unwrap();
    // 現在のCSVには Name と Score だけがある。Gone・Zdateの設定は残る。
    let headers = ["Name", "Score"].map(String::from).to_vec();
    let resolved = settings.resolve(&headers);
    assert_eq!(
        resolved
            .dormant_formats
            .iter()
            .map(|entry| entry.column.as_str())
            .collect::<Vec<_>>(),
        ["Gone", "Zdate"]
    );
    assert!(!resolved.dormant_style_rules().is_empty());
    assert_eq!(resolved.dormant_conditional_rules().len(), 1);

    let custom = NoCustomFormats;
    let cell = resolved.render_cell(
        1,
        1,
        &CellInput {
            raw: "1234.5",
            derived: None,
        },
        &custom,
    );
    assert_eq!(cell.display.text(), "1,234.50");
    assert_eq!(cell.style.bold, Some(true));
    assert_eq!(cell.style.background, Some(color("#ff0000")));
    assert_eq!(cell.conditional.matched, [0]);

    // 見出しが戻れば、休眠していた設定が元のまま有効になる。
    let full = ["Gone", "Name", "Score", "Zdate"]
        .map(String::from)
        .to_vec();
    let resolved = settings.resolve(&full);
    assert!(resolved.dormant_formats.is_empty());
    assert!(resolved.dormant_style_rules().is_empty());
    assert!(resolved.dormant_conditional_rules().is_empty());
}

#[test]
// {
//   責務: [user_settings_win_per_key_and_user_script_is_never_written: user.rlyが定義した書式・行高・style・条件付き書式がgeneratedより優先され、保存してもuser.rlyが書き換わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn user_settings_win_per_key_and_user_script_is_never_written() {
    let (_directory, manifest) = create_project();
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let generated = generated_path(&manifest);
    let user_text = concat!(
        "' keep user config\n",
        "SET This.Project.Source(\"data\").Worksheet.Display.Format(\"Score\") = Integer(Thousands(\"false\"))\n",
        "SET This.Project.Source(\"data\").Worksheet.Display.RowHeight(\"0\") = Millimeters(\"20\")\n",
        "SET This.Project.Source(\"data\").Worksheet.Display.Style(\"0\") = Rule(Target(Column(\"Name\")), Style(Bold(\"true\")))\n",
    );
    fs::write(&user, user_text).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    session
        .save_generated_display_settings("data", &sample_settings())
        .unwrap();
    session.save().unwrap();

    assert_eq!(fs::read_to_string(&user).unwrap(), user_text);
    let saved = fs::read_to_string(&generated).unwrap();
    assert!(!saved.contains("Format(\"Score\")"), "{saved}");
    assert!(!saved.contains("RowHeight(\"0\")"));
    assert!(
        saved.contains("RowHeight(\"7\")"),
        "userが定義していない行高は保存する"
    );
    assert!(
        !saved.contains("Display.Style("),
        "userがstyleを持つsourceはgeneratedを使わない"
    );
    assert!(
        saved.contains("Display.Conditional("),
        "条件付き書式はuser定義が無いので保存する"
    );

    let reopened = ProjectSession::open(&manifest).unwrap();
    let settings = reopened.display_settings("data").unwrap();
    let score = settings
        .formats
        .iter()
        .find(|entry| entry.column == "Score")
        .unwrap();
    assert_eq!(
        score.format,
        DisplayFormat::Integer {
            thousands_separator: false
        }
    );
    assert_eq!(settings.style.row_heights.get(&0), Some(&20.0));
    assert_eq!(settings.style.row_heights.get(&7), Some(&30.0));
    assert_eq!(settings.style.rules.len(), 1);
    assert_eq!(settings.conditional.rules.len(), 3);
}

#[test]
// {
//   責務: [bad_settings_are_rejected_and_save_failure_keeps_staging: 重複・空の見出し、検証に通らないstyle・条件付き書式・行高、不明なsourceを拒否し、保存失敗でもstagingが残って再試行できることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn bad_settings_are_rejected_and_save_failure_keeps_staging() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let before_generated = fs::read(&generated).unwrap();
    let mut session = ProjectSession::open(&manifest).unwrap();

    let format = |column: &str| ColumnFormat {
        column: column.into(),
        format: DisplayFormat::Text,
    };
    let duplicated = DisplaySettings {
        formats: vec![format("Score"), format("Score")],
        ..DisplaySettings::default()
    };
    let empty = DisplaySettings {
        formats: vec![format(" ")],
        ..DisplaySettings::default()
    };
    let mut bad_rule = DisplaySettings::default();
    bad_rule.style.rules.push(StyleRule {
        target: StyleTarget::Column(String::new()),
        style: CellStyle::default(),
    });
    let mut bad_height = DisplaySettings::default();
    bad_height.style.row_heights.insert(0, f64::NAN);
    let mut bad_conditional = DisplaySettings::default();
    bad_conditional.conditional.rules.push(ConditionalRule {
        target: StyleTarget::Column("Score".into()),
        source: ValueSource::Number,
        predicate: Predicate::Contains("x".into()),
        style: CellStyle::default(),
        stop_if_true: false,
    });
    for rejected in [duplicated, empty, bad_rule, bad_height, bad_conditional] {
        assert!(
            session
                .save_generated_display_settings("data", &rejected)
                .is_err()
        );
    }
    assert!(
        session
            .save_generated_display_settings("nope", &sample_settings())
            .is_err()
    );
    assert!(!session.dirty_state().structure_or_config_is_dirty());

    session
        .save_generated_display_settings("data", &sample_settings())
        .unwrap();
    session.project_mut().name.clear();
    assert!(session.save().is_err());
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());
    assert_eq!(session.display_settings("data").unwrap(), sample_settings());

    session.project_mut().name = "Recovered".into();
    session.save().unwrap();
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("Display.Format(\"Score\")")
    );
}

#[test]
// {
//   責務: [duplicate_keys_in_one_file_are_reported_with_the_second_line: 同じfile内で同じキーが2回出た宣言を、どちらかを黙って捨てずに2行目のerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn duplicate_keys_in_one_file_are_reported_with_the_second_line() {
    let prefix = "SET This.Project.Source(\"data\").Worksheet.Display.";
    for (first, second) in [
        (
            "Format(\"Score\") = Text()",
            "Format(\"Score\") = Integer(Thousands(\"true\"))",
        ),
        (
            "RowHeight(\"1\") = Millimeters(\"5\")",
            "RowHeight(\"1\") = Millimeters(\"6\")",
        ),
        (
            "Style(\"0\") = Rule(Target(Column(\"A\")), Style())",
            "Style(\"0\") = Rule(Target(Column(\"B\")), Style())",
        ),
        (
            "Conditional(\"2\") = Rule(Target(Column(\"A\")), Source(\"Raw\"), When(IsEmpty()), Style())",
            "Conditional(\"2\") = Rule(Target(Column(\"B\")), Source(\"Raw\"), When(IsEmpty()), Style())",
        ),
    ] {
        let (_directory, manifest) = create_project();
        fs::write(
            generated_path(&manifest),
            format!("{prefix}{first}\n{prefix}{second}\n"),
        )
        .unwrap();
        let session = ProjectSession::open(&manifest).unwrap();
        let error = session.display_settings("data").unwrap_err().to_string();
        assert!(error.contains("line 2"), "{first}: {error}");
    }
}

#[test]
// {
//   責務: [malformed_display_declarations_are_reported_instead_of_ignored: 壊れた表示設定の宣言を黙って捨てず、行番号つきのerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn malformed_display_declarations_are_reported_instead_of_ignored() {
    let prefix = "SET This.Project.Source(\"data\").Worksheet.Display.";
    for body in [
        "Format(\"Score\") = Cube()",
        "Format(\"Score\") = Decimal(Places(\"x\"), Thousands(\"true\"))",
        "Format(\"Score\") = Decimal(Places(\"2\"))",
        "Format(\"Score\") = Date(Style(\"Nope\"))",
        "Format(\"Score\") = Text() trailing",
        "Format(\" \") = Text()",
        "Wobble(\"x\") = Text()",
        "RowHeight(\"0\") = Millimeters(\"-3\")",
        "RowHeight(\"zero\") = Millimeters(\"3\")",
        "RowHeight(\"0\") = Millimeters(\"NaN\")",
        "Style(\"0\") = Rule(Target(Column(\"A\")), Style(Bold(\"maybe\")))",
        "Style(\"0\") = Rule(Target(Column(\"A\")), Style(Bold(\"true\"), Bold(\"false\")))",
        "Style(\"0\") = Rule(Target(Column(\"A\")), Style(TextColor(\"red\")))",
        "Style(\"0\") = Rule(Target(Column(\"A\")), Style(Borders([Border(\"Up\", \"Thin\")])))",
        "Style(\"0\") = Rule(Target(Column(\"A\")), Style(Glow(\"x\")))",
        "Style(\"0\") = Rule(Target(Range([\"A\"], \"5\", \"2\")), Style())",
        "Style(\"0\") = Rule(Style())",
        "Conditional(\"0\") = Rule(Target(Column(\"A\")), Source(\"Number\"), When(Contains(\"x\")), Style())",
        "Conditional(\"0\") = Rule(Target(Column(\"A\")), Source(\"Raw\"), When(Compare(\"Gt\", Number(\"1\"))), Style())",
        "Conditional(\"0\") = Rule(Target(Column(\"A\")), Source(\"Raw\"), When(Shout()), Style())",
        "Conditional(\"0\") = Rule(Target(Column(\"A\")), Source(\"Raw\"), When(IsEmpty()), Style(), Stop(\"x\"))",
    ] {
        let (_directory, manifest) = create_project();
        let generated = generated_path(&manifest);
        fs::write(&generated, format!("{prefix}{body}\n")).unwrap();
        let session = ProjectSession::open(&manifest).unwrap();
        let error = session.display_settings("data").unwrap_err().to_string();
        assert!(error.contains("line 1"), "{body}: {error}");
    }
}
