//! ZIP-compatible packaging for ordinary `.rwprj` projects.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::project::{RowlyProject, SourceKind, resolve_project_reference};

/// Package a project folder into a standard ZIP-compatible `.rowlyx` archive.
/// Only project-relative references are collected; absolute external references
/// remain unchanged in the `.rwprj` and are never copied into the archive.
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

    let output = File::create(archive_file).map_err(|error| io_error(archive_file, error))?;
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
    Ok(())
}

/// An opened and validated `.rowlyx` archive.
#[derive(Debug, Clone)]
pub struct RowlyxArchive {
    path: PathBuf,
    project_entry: PathBuf,
}

impl RowlyxArchive {
    /// Open a ZIP container, reject unsafe or duplicate entries, and locate its
    /// single project manifest without extracting any files.
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

    /// Extract the archive into a destination directory and return the project
    /// manifest path inside that directory.
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

fn validate_manifest_value(value: serde_json::Value) -> Result<(), RowlyxError> {
    // Keep project schema validation in one place by writing no intermediate file.
    crate::project::validate_project_value(value)
        .map_err(|error| RowlyxError::Manifest(error.to_string()))
}

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

fn relative_entry(root: &Path, path: &Path) -> Result<PathBuf, RowlyxError> {
    lexical_relative(root, path)
        .ok_or_else(|| RowlyxError::InvalidReference(path.display().to_string()))
}

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

fn zip_name(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn io_error(path: &Path, error: io::Error) -> RowlyxError {
    RowlyxError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
fn zip_error(error: zip::result::ZipError) -> RowlyxError {
    RowlyxError::Archive(error.to_string())
}

#[derive(Debug, Error)]
pub enum RowlyxError {
    #[error("project error: {0}")]
    Project(#[from] crate::project::ProjectError),
    #[error("failed to access `{path}`: {message}")]
    Io { path: String, message: String },
    #[error("invalid or damaged rowlyx archive: {0}")]
    Archive(String),
    #[error("unsafe archive entry path `{0}`")]
    UnsafeEntry(String),
    #[error("archive contains duplicate entry `{0}`")]
    DuplicateEntry(String),
    #[error("archive must contain exactly one `.rwprj` manifest, found {0}")]
    ProjectDefinitionCount(usize),
    #[error("invalid project manifest inside archive: {0}")]
    Manifest(String),
    #[error("project reference is invalid or escapes its root: {0}")]
    InvalidReference(String),
    #[error("project path is a symbolic link and cannot be packaged safely: {0}")]
    UnsupportedLink(String),
    #[error("extract destination is not empty: {0}")]
    DestinationNotEmpty(String),
}
