//! Versioned `.rwprj` project manifests. The manifest stores references only;
//! CSV contents remain in their source files.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use thiserror::Error;

use crate::data::replace_file;

/// ```text
/// 責務: [PROJECT_FORMAT: Rowly project manifestを識別するformat名]
/// 補足: [保存形式と読込検証で共通して使う固定値]
/// ```
pub const PROJECT_FORMAT: &str = "rowly-project";
/// ```text
/// 責務: [PROJECT_VERSION: 現在サポートするproject manifestのversion]
/// 補足: [保存形式と読込検証で共通して使う固定値]
/// ```
pub const PROJECT_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// RowlyProject: source参照、script path、history directoryから成るproject manifestのmodel
/// ]
/// フィールド: [
/// name: projectの表示名
/// sources: projectが宣言するsource群
/// scripts: init、generated、user scriptとmacro directoryの参照
/// history: project history directoryへのpath参照
/// ]
/// ```
pub struct RowlyProject {
    /// ```text
    /// 責務: [name: projectの空でない表示名]
    /// ```
    pub name: String,
    /// ```text
    /// 責務: [sources: stable idを持つproject source定義]
    /// ```
    pub sources: Vec<ProjectSource>,
    /// ```text
    /// 責務: [scripts: projectで参照する用途別script path群]
    /// ```
    pub scripts: ProjectScripts,
    /// ```text
    /// 責務: [history: project history directoryへのpath参照]
    /// ```
    pub history: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ProjectSource: source id、種別、path、探索境界、schema識別情報をまとめる
/// ]
/// フィールド: [
/// id: project内でsourceを識別するstable id
/// kind: fileまたはdirectoryのsource種別
/// path: manifestから解決するsource path
/// recursive: directory sourceで子directoryも列挙するか
/// search_root: 欠落file sourceの再link探索境界
/// schema: 再link候補を識別するordered header signature
/// ]
/// ```
pub struct ProjectSource {
    /// ```text
    /// 責務: [id: manifest内のsourceを一意に識別するstable id]
    /// ```
    pub id: String,
    /// ```text
    /// 責務: [kind: source pathをfileまたはdirectoryとして扱う種別]
    /// ```
    pub kind: SourceKind,
    /// ```text
    /// 責務: [path: manifest位置を基準に解決するsource参照]
    /// ```
    pub path: PathBuf,
    /// ```text
    /// 責務: [recursive: directory sourceの子directoryも対象にするか]
    /// ```
    pub recursive: bool,
    /// ```text
    /// 責務: [search_root: file再linkの探索を制限する任意のdirectory参照]
    /// ```
    pub search_root: Option<PathBuf>,
    /// ```text
    /// 責務: [schema: 移動したCSVを識別する任意のordered header signature]
    /// ```
    pub schema: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// SourceKind: project source pathをfileまたはdirectoryとして扱う種別
/// ]
/// 補足: [
/// File: 単一CSV fileをsourceとして扱う
/// Directory: CSVを含むdirectoryをsourceとして扱う
/// ]
/// ```
pub enum SourceKind {
    /// 単一file source。
    File,
    /// CSVを列挙するdirectory source。
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ProjectScripts: projectで参照する用途別script pathを保持する
/// ]
/// フィールド: [
/// init: project configを初期化するscript
/// generated: アプリが生成・管理するscript
/// user: user script
/// macros: project macro directoryへのpath参照
/// ]
/// ```
pub struct ProjectScripts {
    /// ```text
    /// 責務: [init: config初期化用scriptへのpath参照]
    /// ```
    pub init: PathBuf,
    /// ```text
    /// 責務: [generated: アプリ管理scriptへのpath参照]
    /// ```
    pub generated: PathBuf,
    /// ```text
    /// 責務: [user: project user scriptへのpath参照]
    /// ```
    pub user: PathBuf,
    /// ```text
    /// 責務: [macros: project macro directoryへのpath参照]
    /// ```
    pub macros: PathBuf,
}

impl RowlyProject {
    /// ```text
    /// 責務: [load: manifest JSONを読み込み、RowlyProjectとしてparse・validateする]
    /// 処理: [fileをreadし、JSON valueからproject modelを構築する]
    /// 引数: [project_path: 読み込むmanifest path]
    /// 戻り値: [Self: 検証済みproject model。relative pathはmanifest内の表記を保持する]
    /// エラー: [ProjectError: file read、JSON parse、またはmanifest schemaの失敗]
    /// ```
    pub fn load(project_path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let project_path = project_path.as_ref();
        let bytes = fs::read(project_path).map_err(|error| ProjectError::Read {
            path: project_path.display().to_string(),
            message: error.to_string(),
        })?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| ProjectError::Parse {
            path: project_path.display().to_string(),
            message: error.to_string(),
        })?;
        Self::from_value(value)
    }

