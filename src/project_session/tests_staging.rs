use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::{TempDir, tempdir};

use super::*;
use crate::{
    process::{ColumnType, ValidationRule, ValidationTarget},
    project::{ProjectScripts, ProjectSource, SourceKind},
};

// {
//   責務: [create_project: 型宣言・検証規則のstaging検証に使うprojectを作る。],
//   処理: [manifest、source、generated/user scriptを一時directoryへ配置する。],
//   引数: [],
//   戻り値: [(TempDir, PathBuf): directoryとmanifest path。fixture作成に失敗した場合はtestを失敗させる。]
// }
fn create_project() -> (TempDir, PathBuf) {
    let directory = tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("scripts/macros")).unwrap();
    fs::create_dir_all(root.join("history")).unwrap();
    fs::write(root.join("data.csv"), "Name,Score\nAlice,10\nBob,20\n").unwrap();
    fs::write(
        root.join("scripts/init.rly"),
        "INCLUDE GENERATED\nINCLUDE USER\n",
    )
    .unwrap();
    fs::write(
        root.join("scripts/generated.rly"),
        "SET This.Project.Source(\"data\").Worksheet.Column(\"Score\").Type = String\n",
    )
    .unwrap();
    fs::write(root.join("scripts/user.rly"), "' keep user config\n").unwrap();

    let project = RowlyProject {
        name: "Staging Test".into(),
        sources: vec![ProjectSource {
            id: "data".into(),
            kind: SourceKind::File,
            path: PathBuf::from("data.csv"),
            recursive: false,
            search_root: None,
            schema: Some(vec!["Name".into(), "Score".into()]),
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
//   責務: [generated_path: fixtureのgenerated script pathを返す。],
//   処理: [manifestの親directoryから固定のscript位置を求める。],
//   引数: [manifest: fixtureのmanifest path。],
//   戻り値: [PathBuf: generated scriptのpath。]
// }
fn generated_path(manifest: &Path) -> PathBuf {
    manifest.parent().unwrap().join("scripts/generated.rly")
}

#[test]
// {
//   責務: [generated_config_is_staged_until_save_and_visible_inside_session: 未保存設定のsession内参照とsave後の復元を検証する。],
//   処理: [型・検証規則をstageし、disk非変更、save、再open、user script保持を確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn generated_config_is_staged_until_save_and_visible_inside_session() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let user = manifest.parent().unwrap().join("scripts/user.rly");
    let before_generated = fs::read_to_string(&generated).unwrap();
    let before_user = fs::read_to_string(&user).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    let mut document = session.open_source_document("data").unwrap();
    assert_eq!(
        document.column_type_declaration_by_header("Score").unwrap(),
        Some(ColumnType::String)
    );

    document
        .set_column_type_declaration_by_header("Score", ColumnType::Integer)
        .unwrap();
    document
        .set_validation_rule(
            ValidationTarget::Header("Score".into()),
            ValidationRule::AllowedValues(vec!["10".into(), "20".into()]),
        )
        .unwrap();

    session
        .save_generated_column_types("data", &document)
        .unwrap();
    session
        .save_generated_validation_rules("data", &document)
        .unwrap();

    assert_eq!(fs::read_to_string(&generated).unwrap(), before_generated);
    assert_eq!(fs::read_to_string(&user).unwrap(), before_user);
    assert!(session.dirty_state().structure_or_config_is_dirty());

    let staged = session.open_source_document("data").unwrap();
    assert_eq!(
        staged.column_type_declaration_by_header("Score").unwrap(),
        Some(ColumnType::Integer)
    );
    assert!(staged.validation_rules().any(|(target, rule)| {
        target == &ValidationTarget::Header("Score".into())
            && rule == &ValidationRule::AllowedValues(vec!["10".into(), "20".into()])
    }));

    session.save().unwrap();

    let saved = fs::read_to_string(&generated).unwrap();
    assert!(saved.contains("Type = Integer"));
    assert!(saved.contains("Validation.AllowedValues = [\"10\", \"20\"]"));
    assert_eq!(fs::read_to_string(&user).unwrap(), before_user);
    assert!(!session.is_dirty());

    let reopened = ProjectSession::open(&manifest).unwrap();
    let restored = reopened.open_source_document("data").unwrap();
    assert_eq!(
        restored.column_type_declaration_by_header("Score").unwrap(),
        Some(ColumnType::Integer)
    );
    assert!(restored.validation_rules().any(|(target, rule)| {
        target == &ValidationTarget::Header("Score".into())
            && rule == &ValidationRule::AllowedValues(vec!["10".into(), "20".into()])
    }));
}

#[test]
// {
//   責務: [manifest_save_failure_keeps_generated_file_and_staging_for_retry: manifest保存失敗時の原状維持と再試行を検証する。],
//   処理: [不正なproject名で保存を失敗させ、disk非変更とdirty保持、修正後の保存成功を確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn manifest_save_failure_keeps_generated_file_and_staging_for_retry() {
    let (_directory, manifest) = create_project();
    let generated = generated_path(&manifest);
    let before_manifest = fs::read(&manifest).unwrap();
    let before_generated = fs::read(&generated).unwrap();

    let mut session = ProjectSession::open(&manifest).unwrap();
    let mut document = session.open_source_document("data").unwrap();
    document
        .set_column_type_declaration_by_header("Score", ColumnType::Integer)
        .unwrap();
    session
        .save_generated_column_types("data", &document)
        .unwrap();
    session.project_mut().name.clear();

    assert!(session.save().is_err());
    assert_eq!(fs::read(&manifest).unwrap(), before_manifest);
    assert_eq!(fs::read(&generated).unwrap(), before_generated);
    assert!(session.dirty_state().structure_or_config_is_dirty());

    session.project_mut().name = "Recovered".into();
    session.save().unwrap();
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("Type = Integer")
    );
    assert!(!session.is_dirty());
}

#[test]
// {
//   責務: [package_repack_failure_restores_workspace_and_keeps_original_archive_for_retry: package再圧縮失敗時のarchive保持と再試行を検証する。],
//   処理: [不正なsourceで保存を失敗させ、元archive保持、dirty保持、source修正後の保存成功を確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn package_repack_failure_restores_workspace_and_keeps_original_archive_for_retry() {
    let (directory, manifest) = create_project();
    let archive = directory.path().join("project.rowlyx");
    pack_project(&manifest, &archive).unwrap();
    let archive_before = fs::read(&archive).unwrap();

    let mut session = ProjectSession::open(&archive).unwrap();
    let mut document = session.open_source_document("data").unwrap();
    document
        .set_column_type_declaration_by_header("Score", ColumnType::Integer)
        .unwrap();
    session
        .save_generated_column_types("data", &document)
        .unwrap();

    fs::create_dir(session.working_directory().join("broken-source")).unwrap();
    session.project_mut().sources[0].path = PathBuf::from("broken-source");

    assert!(session.save().is_err());
    assert_eq!(fs::read(&archive).unwrap(), archive_before);
    assert!(session.is_dirty());

    session.project_mut().sources[0].path = PathBuf::from("data.csv");
    session.save().unwrap();
    assert!(!session.is_dirty());

    let reopened = ProjectSession::open(&archive).unwrap();
    let restored = reopened.open_source_document("data").unwrap();
    assert_eq!(
        restored.column_type_declaration_by_header("Score").unwrap(),
        Some(ColumnType::Integer)
    );
}
