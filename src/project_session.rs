//! Editable sessions for directory projects and `.rowlyx` packages.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use thiserror::Error;

use crate::{
    process::CsvDocument,
    project::{
        ProjectError, RowlyProject, SourceKind, resolve_project_reference,
        write_project_file_atomic,
    },
    project_init::{self, ProjectInitError},
    rowlyx::{RowlyxArchive, RowlyxError, pack_project},
};

#[derive(Debug)]
// {
//   責務: [
//     ProjectBacking: ProjectSessionが保存時に使うdirectory manifestまたはpackage workspaceを保持する
//   ]
//   補足: [
//     Directory: manifest_pathをbackingとして使う
//     Package: archive_path、workspace内manifest_path、一時workspaceのTempDirを保持する
//   ]
// }
enum ProjectBacking {
    Directory {
        manifest_path: PathBuf,
    },
    Package {
        archive_path: PathBuf,
        manifest_path: PathBuf,
        workspace: TempDir,
    },
}


#[derive(Debug)]
// {
//   責務: [
//     ProjectFileSnapshot: save前のmanifest file状態を保持して後続処理失敗時に復元する
//   ]
// }
struct ProjectFileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

impl ProjectFileSnapshot {
    fn capture(path: &Path) -> Result<Self, ProjectSessionError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(ProjectSessionError::Snapshot {
                    path: path.display().to_string(),
                    message: error.to_string(),
                });
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            bytes,
        })
    }

    fn restore(&self) -> Result<(), ProjectSessionError> {
        match &self.bytes {
            Some(bytes) => {
                write_project_file_atomic(&self.path, bytes)?;
            }
            None => match fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ProjectSessionError::Rollback {
                        primary: "restore manifest snapshot".into(),
                        rollback: error.to_string(),
                    });
                }
            },
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ProjectBackingKind: sessionが編集しているprojectの保存形式を表す
/// ]
/// 補足: [Directory: .rwprj directory project, Package: .rowlyx archive project]
/// ```
pub enum ProjectBackingKind {
    /// .rwprj manifestを直接使う。
    Directory,
    /// managed workspace経由で.rowlyxを編集する。
    Package,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ProjectDirtyState: data、project config、package archiveそれぞれの未保存状態を独立して記録する
/// ]
/// フィールド: [
/// data: project source dataに未保存変更がある
/// structure_or_config: manifestまたはpersisted configに未保存変更がある
/// package: workspace変更をarchiveへ反映する未完了状態
/// ]
/// ```
pub struct ProjectDirtyState {
    data: bool,
    structure_or_config: bool,
    package: bool,
}

impl ProjectDirtyState {
    /// ```text
    /// 責務: [data_is_dirty: data categoryのdirty flagを返す]
    /// 処理: [data flagを読む]
    /// 引数: []
    /// 戻り値: [bool: dataに未保存変更があればtrue]
    /// ```
    pub fn data_is_dirty(self) -> bool {
        self.data
    }

    /// ```text
    /// 責務: [structure_or_config_is_dirty: project structure / configのdirty flagを返す]
    /// 処理: [structure_or_config flagを読む]
    /// 引数: []
    /// 戻り値: [bool: manifestまたはpersisted configに未保存変更があればtrue]
    /// ```
    pub fn structure_or_config_is_dirty(self) -> bool {
        self.structure_or_config
    }

    /// ```text
    /// 責務: [package_is_dirty: package archiveのdirty flagを返す]
    /// 処理: [package flagを読む]
    /// 引数: []
    /// 戻り値: [bool: workspaceをarchiveへ保存する必要があればtrue]
    /// ```
    pub fn package_is_dirty(self) -> bool {
        self.package
    }