    /// ```text
    /// 責務: [save: project modelをpretty JSON manifestとして保存する]
    /// 処理: [modelをvalidateし、manifest JSONをatomic file replacementで書き込む]
    /// 引数: [project_path: 保存先manifest path]
    /// 戻り値: [(): manifest保存成功時に値を返さない]
    /// 副作用: [manifest fileを置換する。source dataはcopyもembedもしない]
    /// エラー: [ProjectError: model不正またはmanifest file writeの失敗]
    /// 補足: [PathBuf fieldにUTF-8化できないpathがある場合、JSON value構築時にpanicする]
    /// ```
    pub fn save(&self, project_path: impl AsRef<Path>) -> Result<(), ProjectError> {
        self.validate()?;
        let value = json!({
            "format": PROJECT_FORMAT,
            "version": PROJECT_VERSION,
            "name": self.name,
            "sources": self.sources.iter().map(|source| {
                let mut item = json!({
                    "id": source.id,
                    "type": match source.kind { SourceKind::File => "file", SourceKind::Directory => "directory" },
                    "path": source.path,
                });
                if source.kind == SourceKind::Directory { item["recursive"] = json!(source.recursive); }
                if let Some(search_root) = &source.search_root { item["search_root"] = json!(search_root); }
                if let Some(schema) = &source.schema { item["schema"] = json!(schema); }
                item
            }).collect::<Vec<_>>(),
            "scripts": {
                "init": self.scripts.init,
                "generated": self.scripts.generated,
                "user": self.scripts.user,
                "macros": self.scripts.macros,
            },
            "history": self.history,
        });
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|error| ProjectError::Schema(error.to_string()))?;
        let path = project_path.as_ref();
        write_project_file_atomic(path, &bytes)
    }

    /// ```text
    /// 責務: [resolve_sources: manifest pathを基準に宣言済みsource参照を解決する]
    /// 処理: [各source pathとsearch_rootにreference解決を適用する]
    /// 引数: [project_path: relative referenceの基準となるmanifest path]
    /// 戻り値: [Vec<ResolvedSource>: source設定を保った解決済み参照一覧]
    /// 補足: [directory列挙やpathの存在確認は行わない]
    /// ```
    pub fn resolve_sources(&self, project_path: impl AsRef<Path>) -> Vec<ResolvedSource> {
        self.sources
            .iter()
            .map(|source| ResolvedSource {
                id: source.id.clone(),
                kind: source.kind,
                path: resolve_project_reference(project_path.as_ref(), &source.path),
                recursive: source.recursive,
                search_root: source
                    .search_root
                    .as_ref()
                    .map(|path| resolve_project_reference(project_path.as_ref(), path)),
                schema: source.schema.clone(),
            })
            .collect()
    }

