use std::{
    fs,
    path::{Path, PathBuf},
};

use rowly::{
    process::{
        CsvDocument,
        calculation::{
            CalculationBindingId, CalculationEngine, CalculationStatus, CalculationTarget,
        },
    },
    project::{ProjectScripts, ProjectSource, RowlyProject, SourceKind},
    project_session::ProjectSession,
    rowly_dsl,
};
use tempfile::{TempDir, tempdir};

/// project fixtureと bare CSV fixtureが共有する、先頭行がheaderのCSV本文。
const SOURCE_CSV: &str = "A,B,C\n1,2,\n3,4,\n";
/// A2とB2、A3とB3の積をC列へ割り当てるRowly DSL宣言。
const PRODUCT_RANGE_DSL: &str = "SET This.Worksheet.Editor.Range(\"C2:C3\").Calculation.Expression =\n    CellValue(\"A2\") * CellValue(\"B2\")\n";
/// user.rlyがC2へ割り当てる和の宣言。GUI由来の積より優先されることを検証する。
const USER_SUM_DECLARATION: &str = "SET This.Project.Source(\"data\").Worksheet.Editor.Cell(\"C2\").Calculation.Expression = CellValue(\"A2\") + CellValue(\"B2\")\n";

// {
//   責務: [create_project: Calculation Bindingの保存検証に使うprojectを作る。],
//   処理: [manifest、source CSV、init / generated / user scriptを一時directoryへ配置する。],
//   引数: [user_script: user.rlyへ書く本文。],
//   戻り値: [(TempDir, PathBuf): 一時directoryとmanifest path。fixture作成に失敗した場合はtestを失敗させる。]
// }
fn create_project(user_script: &str) -> (TempDir, PathBuf) {
    let directory = tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("scripts/macros")).unwrap();
    fs::create_dir_all(root.join("history")).unwrap();
    fs::write(root.join("data.csv"), SOURCE_CSV).unwrap();
    fs::write(
        root.join("scripts/init.rly"),
        "INCLUDE GENERATED\nINCLUDE USER\n",
    )
    .unwrap();
    fs::write(root.join("scripts/generated.rly"), "' generated\n").unwrap();
    fs::write(root.join("scripts/user.rly"), user_script).unwrap();

    let project = RowlyProject {
        name: "Calculation Persistence Test".into(),
        sources: vec![ProjectSource {
            id: "data".into(),
            kind: SourceKind::File,
            path: PathBuf::from("data.csv"),
            recursive: false,
            search_root: None,
            schema: Some(vec!["A".into(), "B".into(), "C".into()]),
        }],
        scripts: ProjectScripts {
            init: PathBuf::from("scripts/init.rly"),
            generated: PathBuf::from("scripts/generated.rly"),
            user: PathBuf::from("scripts/user.rly"),
            macros: PathBuf::from("scripts/macros"),
        },
        history: PathBuf::from("history"),
    };
    let manifest = root.join("project.rwprj");
    project.save(&manifest).unwrap();
    (directory, manifest)
}

// {
//   責務: [script_text: manifestと同じprojectにあるscriptの本文を読む。],
//   引数: [manifest: projectのmanifest path。 name: scripts配下のfile名。],
//   戻り値: [String: fileの本文。読めない場合はtestを失敗させる。]
// }
fn script_text(manifest: &Path, name: &str) -> String {
    fs::read_to_string(manifest.parent().unwrap().join("scripts").join(name)).unwrap()
}

// {
//   責務: [evaluated_value: 指定cellのbindingがEvaluatedの場合にderived resultを返す。],
//   引数: [engine: 復元済みのCalculationEngine。 cell: A1表記のtarget cell。],
//   戻り値: [Option<String>: 評価済みの値。bindingが無い、または未評価ならNone。]
// }
fn evaluated_value(engine: &CalculationEngine, cell: &str) -> Option<String> {
    let id = CalculationBindingId::new(format!("dsl:{cell}")).unwrap();
    let result = engine.result(&id)?;
    (result.status == CalculationStatus::Evaluated)
        .then(|| result.value.clone())
        .flatten()
}

