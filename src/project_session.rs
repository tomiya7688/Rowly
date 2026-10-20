//! Editable sessions for directory projects and `.rowlyx` packages.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[path = "project_session/configuration.rs"]
mod configuration;

use tempfile::TempDir;
use thiserror::Error;

use crate::{
    project::{ProjectError, RowlyProject, write_project_file_atomic},
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
//   責務: [ProjectFileSnapshot: save前のmanifest bytesを保持し、後続処理失敗時の復元に使う]
//   フィールド: [path: manifest path, bytes: save前bytes。manifestが未作成ならNone。]
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
            Some(bytes) => write_project_file_atomic(&self.path, bytes)?,
            None => match fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ProjectSessionError::Rollback {
                        primary: "manifest snapshot restore".to_owned(),
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
/// フィールド: [project: 編集対象manifest model, backing: 保存元形式とpath, dirty: 保存カテゴリごとの状態, staged_init: save前のgenerated init config]
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
    /// 責務: [save: manifestとstaged configを保存し、package sessionではworkspaceを元archiveへ再packする]
    /// 処理: [
    /// 1: manifestの保存前snapshotを取得する
    /// 2: manifestを保存し、staged init configを確定する
    /// 3: package backingならworkspaceからarchiveを作り、validation後に元archiveを置換する
    /// 4: 失敗時は保存前manifestと確定済みconfigを復元し、成功時だけstagingを破棄する
    /// ]
    /// 引数: []
    /// 戻り値: [Result<(), ProjectSessionError>: backing保存成功、または保存・復元error]
    /// 副作用: [manifestとgenerated configを更新し、packageならoriginal archiveをatomic replacementする]
    /// エラー: [ProjectSessionError: manifest/config save、pack、validation、archive replacement、またはrollbackの失敗。stagingとdirty stateを保持する]
    /// ```
    pub fn save(&mut self) -> Result<(), ProjectSessionError> {
        let manifest_path = self.manifest_path().to_path_buf();
        if let Some(staged_init) = &self.staged_init {
            staged_init.validate_for_project(&self.project, &manifest_path)?;
        }
        let manifest_snapshot = ProjectFileSnapshot::capture(&manifest_path)?;
        self.project.save(&manifest_path)?;

        let init_snapshot = match &self.staged_init {
            Some(staged_init) => match staged_init.commit() {
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
            if let Some(snapshot) = &init_snapshot
                && let Err(rollback) = snapshot.restore()
            {
                rollback_errors.push(rollback.to_string());
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
    #[error(transparent)]
    Calculation(#[from] crate::process::calculation::CalculationError),
    #[error("project source `{0}` does not exist")]
    SourceNotFound(String),
    #[error("project source `{0}` is a directory; open a file source instead")]
    DirectorySource(String),
}