    /// ```text
    /// 責務: [is_dirty: いずれかの保存対象に未保存変更があるか返す]
    /// 処理: [data、structure_or_config、package flagをORする]
    /// 引数: []
    /// 戻り値: [bool: 少なくとも一つのflagがtrueならtrue]
    /// ```
    pub fn is_dirty(self) -> bool {
        self.data || self.structure_or_config || self.package
    }
}

#[derive(Debug)]
/// ```text
/// 責務: [
/// ProjectSession: directoryまたはpackageをbackingとしてproject modelと未保存状態を管理する
/// ]
/// 補足: [.rowlyxはsession中managed temporary workspaceへ展開し、save時にarchiveへ再packする]
/// フィールド: [project: 編集対象manifest model, backing: 保存元形式とpath, dirty: 保存カテゴリごとの状態]
/// ```
pub struct ProjectSession {
    project: RowlyProject,
    backing: ProjectBacking,
    dirty: ProjectDirtyState,
    staged_init: Option<project_init::StagedProjectInit>,
}

impl ProjectSession {
    /// ```text
    /// 責務: [open: .rwprj directory projectまたは.rowlyx packageの編集sessionを開く]
    /// 処理: [
    /// 1: extensionでbacking種別を判定する
    /// 2: packageならmanaged temporary workspaceへextractし、directoryならmanifestを読む
    /// 3: manifestからproject modelを構築し、dirty stateを初期化する
    /// ]
    /// 引数: [path: 開くmanifestまたはarchive path]
    /// 戻り値: [Self: projectとbackingを保持するclean session]
    /// 副作用: [package session用にtemporary workspaceを作成する]
    /// エラー: [ProjectSessionError: backing種別、workspace、archive、またはmanifestの失敗。init設定はapply_safe_init時に読む]
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProjectSessionError> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().and_then(|value| value.to_str());
        if extension.is_some_and(|value| value.eq_ignore_ascii_case("rowlyx")) {
            let archive = RowlyxArchive::open(&path)?;
            let workspace = tempfile::Builder::new()
                .prefix("rowly-session-")
                .tempdir()
                .map_err(ProjectSessionError::TemporaryWorkspace)?;
            let manifest_path = archive.extract_to(workspace.path())?;
            let project = RowlyProject::load(&manifest_path)?;
            Ok(Self {
                project,
                backing: ProjectBacking::Package {
                    archive_path: path,
                    manifest_path,
                    workspace,
                },
                dirty: ProjectDirtyState::default(),
                staged_init: None,
            })
        } else if extension.is_some_and(|value| value.eq_ignore_ascii_case("rwprj")) {
            let project = RowlyProject::load(&path)?;
            Ok(Self {
                project,
                backing: ProjectBacking::Directory {
                    manifest_path: path,
                },
                dirty: ProjectDirtyState::default(),
                staged_init: None,
            })
        } else {
            Err(ProjectSessionError::UnsupportedBacking(
                path.display().to_string(),
            ))
        }
    }

    /// ```text
    /// 責務: [project: sessionのproject modelをimmutableに参照する]
    /// 処理: [保持中のprojectへの参照を返す]
    /// 引数: []
    /// 戻り値: [&RowlyProject: session内project model]
    /// ```
    pub fn project(&self) -> &RowlyProject {
        &self.project
    }