#[test]
// {
//   責務: [saved_bindings_are_restored_after_reopen_without_touching_csv: 保存したBindingがproject再open後に復元されCSVへ式が書かれないことを検証する。],
//   処理: [DSLでrange Bindingを作り、generatedへ保存して再openし、derived resultとCSV本文を確認する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn saved_bindings_are_restored_after_reopen_without_touching_csv() {
    let (_directory, manifest) = create_project("' user\n");
    let csv_path = manifest.parent().unwrap().join("data.csv");

    let mut session = ProjectSession::open(&manifest).unwrap();
    let (mut document, mut engine) = session.open_source_with_calculations("data").unwrap();
    rowly_dsl::run_with_calculation_engine(PRODUCT_RANGE_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    session
        .save_generated_calculation_bindings("data", &document, &engine)
        .unwrap();
    session.save().unwrap();
    drop(session);

    let generated = script_text(&manifest, "generated.rly");
    assert!(
        generated.contains("Cell(\"C2\")"),
        "generated.rly: {generated}"
    );
    assert!(
        generated.contains("Cell(\"C3\")"),
        "generated.rly: {generated}"
    );

    let reopened = ProjectSession::open(&manifest).unwrap();
    let (restored_document, restored_engine) =
        reopened.open_source_with_calculations("data").unwrap();
    assert_eq!(
        evaluated_value(&restored_engine, "C2").as_deref(),
        Some("2")
    );
    assert_eq!(
        evaluated_value(&restored_engine, "C3").as_deref(),
        Some("12")
    );

    // 式とderived resultはCSV本文にもメモリ上のraw cellにも書かれない。
    assert_eq!(fs::read_to_string(csv_path).unwrap(), SOURCE_CSV);
    assert_eq!(restored_document.cell(1, 2), Some(""));
    assert_eq!(restored_document.cell(2, 2), Some(""));
}

#[test]
// {
//   責務: [saving_generated_bindings_keeps_user_script_and_user_declaration_wins: GUI保存がuser.rlyを書き換えず、user宣言が重なるcellを優先することを検証する。],
//   処理: [user.rlyがC2を定義するprojectでGUI相当のBindingを保存し、user.rlyのbyte列とgeneratedの内容、再open後の結果を確認する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn saving_generated_bindings_keeps_user_script_and_user_declaration_wins() {
    let user_script = format!("' hand written\n{USER_SUM_DECLARATION}");
    let (_directory, manifest) = create_project(&user_script);
    let user_path = manifest.parent().unwrap().join("scripts/user.rly");
    let user_bytes_before = fs::read(&user_path).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    let (mut document, mut engine) = session.open_source_with_calculations("data").unwrap();
    // user.rly由来のC2 Bindingが復元され、和として評価される。
    assert_eq!(evaluated_value(&engine, "C2").as_deref(), Some("3"));

    // C3だけをGUI相当の経路で追加し、保存する。
    let c3_dsl = "SET This.Worksheet.Editor.Cell(\"C3\").Calculation.Expression =\n    CellValue(\"A3\") * CellValue(\"B3\")\n";
    rowly_dsl::run_with_calculation_engine(c3_dsl, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    session
        .save_generated_calculation_bindings("data", &document, &engine)
        .unwrap();
    session.save().unwrap();

    let c2: CalculationTarget = CalculationTarget::Cell("C2".parse().unwrap());
    let c3: CalculationTarget = CalculationTarget::Cell("C3".parse().unwrap());
    assert!(
        session
            .user_defines_calculation_binding("data", c2)
            .unwrap()
    );
    assert!(
        !session
            .user_defines_calculation_binding("data", c3)
            .unwrap()
    );
    drop(session);

    assert_eq!(fs::read(&user_path).unwrap(), user_bytes_before);
    let generated = script_text(&manifest, "generated.rly");
    assert!(
        generated.contains("Cell(\"C3\")"),
        "generated.rly: {generated}"
    );
    assert!(
        !generated.contains("Cell(\"C2\")"),
        "user定義のC2をgeneratedへ複製しない: {generated}"
    );

    let reopened = ProjectSession::open(&manifest).unwrap();
    let (_, restored_engine) = reopened.open_source_with_calculations("data").unwrap();
    assert_eq!(
        evaluated_value(&restored_engine, "C2").as_deref(),
        Some("3")
    );
    assert_eq!(
        evaluated_value(&restored_engine, "C3").as_deref(),
        Some("12")
    );
}

