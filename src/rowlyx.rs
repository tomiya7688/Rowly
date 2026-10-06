//! ZIP-compatible packaging for ordinary `.rwprj` projects.

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::data::replace_file;
use crate::project::{RowlyProject, SourceKind, resolve_project_reference};

/// ```text
/// 責務: [pack_project: project manifestと内部参照fileをZIP互換の.rowlyxへpackする]
/// 処理: [manifest基準の内部referenceを収集し、一時archiveを検証してから出力先へ置換する]
/// 引数: [project_file: pack対象の.rwprj, archive_file: 生成する.rowlyx path]
/// 戻り値: [(): archive保存成功時に値を返さない]
/// 副作用: [一時archiveを作成し、成功時にarchive_fileを置換する]
/// エラー: [RowlyxError: manifest、reference、file IO、archive作成または検証の失敗]
/// 補足: [absolute referenceとcanonical pathがproject root外へ解決されるreferenceはpackしない]
/// ```
pub fn pack_project(
    project_file: impl AsRef<Path>,
    archive_file: impl AsRef<Path>,
) -> Result<(), RowlyxError> {
    let project_file = project_file.as_ref();
    let archive_file = archive_file.as_ref();
    let project = RowlyProject::load(project_file)?;
    let root = project_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(root).map_err(|error| io_error(root, error))?;
    let project_file =
        fs::canonicalize(project_file).map_err(|error| io_error(project_file, error))?;
    let manifest_entry = relative_entry(&root, &project_file)?;

    let mut entries = BTreeSet::new();
    entries.insert(manifest_entry);
    let mut references = Vec::new();
    references.extend(project.sources.iter().map(|source| {
        let resolved = resolve_project_reference(&project_file, &source.path);
        (
            resolved,
            source.kind,
            source.recursive,
            source.path.is_absolute(),
        )
    }));
    references.extend([
        (
            resolve_project_reference(&project_file, &project.scripts.init),
            SourceKind::File,
            false,
            project.scripts.init.is_absolute(),
        ),
        (
            resolve_project_reference(&project_file, &project.scripts.generated),
            SourceKind::File,
            false,
            project.scripts.generated.is_absolute(),
        ),
        (
            resolve_project_reference(&project_file, &project.scripts.user),
            SourceKind::File,
            false,
            project.scripts.user.is_absolute(),
        ),
        (
            resolve_project_reference(&project_file, &project.scripts.macros),
            SourceKind::Directory,
            true,
            project.scripts.macros.is_absolute(),
        ),
        (
            resolve_project_reference(&project_file, &project.history),
            SourceKind::Directory,
            true,
            project.history.is_absolute(),
        ),
    ]);

    let archive_target = archive_target_path(archive_file)?;
    if archive_target == project_file {
        return Err(RowlyxError::InvalidReference(
            "archive output would overwrite the project manifest".into(),
        ));
    }

    for (path, kind, recursive, external) in references {
        // Absolute references are explicitly external and stay out of the pack.
        if external {
            continue;
        }
        let relative = match lexical_relative(&root, &path) {
            Some(relative) => relative,
            None => continue,
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        if !path.exists() {
            continue;
        }
        let canonical = fs::canonicalize(&path).map_err(|error| io_error(&path, error))?;
        if !canonical.starts_with(&root) {
            return Err(RowlyxError::InvalidReference(format!(
                "reference resolves outside project root: {}",
                path.display()
            )));
        }
        if canonical == archive_target {
            return Err(RowlyxError::InvalidReference(format!(
                "archive output would overwrite project data `{}`",
                path.display()
            )));
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RowlyxError::UnsupportedLink(path.display().to_string()));
            }
            Ok(_) => {}
            Err(error) => return Err(io_error(&path, error)),
        }
        if path.is_dir() {
            if kind == SourceKind::File {
                return Err(RowlyxError::InvalidReference(format!(
                    "file reference is a directory: {}",
                    path.display()
                )));
            }
            collect_directory(&root, &path, &mut entries, recursive)?;
        } else if path.is_file() {
            if kind == SourceKind::Directory {
                return Err(RowlyxError::InvalidReference(format!(
                    "directory reference is a file: {}",
                    path.display()
                )));
            }
            entries.insert(relative.clone());
        } else {
            return Err(RowlyxError::InvalidReference(path.display().to_string()));
        }
        // A declared directory itself is represented even when it is empty.
        if kind == SourceKind::Directory {
            entries.insert(relative);
        }
    }

    let temporary = create_temporary_archive(archive_file)?;
    let result = (|| {
        let output = File::create(&temporary).map_err(|error| io_error(&temporary, error))?;
        let mut zip = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for relative in entries {
            let source = root.join(&relative);
            let entry = zip_name(&relative);
            if source.is_dir() {
                zip.add_directory(format!("{entry}/"), options)
                    .map_err(zip_error)?;
            } else {
                zip.start_file(entry, options).map_err(zip_error)?;
                let mut input = File::open(&source).map_err(|error| io_error(&source, error))?;
                io::copy(&mut input, &mut zip).map_err(|error| io_error(&source, error))?;
            }
        }
        zip.finish().map_err(zip_error)?;
        RowlyxArchive::open(&temporary)?;
        replace_file(&temporary, archive_file).map_err(|error| io_error(archive_file, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

// {
//   責務: [create_temporary_archive: archive出力先と同じdirectoryに一意なtemporary pathを確保する]
//   処理: [process id、時刻、連番を使った候補を最大16回create_newで作成する]
//   引数: [path: 最終archiveのpath]
//   戻り値: [PathBuf: 確保したtemporary archive path]
//   副作用: [空のtemporary fileを作成する]
//   エラー: [RowlyxError: 親directory、file name、またはtemporary fileの作成失敗]
// }
fn create_temporary_archive(path: &Path) -> Result<PathBuf, RowlyxError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| RowlyxError::InvalidReference(path.display().to_string()))?;
    for attempt in 0..16u32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.{}.{}.tmp", std::process::id(), stamp, attempt));
        let temporary = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                drop(file);
                return Ok(temporary);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(&temporary, error)),
        }
    }
    Err(io_error(
        path,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary rowlyx archive",
        ),
    ))
}