    /// ```text
    /// 責務: [resolve_source_locations: 欠落file sourceをsearch_root内で限定的に再linkする]
    /// 処理: [
    /// 1: 現在のpathを確認し、存在すればAvailableとして返す
    /// 2: file sourceに限りfilenameとsaved header schemaが一致する候補を探す
    /// 3: 候補が一つならpathを更新し、0件または複数ならMissing / Ambiguousを返す
    /// ]
    /// 引数: [project_path: relative referenceの基準となるmanifest path]
    /// 戻り値: [Vec<SourceResolution>: sourceごとの状態と解決path]
    /// 副作用: [一意に再linkされたsourceのpathを更新する。stable source idは維持する]
    /// エラー: [ProjectError: search_root directoryの走査に失敗した]
    /// ```
    pub fn resolve_source_locations(
        &mut self,
        project_path: impl AsRef<Path>,
    ) -> Result<Vec<SourceResolution>, ProjectError> {
        let project_path = project_path.as_ref();
        let mut results = Vec::with_capacity(self.sources.len());
        for source in &mut self.sources {
            let path = resolve_project_reference(project_path, &source.path);
            if source_path_exists(source.kind, &path) {
                results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Available,
                    path: Some(path),
                });
                continue;
            }
            if source.kind != SourceKind::File {
                results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Missing,
                    path: None,
                });
                continue;
            }
            let (Some(search_root), Some(schema)) = (&source.search_root, &source.schema) else {
                results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Missing,
                    path: None,
                });
                continue;
            };
            let search_root = resolve_project_reference(project_path, search_root);
            if !search_root.is_dir() {
                results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Missing,
                    path: None,
                });
                continue;
            }
            let mut candidates = Vec::new();
            find_matching_sources(&search_root, path.file_name(), schema, &mut candidates)?;
            match candidates.as_slice() {
                [candidate] => {
                    source.path = candidate.clone();
                    results.push(SourceResolution {
                        id: source.id.clone(),
                        status: SourceStatus::Relinked,
                        path: Some(candidate.clone()),
                    });
                }
                [] => results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Missing,
                    path: None,
                }),
                _ => results.push(SourceResolution {
                    id: source.id.clone(),
                    status: SourceStatus::Ambiguous,
                    path: None,
                }),
            }
        }
        Ok(results)
    }

    /// ```text
    /// 責務: [capture_source_schemas: file sourceのCSV headerを再link用signatureとして記録する]
    /// 処理: [existing file sourceを開き、先頭recordをschemaへ保存する。欠落fileはskipする]
    /// 引数: [project_path: relative source pathの基準となるmanifest path]
    /// 戻り値: [(): schema取得後は値を返さない]
    /// 副作用: [対象file sourceのschemaを更新する]
    /// エラー: [ProjectError: CSVを開く、または読み込めない]
    /// ```
    pub fn capture_source_schemas(
        &mut self,
        project_path: impl AsRef<Path>,
    ) -> Result<(), ProjectError> {
        let project_path = project_path.as_ref();
        for source in &mut self.sources {
            if source.kind != SourceKind::File {
                continue;
            }
            let path = resolve_project_reference(project_path, &source.path);
            if !path.is_file() {
                continue;
            }
            let document =
                crate::process::CsvDocument::open(&path).map_err(|error| ProjectError::Read {
                    path: path.display().to_string(),
                    message: error.to_string(),
                })?;
            source.schema = document.rows().next().map(|row| row.to_vec());
        }
        Ok(())
    }

    // {
    //   責務: [
    //     from_value: JSON valueをmanifest schemaに従ってRowlyProjectへ変換する
    //   ]
    //   処理: [
    //     1: formatとversionを検証する
    //     2: source、scripts、historyを読み取る。recursiveはboolean値だけ採用し、それ以外はfalseにする
    //     3: project全体をvalidateして返す
    //   ]
    //   引数: [value: parse済みmanifest JSON]
    //   戻り値: [RowlyProject: schema検証済みproject model]
    //   エラー: [ProjectError: 必須field、型、version、source定義、またはmodel制約の違反]
    // }
    fn from_value(value: Value) -> Result<Self, ProjectError> {
        let object = value
            .as_object()
            .ok_or_else(|| ProjectError::Schema("project must be a JSON object".into()))?;
        let format = string_field(object, "format")?;
        if format != PROJECT_FORMAT {
            return Err(ProjectError::Schema(format!(
                "unsupported project format `{format}`"
            )));
        }
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| ProjectError::Schema("missing integer version".into()))?;
        if version != PROJECT_VERSION {
            return Err(ProjectError::Schema(format!(
                "unsupported project version {version}"
            )));
        }
        let name = string_field(object, "name")?.to_owned();
        let entries = object
            .get("sources")
            .and_then(Value::as_array)
            .ok_or_else(|| ProjectError::Schema("missing sources array".into()))?;
        let mut sources = Vec::with_capacity(entries.len());
        for entry in entries {
            let item = entry
                .as_object()
                .ok_or_else(|| ProjectError::Schema("each source must be an object".into()))?;
            let id = string_field(item, "id")?.to_owned();
            let kind = match string_field(item, "type")? {
                "file" => SourceKind::File,
                "directory" => SourceKind::Directory,
                other => {
                    return Err(ProjectError::Schema(format!(
                        "unsupported source type `{other}`"
                    )));
                }
            };
            let path = PathBuf::from(string_field(item, "path")?);
            let recursive = item
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let search_root = optional_path_field(item, "search_root")?;
            let schema = match item.get("schema") {
                None => None,
                Some(Value::Array(columns)) => Some(
                    columns
                        .iter()
                        .map(|column| {
                            column.as_str().map(str::to_owned).ok_or_else(|| {
                                ProjectError::Schema(format!(
                                    "source `{id}` schema entries must be strings"
                                ))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                Some(_) => {
                    return Err(ProjectError::Schema(format!(
                        "source `{id}` schema must be an array"
                    )));
                }
            };
            if kind == SourceKind::File && recursive {
                return Err(ProjectError::Schema(format!(
                    "file source `{id}` cannot be recursive"
                )));
            }
            sources.push(ProjectSource {
                id,
                kind,
                path,
                recursive,
                search_root,
                schema,
            });
        }
        let scripts = object
            .get("scripts")
            .and_then(Value::as_object)
            .ok_or_else(|| ProjectError::Schema("missing scripts object".into()))?;
        let scripts = ProjectScripts {
            init: PathBuf::from(string_field(scripts, "init")?),
            generated: PathBuf::from(string_field(scripts, "generated")?),
            user: PathBuf::from(string_field(scripts, "user")?),
            macros: PathBuf::from(string_field(scripts, "macros")?),
        };
        let history = PathBuf::from(string_field(object, "history")?);
        let project = Self {
            name,
            sources,
            scripts,
            history,
        };
        project.validate()?;
        Ok(project)
    }

    // {
    //   責務: [validate: project name、source id / path、script / history pathを検証する]
    //   処理: [空name、空または重複id、空path、file sourceのrecursive設定を拒否する]
    //   引数: []
    //   戻り値: [(): 全fieldが有効なとき値を返さない]
    //   エラー: [ProjectError: schema制約に違反するfieldがある]
    // }
    fn validate(&self) -> Result<(), ProjectError> {
        if self.name.trim().is_empty() {
            return Err(ProjectError::Schema("name must not be empty".into()));
        }
        let mut ids = HashSet::new();
        for source in &self.sources {
            if source.id.trim().is_empty() || !ids.insert(&source.id) {
                return Err(ProjectError::Schema(format!(
                    "source id `{}` is empty or duplicated",
                    source.id
                )));
            }
            if source.path.as_os_str().is_empty() {
                return Err(ProjectError::Schema(format!(
                    "source `{}` has an empty path",
                    source.id
                )));
            }
            if source.kind == SourceKind::File && source.recursive {
                return Err(ProjectError::Schema(format!(
                    "file source `{}` cannot be recursive",
                    source.id
                )));
            }
        }
        if self.scripts.init.as_os_str().is_empty()
            || self.scripts.generated.as_os_str().is_empty()
            || self.scripts.user.as_os_str().is_empty()
            || self.scripts.macros.as_os_str().is_empty()
            || self.history.as_os_str().is_empty()
        {
            return Err(ProjectError::Schema(
                "script and history paths must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// {
//   責務: [
//     write_project_file_atomic: bytesをtemporary fileへ書き、成功後に指定されたproject fileを置換する
//   ]
//   処理: [
//     1: 同名temporary fileの衝突を避けて作成し、bytesを書いてsyncする
//     2: temporary fileでdestinationを置換する
//     3: 失敗時はtemporary fileの削除を試みる
//   ]
//   引数: [path: 置換するmanifest path, bytes: 保存するmanifest bytes]
//   戻り値: [(): replacement成功時に値を返さない]
//   副作用: [temporary fileを作成し、成功時はdestinationを置換する]
//   エラー: [ProjectError: path、temporary file、write、sync、またはreplacementの失敗]
// }
pub(crate) fn write_project_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| ProjectError::Write {
        path: path.display().to_string(),
        message: "project path has no file name".into(),
    })?;
    let mut temporary = None;
    for attempt in 0..16u32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.{}.{}.tmp", std::process::id(), stamp, attempt));
        let candidate = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ProjectError::Write {
                    path: path.display().to_string(),
                    message: error.to_string(),
                });
            }
        }
    }
    let Some((temporary_path, mut file)) = temporary else {
        return Err(ProjectError::Write {
            path: path.display().to_string(),
            message: "could not allocate a temporary project manifest".into(),
        });
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary_path, path)?;
        Ok::<(), io::Error>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary_path);
        return Err(ProjectError::Write {
            path: path.display().to_string(),
            message: error.to_string(),
        });
    }
    Ok(())
}