    /// ```text
    /// 責務: [save_generated_column_types: source documentのcolumn type設定をgenerated init scriptへ保存する]
    /// 処理: [source idを確認し、project initのgenerated設定を更新する]
    /// 引数: [source_id: project source id, document: type宣言を抽出するCSV document]
    /// 戻り値: [(): generated config保存成功時に値を返さない]
    /// 副作用: [必要ならinit.rlyとuser.rlyを作成し、generated init fileを更新してstructure / config dirtyを立てる]
    /// エラー: [ProjectSessionError: source id、設定検証・既存script読込、init / user / generated fileの作成・writeの失敗]
    /// ```
    pub fn save_generated_column_types(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        let staged = project_init::stage_generated_column_types(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [save_generated_validation_rules: sourceのvalidation ruleをgenerated init DSLへ保存する]
    /// 処理: [source idを確認し、project initのgenerated設定を更新する]
    /// 引数: [source_id: project source id, document: validation ruleを抽出するCSV document]
    /// 戻り値: [(): generated config保存成功時に値を返さない]
    /// 副作用: [必要ならinit.rlyとuser.rlyを作成し、generated init fileを更新してstructure / config dirtyを立てる]
    /// エラー: [ProjectSessionError: source id、設定検証・既存script読込、init / user / generated fileの作成・writeの失敗]
    /// ```
    pub fn save_generated_validation_rules(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        let staged = project_init::stage_generated_validation_rules(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [open_source_document: 宣言済みfile sourceを開きsafe init configを適用する]
    /// 処理: [source idと種別を確認し、sidecar metadataを読まずにresolved CSVを開いてconfig-only initを適用する]
    /// 引数: [source_id: 開くfile sourceのstable id]
    /// 戻り値: [CsvDocument: safe init適用後のCSV document]
    /// エラー: [ProjectSessionError: source不明 / directory指定、CSV open、またはsafe initの失敗]
    /// ```
    pub fn open_source_document(
        &self,
        source_id: &str,
    ) -> Result<CsvDocument, ProjectSessionError> {
        let source = self
            .project
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .ok_or_else(|| ProjectSessionError::SourceNotFound(source_id.to_owned()))?;
        if source.kind != SourceKind::File {
            return Err(ProjectSessionError::DirectorySource(source_id.to_owned()));
        }
        let path = resolve_project_reference(self.manifest_path(), &source.path);
        let mut document = CsvDocument::open_without_metadata(path)?;
        self.apply_safe_init(source_id, &mut document)?;
        Ok(document)
    }

    /// ```text
    /// 責務: [apply_safe_init: 対象documentへsupported project configを適用する]
    /// 処理: [config-only project init interpreterを使い、user macroや一般DSL statementは実行しない]
    /// 引数: [source_id: 設定を選ぶsource id, document: 設定を適用するmutable CSV document]
    /// 戻り値: [(): config適用成功時に値を返さない]
    /// 副作用: [supported configに基づきdocument metadata等を更新する]
    /// エラー: [ProjectSessionError: sourceまたはinit configの検証・適用失敗]
    /// ```
    pub fn apply_safe_init(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if let Some(staged) = &self.staged_init {
            staged.apply_source(source_id, document)?;
        } else {
            project_init::apply_safe_init(
                &self.project,
                self.manifest_path(),
                source_id,
                document,
            )?;
        }
        Ok(())
    }

    /// ```text
    /// 責務: [project_mut: sessionのmanifest modelをmutableに参照する]
    /// 処理: [mutable referenceを返す前にstructure / config dirtyを記録する]
    /// 引数: []
    /// 戻り値: [&mut RowlyProject: 呼び出し元が編集できるproject model]
    /// 副作用: [structure_or_config dirtyを立て、package sessionならpackage dirtyも立てる]
    /// ```
    pub fn project_mut(&mut self) -> &mut RowlyProject {
        self.mark_structure_or_config_dirty();
        &mut self.project
    }

    /// ```text
    /// 責務: [mark_data_dirty: sessionが所有するdataに未保存変更があると記録する]
    /// 処理: [data dirtyを立て、package sessionではpackage dirtyも立てる]
    /// 引数: []
    /// 戻り値: [(): 状態更新後は値を返さない]
    /// 副作用: [dirty stateを更新する]
    /// ```
    pub fn mark_data_dirty(&mut self) {
        self.dirty.data = true;
        self.mark_package_dirty_if_needed();
    }

    /// ```text
    /// 責務: [mark_data_saved: 所有documentのsave後にdata dirty flagを解除する]
    /// 処理: [data dirtyだけをfalseにする]
    /// 引数: []
    /// 戻り値: [(): 状態更新後は値を返さない]
    /// 副作用: [data dirtyを解除する。package dirtyはarchive saveまで維持される]
    /// ```
    pub fn mark_data_saved(&mut self) {
        self.dirty.data = false;
    }

    /// ```text
    /// 責務: [mark_structure_or_config_dirty: manifestまたはpersisted configに未保存変更があると記録する]
    /// 処理: [structure_or_config dirtyを立て、package sessionではpackage dirtyも立てる]
    /// 引数: []
    /// 戻り値: [(): 状態更新後は値を返さない]
    /// 副作用: [dirty stateを更新する]
    /// ```
    pub fn mark_structure_or_config_dirty(&mut self) {
        self.dirty.structure_or_config = true;
        self.mark_package_dirty_if_needed();
    }

    // {
    //   責務: [mark_package_dirty_if_needed: package backingのworkspace変更をarchive dirtyとして記録する]
    //   処理: [backingがPackageのときだけpackage dirtyを立てる]
    //   引数: []
    //   戻り値: [(): 状態更新後は値を返さない]
    //   副作用: [package dirtyを必要な場合に更新する]
    // }
    fn mark_package_dirty_if_needed(&mut self) {
        if matches!(self.backing, ProjectBacking::Package { .. }) {
            self.dirty.package = true;
        }
    }

    /// ```text
    /// 責務: [working_directory: directory projectまたはpackage workspaceのrootを返す]
    /// 処理: [Directoryならmanifest parent、Packageならmanaged workspace pathを返す]
    /// 引数: []
    /// 戻り値: [&Path: sessionが参照する作業directory]
    /// ```
    pub fn working_directory(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path } => {
                manifest_path.parent().unwrap_or_else(|| Path::new("."))
            }
            ProjectBacking::Package { workspace, .. } => workspace.path(),
        }
    }

    /// ```text
    /// 責務: [manifest_path: sessionが編集しているmanifest pathを返す]
    /// 処理: [backing種別に応じて元directory manifestまたはworkspace内manifestを返す]
    /// 引数: []
    /// 戻り値: [&Path: sessionのcurrent manifest path]
    /// 補足: [package sessionではarchive内ではなく展開workspaceを指す]
    /// ```
    pub fn manifest_path(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path }
            | ProjectBacking::Package { manifest_path, .. } => manifest_path,
        }
    }