/// ```text
/// 責務: [
/// RowlyxArchive: 安全性とproject manifest schemaを検証した.rowlyx archiveへの参照
/// ]
/// フィールド: [
/// path: 検証したarchive file path
/// project_entry: archive root直下で唯一の.rwprj manifest entry
/// ]
/// 補足: [openはarchiveを展開せず、extract_toで展開する]
/// ```
#[derive(Debug, Clone)]
pub struct RowlyxArchive {
    path: PathBuf,
    project_entry: PathBuf,
}

impl RowlyxArchive {
    /// ```text
    /// 責務: [open: ZIP archiveのentryと唯一のroot-level project manifestを検証する]
    /// 処理: [unsafe path、重複entry、symlink、manifest数、manifest JSON/schemaを検査する]
    /// 引数: [path: 開く.rowlyx archive path]
    /// 戻り値: [Self: 検証済みarchiveとmanifest entryへの参照]
    /// エラー: [RowlyxError: file read、ZIP形式、entry安全性、manifest数またはschemaの失敗]
    /// 補足: [fileは展開せず、manifestの検証だけをメモリ上で行う]
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RowlyxError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|error| io_error(&path, error))?;
        let mut archive = ZipArchive::new(file).map_err(zip_error)?;
        let mut names = BTreeSet::new();
        let mut manifests = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(zip_error)?;
            let enclosed = entry
                .enclosed_name()
                .ok_or_else(|| RowlyxError::UnsafeEntry(entry.name().to_owned()))?;
            if !safe_relative_path(&enclosed) {
                return Err(RowlyxError::UnsafeEntry(entry.name().to_owned()));
            }
            let name = zip_name(&enclosed);
            if !names.insert(name.clone()) {
                return Err(RowlyxError::DuplicateEntry(name));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(RowlyxError::UnsupportedLink(entry.name().to_owned()));
            }
            if !entry.is_dir()
                && enclosed
                    .parent()
                    .is_none_or(|parent| parent.as_os_str().is_empty())
                && enclosed
                    .extension()
                    .is_some_and(|extension| extension == "rwprj")
            {
                manifests.push(enclosed);
            }
        }
        if manifests.len() != 1 {
            return Err(RowlyxError::ProjectDefinitionCount(manifests.len()));
        }
        // Validate the manifest JSON/schema now, before extraction mutates disk.
        let manifest_name = zip_name(&manifests[0]);
        let mut archive =
            ZipArchive::new(File::open(&path).map_err(|error| io_error(&path, error))?)
                .map_err(zip_error)?;
        let mut manifest = archive.by_name(&manifest_name).map_err(zip_error)?;
        let mut bytes = Vec::new();
        manifest
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(&path, error))?;
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|error| RowlyxError::Manifest(error.to_string()))?;
        // Reuse the canonical project schema validation through a temporary file-free parser.
        validate_manifest_value(value)?;
        Ok(Self {
            path,
            project_entry: manifests.remove(0),
        })
    }

    /// ```text
    /// 責務: [extract_to: archive内容を空のdestinationへ展開しmanifest pathを返す]
    /// 処理: [destinationを作成し、entry pathとsymlinkを検査しながらfile / directoryを書き出す]
    /// 引数: [destination: 展開先directory。既存の場合は空であること]
    /// 戻り値: [PathBuf: 展開先rootを基準にしたproject manifest path]
    /// 副作用: [destination directoryとarchive entryをdisk上に作成する]
    /// エラー: [RowlyxError: destination不正、unsafe entry、symlink、archive readまたはfile writeの失敗]
    /// 補足: [展開途中で失敗した場合、destination内に作成済みentryが残る]
    /// ```
    pub fn extract_to(&self, destination: impl AsRef<Path>) -> Result<PathBuf, RowlyxError> {
        let destination = destination.as_ref();
        fs::create_dir_all(destination).map_err(|error| io_error(destination, error))?;
        if fs::read_dir(destination)
            .map_err(|error| io_error(destination, error))?
            .next()
            .is_some()
        {
            return Err(RowlyxError::DestinationNotEmpty(
                destination.display().to_string(),
            ));
        }
        let root = fs::canonicalize(destination).map_err(|error| io_error(destination, error))?;
        let mut archive =
            ZipArchive::new(File::open(&self.path).map_err(|error| io_error(&self.path, error))?)
                .map_err(zip_error)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(zip_error)?;
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(RowlyxError::UnsupportedLink(entry.name().to_owned()));
            }
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| RowlyxError::UnsafeEntry(entry.name().to_owned()))?
                .to_path_buf();
            if !safe_relative_path(&relative) {
                return Err(RowlyxError::UnsafeEntry(entry.name().to_owned()));
            }
            let target = root.join(&relative);
            if !target.starts_with(&root) {
                return Err(RowlyxError::UnsafeEntry(entry.name().to_owned()));
            }
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
                }
                let mut output = File::create(&target).map_err(|error| io_error(&target, error))?;
                io::copy(&mut entry, &mut output).map_err(|error| io_error(&target, error))?;
                output.flush().map_err(|error| io_error(&target, error))?;
            }
        }
        Ok(root.join(&self.project_entry))
    }
}