// {
//   責務: [validate_project_value: 任意のJSON valueがRowlyProject schemaとして有効か検証する]
//   処理: [RowlyProject::from_valueによるparseとmodel validationを実行する]
//   引数: [value: 検証するmanifest JSON]
//   戻り値: [(): schema検証成功時に値を返さない]
//   エラー: [ProjectError: manifest schemaまたはproject model制約の違反]
// }
pub(crate) fn validate_project_value(value: Value) -> Result<(), ProjectError> {
    RowlyProject::from_value(value).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ResolvedSource: manifestのsource設定とmanifest位置から解決したpathをまとめる
/// ]
/// フィールド: [
/// id: stable source id
/// kind: source種別
/// path: 解決済みsource path
/// recursive: directory列挙設定
/// search_root: 解決済み再link探索境界
/// schema: 再link識別用header signature
/// ]
/// ```
pub struct ResolvedSource {
    /// ```text
    /// 責務: [id: manifest内のstable source id]
    /// ```
    pub id: String,
    /// ```text
    /// 責務: [kind: file / directory sourceの種別]
    /// ```
    pub kind: SourceKind,
    /// ```text
    /// 責務: [path: manifest基準で解決されたsource path]
    /// ```
    pub path: PathBuf,
    /// ```text
    /// 責務: [recursive: directory sourceを再帰列挙する設定]
    /// ```
    pub recursive: bool,
    /// ```text
    /// 責務: [search_root: manifest基準で解決された任意の再link探索境界]
    /// ```
    pub search_root: Option<PathBuf>,
    /// ```text
    /// 責務: [schema: 再link候補比較に使う任意のordered header]
    /// ```
    pub schema: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// SourceStatus: source pathの確認または再link探索の結果を表す
/// ]
/// 補足: [
/// Available: 宣言済みpathが該当種別のfile / directoryとして存在する
/// Missing: source pathまたは再link候補が見つからない
/// Relinked: 探索範囲から一意な候補へpathを再linkした
/// Ambiguous: 複数候補が一致し一意に決められない
/// ]
/// ```
pub enum SourceStatus {
    /// 宣言済みpathにsourceがある。
    Available,
    /// sourceまたは再link候補がない。
    Missing,
    /// 探索で見つけた一意候補へ再linkした。
    Relinked,
    /// 一致する候補が複数ある。
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// SourceResolution: sourceごとの確認状態と、利用可能な場合のresolved pathを返す
/// ]
/// フィールド: [
/// id: source id
/// status: path解決結果
/// path: Available / Relinked時のsource path。その他はNone
/// ]
/// ```
pub struct SourceResolution {
    /// ```text
    /// 責務: [id: 解決結果が対応するstable source id]
    /// ```
    pub id: String,
    /// ```text
    /// 責務: [status: path確認または再link探索の結果]
    /// ```
    pub status: SourceStatus,
    /// ```text
    /// 責務: [path: sourceが利用可能な場合のresolved path]
    /// ```
    pub path: Option<PathBuf>,
}

// {
//   責務: [source_path_exists: source種別に対応するfile / directoryがpathにあるか判定する]
//   処理: [SourceKind::Fileにはis_file、Directoryにはis_dirを使う]
//   引数: [kind: 期待するsource種別, path: 確認するpath]
//   戻り値: [bool: 種別に一致するentryがあればtrue]
// }
fn source_path_exists(kind: SourceKind, path: &Path) -> bool {
    match kind {
        SourceKind::File => path.is_file(),
        SourceKind::Directory => path.is_dir(),
    }
}

// {
//   責務: [find_matching_sources: 探索tree内からfilenameとheader schemaが一致するfileを集める]
//   処理: [subdirectoryを再帰探索し、symlinkを飛ばして一致CSVをmatchesへ追加する]
//   引数: [root: 探索directory, filename: 必要なfile名, schema: 必要なordered header, matches: 候補pathの出力先]
//   戻り値: [(): 候補追加後は値を返さない]
//   副作用: [一致したpathをmatchesへ追加する]
//   エラー: [ProjectError: directory entryを読み込めない]
// }
fn find_matching_sources(
    root: &Path,
    filename: Option<&std::ffi::OsStr>,
    schema: &[String],
    matches: &mut Vec<PathBuf>,
) -> Result<(), ProjectError> {
    let entries = fs::read_dir(root).map_err(|error| ProjectError::Read {
        path: root.display().to_string(),
        message: error.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| ProjectError::Read {
            path: root.display().to_string(),
            message: error.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|error| ProjectError::Read {
            path: entry.path().display().to_string(),
            message: error.to_string(),
        })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            find_matching_sources(&entry.path(), filename, schema, matches)?;
        } else if file_type.is_file()
            && filename.is_some_and(|name| entry.file_name() == name)
            && csv_header_matches(&entry.path(), schema)
        {
            matches.push(entry.path());
        }
    }
    Ok(())
}

// {
//   責務: [csv_header_matches: CSVの先頭recordが期待するordered schemaと一致するか判定する]
//   処理: [CSVを開きheaderを比較し、openまたはheader取得失敗は不一致として扱う]
//   引数: [path: 確認するCSV path, schema: 期待するheader]
//   戻り値: [bool: headerが完全一致した場合true]
// }
fn csv_header_matches(path: &Path, schema: &[String]) -> bool {
    crate::process::CsvDocument::open(path)
        .ok()
        .and_then(|document| {
            document.rows().next().map(|row| {
                row.iter().map(String::as_str).collect::<Vec<_>>()
                    == schema.iter().map(String::as_str).collect::<Vec<_>>()
            })
        })
        .unwrap_or(false)
}

/// ```text
/// 責務: [resolve_project_reference: relative referenceをmanifestのparent directory基準でpathへ解決する]
/// 処理: [absolute pathは維持し、relative pathはmanifest parentへjoinする]
/// 引数: [project_path: 基準となるmanifest path, reference: 解決するscript / history / source reference]
/// 戻り値: [PathBuf: absoluteまたはmanifest基準で解決したpath]
/// ```
pub fn resolve_project_reference(
    project_path: impl AsRef<Path>,
    reference: impl AsRef<Path>,
) -> PathBuf {
    let root = project_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let path = reference.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

// {
//   責務: [string_field: JSON objectから空でない必須string fieldを読む]
//   処理: [fieldをstringとして取得し、前後空白を除くと空の値を拒否する]
//   引数: [object: 読み取るJSON object, name: field名]
//   戻り値: [&str: object内のfield valueへの参照]
//   エラー: [ProjectError: fieldがない、stringでない、または空]
// }
fn string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a str, ProjectError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ProjectError::Schema(format!("missing or empty string `{name}`")))
}

// {
//   責務: [optional_path_field: JSON objectから任意の非空string path fieldを読む]
//   処理: [未指定ならNone、非空stringならPathBuf、その他の値はschema errorにする]
//   引数: [object: 読み取るJSON object, name: field名]
//   戻り値: [Option<PathBuf>: field未指定またはpath value]
//   エラー: [ProjectError: 指定valueが空stringまたはstring以外]
// }
fn optional_path_field(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<PathBuf>, ProjectError> {
    match object.get(name) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(PathBuf::from(value))),
        Some(_) => Err(ProjectError::Schema(format!(
            "`{name}` must be a non-empty string"
        ))),
    }
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [
/// ProjectError: project manifestのread、parse、schema検証、writeの失敗を表す
/// ]
/// 補足: [
/// Read: manifest、source CSV、またはsource discovery用directoryを読み込めない
/// Parse: manifest JSONをparseできない
/// Schema: manifestの形式またはfield制約に違反する
/// Write: manifestまたはgenerated project fileのatomic writeに失敗する
/// ]
/// ```
pub enum ProjectError {
    #[error("failed to read Rowly project `{path}`: {message}")]
    Read { path: String, message: String },
    #[error("failed to parse Rowly project `{path}`: {message}")]
    Parse { path: String, message: String },
    #[error("invalid Rowly project schema: {0}")]
    Schema(String),
    #[error("failed to write Rowly project `{path}`: {message}")]
    Write { path: String, message: String },
}