#[test]
// {
//   責務: [unbound_binding_is_removed_from_generated_script_and_unknown_source_is_rejected: Binding解除が保存に反映されることと、未知のsource idを拒否することを検証する。],
//   処理: [保存後に一方のBindingを解除して保存し直し、generatedと再open後の結果を確認する。未知のsource idでは保存を試みてerrorを確認する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn unbound_binding_is_removed_from_generated_script_and_unknown_source_is_rejected() {
    let (_directory, manifest) = create_project("' user\n");

    let mut session = ProjectSession::open(&manifest).unwrap();
    let (mut document, mut engine) = session.open_source_with_calculations("data").unwrap();
    rowly_dsl::run_with_calculation_engine(PRODUCT_RANGE_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    session
        .save_generated_calculation_bindings("data", &document, &engine)
        .unwrap();
    session.save().unwrap();

    // C3だけを解除して保存し直す。
    let c3_id = CalculationBindingId::new("dsl:C3").unwrap();
    assert!(engine.remove_binding(&c3_id).unwrap());
    session
        .save_generated_calculation_bindings("data", &document, &engine)
        .unwrap();
    session.save().unwrap();

    // 存在しないsource idはerrorになり、次のsaveでもgeneratedを変更しない。
    let generated_before_rejection = script_text(&manifest, "generated.rly");
    assert!(
        session
            .save_generated_calculation_bindings("missing", &document, &engine)
            .is_err()
    );
    session.save().unwrap();
    drop(session);

    let generated = script_text(&manifest, "generated.rly");
    assert_eq!(generated, generated_before_rejection);
    assert!(
        generated.contains("Cell(\"C2\")"),
        "generated.rly: {generated}"
    );
    assert!(
        !generated.contains("Cell(\"C3\")"),
        "generated.rly: {generated}"
    );

    let reopened = ProjectSession::open(&manifest).unwrap();
    let (_, restored_engine) = reopened.open_source_with_calculations("data").unwrap();
    assert_eq!(
        evaluated_value(&restored_engine, "C2").as_deref(),
        Some("2")
    );
    // 解除したC3はBinding自体が復元されない（未評価のBindingとは区別する）。
    assert!(restored_engine.binding(&c3_id).is_none());
    assert!(restored_engine.result(&c3_id).is_none());
}

#[test]
// {
//   責務: [bare_csv_calculation_binding_does_not_create_sidecar_files: projectを使わないCSVでBindingを使ってもsidecarが作られずCSVへ式が書かれないことを検証する。],
//   処理: [CSV 1ファイルだけのdirectoryでBindingを作り、再計算とCSV保存を行ってdirectory内のfileを列挙する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn bare_csv_calculation_binding_does_not_create_sidecar_files() {
    let directory = tempdir().unwrap();
    let csv_path = directory.path().join("data.csv");
    fs::write(&csv_path, SOURCE_CSV).unwrap();

    let mut document = CsvDocument::open(&csv_path).unwrap();
    let mut engine = CalculationEngine::default();
    rowly_dsl::run_with_calculation_engine(PRODUCT_RANGE_DSL, &mut document, &mut engine).unwrap();
    engine.recalculate_all(&document).unwrap();
    assert_eq!(evaluated_value(&engine, "C3").as_deref(), Some("12"));

    // 通常のCSV編集と保存をしても、Bindingはsession内に留まる。
    document.set_cell(1, 0, "5").unwrap();
    document.save().unwrap();

    let mut names = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["data.csv".to_owned()]);
    assert_eq!(
        fs::read_to_string(&csv_path).unwrap(),
        "A,B,C\n5,2,\n3,4,\n"
    );
}