// {
//   責務: [validate_manifest_value: archive内のJSON valueをcanonical project schemaで検証する]
//   処理: [project moduleの共通validatorへ委譲し、失敗をManifest errorへ変換する]
//   引数: [value: parse済みmanifest JSON]
//   戻り値: [(): schema検証成功時に値を返さない]
//   エラー: [RowlyxError: project manifest schemaの不正]
// }
fn validate_manifest_value(value: serde_json::Value) -> Result<(), RowlyxError> {
    // Keep project schema validation in one place by writing no intermediate file.
    crate::project::validate_project_value(value)
        .map_err(|error| RowlyxError::Manifest(error.to_string()))
}

// {
//   責務: [collect_directory: project root内directoryのarchive entryを集める]
//   処理: [直下fileを追加し、recursive時は子directoryを再帰収集する。symlinkは拒否する]
//   引数: [root: project root, directory: 収集対象, entries: archive entry集合, recursive: 子directoryも走査するか]
//   戻り値: [(): entry収集成功時に値を返さない]
//   副作用: [entriesへrelative pathを追加する]
//   エラー: [RowlyxError: directory走査、relative path解決またはsymlink検査の失敗]
// }
fn collect_directory(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeSet<PathBuf>,
    recursive: bool,
) -> Result<(), RowlyxError> {
    let relative = lexical_relative(root, directory)
        .ok_or_else(|| RowlyxError::InvalidReference(directory.display().to_string()))?;
    entries.insert(relative);
    let children = fs::read_dir(directory).map_err(|error| io_error(directory, error))?;
    for child in children {
        let child = child.map_err(|error| io_error(directory, error))?;
        let path = child.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(RowlyxError::UnsupportedLink(path.display().to_string()));
        }
        if metadata.is_dir() {
            if recursive {
                collect_directory(root, &path, entries, true)?;
            }
        } else if metadata.is_file() {
            entries.insert(
                lexical_relative(root, &path)
                    .ok_or_else(|| RowlyxError::InvalidReference(path.display().to_string()))?,
            );
        }
    }
    Ok(())
}