    /// ```text
    /// 責務: [backing_path: sessionに結び付いた元project pathを返す]
    /// 処理: [Directoryならmanifest、Packageならarchive pathを返す]
    /// 引数: []
    /// 戻り値: [&Path: 元project manifestまたはarchive path]
    /// ```
    pub fn backing_path(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path } => manifest_path,
            ProjectBacking::Package { archive_path, .. } => archive_path,
        }
    }

    /// ```text
    /// 責務: [backing_kind: session backingの形式を返す]
    /// 処理: [内部backing variantをProjectBackingKindへ写す]
    /// 引数: []
    /// 戻り値: [ProjectBackingKind: DirectoryまたはPackage]
    /// ```
    pub fn backing_kind(&self) -> ProjectBackingKind {
        match self.backing {
            ProjectBacking::Directory { .. } => ProjectBackingKind::Directory,
            ProjectBacking::Package { .. } => ProjectBackingKind::Package,
        }
    }

    /// ```text
    /// 責務: [is_dirty: sessionのいずれかの保存categoryがdirtyか返す]
    /// 処理: [dirty stateの全flagを集約する]
    /// 引数: []
    /// 戻り値: [bool: 未保存変更があればtrue]
    /// ```
    pub fn is_dirty(&self) -> bool {
        self.dirty.is_dirty()
    }

    /// ```text
    /// 責務: [dirty_state: sessionのcategory別dirty stateを返す]
    /// 処理: [現在のdirty stateをcopyして返す]
    /// 引数: []
    /// 戻り値: [ProjectDirtyState: data / config / packageの状態snapshot]
    /// ```
    pub fn dirty_state(&self) -> ProjectDirtyState {
        self.dirty
    }

    /// ```text
    /// 責務: [save: manifestを保存し、package sessionではworkspaceを元archiveへ再packする]
    /// 処理: [
    /// 1: current manifestを保存する
    /// 2: package backingならworkspaceからarchiveを作り、validation後に元archiveを置換する
    /// 3: 成功後にconfig dirtyを解除し、未保存data状態を維持する
    /// ]
    /// 引数: []
    /// 戻り値: [(): backing保存成功時に値を返さない]
    /// 副作用: [manifestを更新し、packageならoriginal archiveをatomic replacementする]
    /// エラー: [ProjectSessionError: manifest save、pack、validation、またはarchive replacementの失敗。session dirtyは保持する]
    /// ```
    pub fn save(&mut self) -> Result<(), ProjectSessionError> {
        let manifest_path = self.manifest_path().to_path_buf();
        let manifest_snapshot = ProjectFileSnapshot::capture(&manifest_path)?;

        self.project.save(&manifest_path)?;

        let init_snapshot = match &self.staged_init {
            Some(staged) => match staged.commit() {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    if let Err(rollback) = manifest_snapshot.restore() {
                        return Err(ProjectSessionError::Rollback {
                            primary: error.to_string(),
                            rollback: rollback.to_string(),
                        });
                    }
                    return Err(error.into());
                }
            },
            None => None,
        };

        let package_result = if let ProjectBacking::Package {
            archive_path,
            manifest_path,
            ..
        } = &self.backing
        {
            pack_project(manifest_path, archive_path).map_err(ProjectSessionError::from)
        } else {
            Ok(())
        };

        if let Err(error) = package_result {
            let mut rollback_errors = Vec::new();
            if let Some(snapshot) = &init_snapshot {
                if let Err(rollback) = snapshot.restore() {
                    rollback_errors.push(rollback.to_string());
                }
            }
            if let Err(rollback) = manifest_snapshot.restore() {
                rollback_errors.push(rollback.to_string());
            }
            if !rollback_errors.is_empty() {
                return Err(ProjectSessionError::Rollback {
                    primary: error.to_string(),
                    rollback: rollback_errors.join("; "),
                });
            }
            return Err(error);
        }

        self.staged_init = None;
        let data_dirty = self.dirty.data;
        self.dirty = ProjectDirtyState {
            data: data_dirty,
            structure_or_config: false,
            package: data_dirty && matches!(self.backing, ProjectBacking::Package { .. }),
        };
        Ok(())
    }
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [
/// ProjectSessionError: session open、source操作、safe init、saveの失敗を統合して表す
/// ]
/// 補足: [
/// UnsupportedBacking: .rwprj / .rowlyx以外のpathが指定された
/// TemporaryWorkspace: package編集用workspaceを作れない
/// Project: manifest modelのread、parse、validation、save失敗
/// ProjectInit: config-only initの読込・適用、またはgenerated設定の検証・保存失敗
/// Rowlyx: archiveのopen、extract、pack、validation失敗
/// Document: source CSV documentの失敗
/// SourceNotFound: 指定source idがmanifestにない
/// DirectorySource: CSV documentを開く操作にdirectory sourceが指定された
/// ]
/// ```
pub enum ProjectSessionError {
    #[error("unsupported project backing `{0}`; expected `.rwprj` or `.rowlyx`")]
    UnsupportedBacking(String),
    #[error("failed to create a managed project workspace: {0}")]
    TemporaryWorkspace(io::Error),
    #[error("failed to snapshot project file `{path}`: {message}")]
    Snapshot { path: String, message: String },
    #[error("project save failed: {primary}; rollback also failed: {rollback}")]
    Rollback { primary: String, rollback: String },
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    ProjectInit(#[from] ProjectInitError),
    #[error(transparent)]
    Rowlyx(#[from] RowlyxError),
    #[error(transparent)]
    Document(#[from] crate::process::DocumentError),
    #[error("project source `{0}` does not exist")]
    SourceNotFound(String),
    #[error("project source `{0}` is a directory; open a file source instead")]
    DirectorySource(String),
}


#[cfg(test)]
mod staging_tests {
    use std::{fs, path::{Path, PathBuf}};

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::{
        process::{ColumnType, ValidationRule, ValidationTarget},
        project::{ProjectScripts, ProjectSource},
    };

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

    fn generated_path(manifest: &Path) -> PathBuf {
        manifest.parent().unwrap().join("scripts/generated.rly")
    }

    #[test]
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
}