// {
//   責務: [lexical_relative: root配下のpathを構成要素だけでrelative pathへ変換する]
//   処理: [root外への遷移を拒否し、CurDirを除去してParentDirをlexically解決する]
//   引数: [root: relative化の基準directory, path: 判定するpath]
//   戻り値: [Option<PathBuf>: root配下ならrelative path、それ以外はNone]
//   補足: [filesystemのcanonicalizeやentry存在確認は行わない]
// }
fn lexical_relative(root: &Path, path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut clean = PathBuf::new();
    for component in absolute.strip_prefix(root).ok()?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !clean.pop() {
                    return None;
                }
            }
            Component::Normal(part) => clean.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(clean)
}

// {
//   責務: [relative_entry: root配下のpathをarchive entry用relative pathへ変換する]
//   引数: [root: archive root directory, path: archiveへ格納するpath]
//   戻り値: [PathBuf: relative archive entry path]
//   エラー: [RowlyxError: pathがroot配下にない]
// }
fn relative_entry(root: &Path, path: &Path) -> Result<PathBuf, RowlyxError> {
    lexical_relative(root, path)
        .ok_or_else(|| RowlyxError::InvalidReference(path.display().to_string()))
}

// {
//   責務: [archive_target_path: archive出力先の比較用pathを構築する]
//   処理: [parentをcanonicalizeし、既存targetはtarget自身もcanonicalizeする]
//   引数: [path: archive出力先]
//   戻り値: [PathBuf: target比較に使うpath]
//   エラー: [RowlyxError: parentまたは既存targetのcanonicalize失敗]
// }
fn archive_target_path(path: &Path) -> Result<PathBuf, RowlyxError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .ok_or_else(|| RowlyxError::InvalidReference(path.display().to_string()))?;
    let parent = fs::canonicalize(parent).map_err(|error| io_error(parent, error))?;
    let target = parent.join(filename);
    if target.exists() {
        fs::canonicalize(&target).map_err(|error| io_error(&target, error))
    } else {
        Ok(target)
    }
}

// {
//   責務: [safe_relative_path: archive entry pathがrelativeな通常pathだけで構成されるか判定する]
//   処理: [empty、backslash、rooted / drive形式、componentsで検出されるCurDir / ParentDir / Prefixを拒否する]
//   引数: [path: 検証するentry path]
//   戻り値: [bool: 安全なrelative pathならtrue]
//   補足: [Pathがembedded / trailing `.` componentを正規化する場合、その表記は受理される]
// }
fn safe_relative_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    !path.as_os_str().is_empty()
        && !text.contains('\\')
        && !text.starts_with('/')
        && !text.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

// {
//   責務: [zip_name: platform pathをZIP標準のslash区切りentry名へ変換する]
//   引数: [path: ZIP entryにするpath]
//   戻り値: [String: slash区切りのentry名]
// }
fn zip_name(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// {
//   責務: [io_error: pathを含むIO errorをRowlyxErrorへ変換する]
//   引数: [path: 失敗したpath, error: 発生したIO error]
//   戻り値: [RowlyxError: pathとerror messageを保持するIo variant]
// }
fn io_error(path: &Path, error: io::Error) -> RowlyxError {
    RowlyxError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
// {
//   責務: [zip_error: ZIP library errorをRowlyxErrorへ変換する]
//   引数: [error: 発生したZIP error]
//   戻り値: [RowlyxError: error messageを保持するArchive variant]
// }
fn zip_error(error: zip::result::ZipError) -> RowlyxError {
    RowlyxError::Archive(error.to_string())
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [RowlyxError: project package / archive操作で呼出元へ返す失敗]
/// 補足: [Project、IO、ZIP library error、entry安全性、manifest検証、参照、symlink、展開先の失敗を区別する]
/// ```
pub enum RowlyxError {
    /// project manifestの読込・検証失敗。
    #[error("project error: {0}")]
    Project(#[from] crate::project::ProjectError),
    /// pathに対するfile / directory操作の失敗。
    #[error("failed to access `{path}`: {message}")]
    Io { path: String, message: String },
    /// ZIP library operationがZipErrorを返した失敗。
    #[error("invalid or damaged rowlyx archive: {0}")]
    Archive(String),
    /// archive entry pathがrelative安全条件を満たさない。
    #[error("unsafe archive entry path `{0}`")]
    UnsafeEntry(String),
    /// archive内で同じentry名が重複している。
    #[error("archive contains duplicate entry `{0}`")]
    DuplicateEntry(String),
    /// archive root直下の非directory `.rwprj` manifest数が1件ではない。
    #[error("archive must contain exactly one `.rwprj` manifest, found {0}")]
    ProjectDefinitionCount(usize),
    /// archive内manifestのJSONまたはschemaが不正。
    #[error("invalid project manifest inside archive: {0}")]
    Manifest(String),
    /// archive出力pathが無効、project dataと衝突、またはreferenceの種別が実体と合わない。
    #[error("project reference is invalid or escapes its root: {0}")]
    InvalidReference(String),
    /// symbolic linkを安全にpack / extractできない。
    #[error("project path is a symbolic link and cannot be packaged safely: {0}")]
    UnsupportedLink(String),
    /// extract destinationが空ではない。
    #[error("extract destination is not empty: {0}")]
    DestinationNotEmpty(String),
}

#[cfg(all(test, unix))]
mod tests {
    use std::{os::unix::fs::symlink, path::PathBuf};

    use tempfile::tempdir;

    use crate::project::{ProjectScripts, ProjectSource};

    use super::*;

    // {
    //   責務: [
    //     pack_rejects_reference_through_external_directory_symlink: project外を指すdirectory symlink経由のreferenceをpackしない
    //   ]
    //   処理: [
    //     1: project外にCSVを作りproject内directory symlinkから参照する
    //     2: referenceを含むprojectのpackを試す
    //     3: project root外へ解決されるreferenceを拒否しarchiveを作らないことを確認する
    //   ]
    //   引数: []
    //   戻り値: [(): assertion成功時に値を返さない]
    // }
    #[test]
    fn pack_rejects_reference_through_external_directory_symlink() {
        let directory = tempdir().unwrap();
        let project_root = directory.path().join("project");
        let external_directory = directory.path().join("external");
        fs::create_dir(&project_root).unwrap();
        fs::create_dir(&external_directory).unwrap();
        fs::write(external_directory.join("records.csv"), "name\nprivate\n").unwrap();
        symlink(&external_directory, project_root.join("linked")).unwrap();

        let project_file = project_root.join("project.rwprj");
        let project = RowlyProject {
            name: "symlink test".to_owned(),
            sources: vec![ProjectSource {
                id: "records".to_owned(),
                kind: SourceKind::File,
                path: PathBuf::from("linked/records.csv"),
                recursive: false,
                search_root: None,
                schema: None,
            }],
            scripts: ProjectScripts {
                init: PathBuf::from("scripts/init.rly"),
                generated: PathBuf::from("scripts/generated.rly"),
                user: PathBuf::from("scripts/user.rly"),
                macros: PathBuf::from("scripts/macros"),
            },
            history: PathBuf::from(".rowly/history"),
        };
        project.save(&project_file).unwrap();
        let archive_file = directory.path().join("project.rowlyx");

        let error = pack_project(&project_file, &archive_file).unwrap_err();

        assert!(matches!(
            error,
            RowlyxError::InvalidReference(message) if message.contains("outside project root")
        ));
        assert!(!archive_file.exists());
    }
}
