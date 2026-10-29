use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use crate::project::{RowlyProject, SourceKind, resolve_project_reference};

use super::{ProjectInitConfig, ProjectInitError};

pub(super) fn project_script_path(
    manifest_path: &Path,
    reference: &Path,
) -> Result<PathBuf, ProjectInitError> {
    if reference.is_absolute() || reference.as_os_str().is_empty() {
        return Err(ProjectInitError::UnsafeScriptPath(
            reference.display().to_string(),
        ));
    }
    let mut relative = PathBuf::new();
    for component in reference.components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            _ => {
                return Err(ProjectInitError::UnsafeScriptPath(
                    reference.display().to_string(),
                ));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(ProjectInitError::UnsafeScriptPath(
            reference.display().to_string(),
        ));
    }
    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(root).map_err(|error| script_read_error(root, error))?;
    let path = root.join(&relative);
    let mut current = root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(ProjectInitError::UnsafeScriptPath(
                reference.display().to_string(),
            ));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProjectInitError::UnsafeScriptPath(
                    reference.display().to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(script_read_error(&current, error)),
        }
    }
    if !path.starts_with(&root) {
        return Err(ProjectInitError::UnsafeScriptPath(
            reference.display().to_string(),
        ));
    }
    Ok(path)
}

pub(super) fn ensure_distinct_paths(
    init: &Path,
    generated: &Path,
    user: &Path,
) -> Result<(), ProjectInitError> {
    if same_path(init, generated) || same_path(init, user) || same_path(generated, user) {
        return Err(ProjectInitError::ScriptPathCollision);
    }
    Ok(())
}

pub(super) fn ensure_generated_path_does_not_alias_project_data(
    project: &RowlyProject,
    manifest_path: &Path,
    init_path: &Path,
    generated_path: &Path,
    user_path: &Path,
) -> Result<(), ProjectInitError> {
    let generated = normalized_path(generated_path);
    let mut protected = vec![
        manifest_path.to_path_buf(),
        init_path.to_path_buf(),
        user_path.to_path_buf(),
        resolve_project_reference(manifest_path, &project.scripts.macros),
        resolve_project_reference(manifest_path, &project.history),
    ];
    let mut protected_directories = vec![
        resolve_project_reference(manifest_path, &project.scripts.macros),
        resolve_project_reference(manifest_path, &project.history),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .collect::<Vec<_>>();
    for source in &project.sources {
        let path = resolve_project_reference(manifest_path, &source.path);
        if source.kind == SourceKind::Directory {
            protected_directories.push(path.clone());
        }
        protected.push(path);
    }
    if protected
        .iter()
        .any(|path| same_path(&generated, &normalized_path(path)))
        || protected_directories
            .iter()
            .any(|directory| generated.starts_with(normalized_path(directory)))
    {
        return Err(ProjectInitError::GeneratedPathAliasesProjectData(
            generated_path.display().to_string(),
        ));
    }
    Ok(())
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let left = normalized_path(left);
    let right = normalized_path(right);
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(super) fn normalized_path(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    });
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(name) => normalized.push(name),
        }
    }
    normalized
}

pub(super) fn validate_regular_file_if_present(path: &Path) -> Result<(), ProjectInitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(ProjectInitError::NotRegularScriptFile(
            path.display().to_string(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(script_read_error(path, error)),
    }
}

pub(super) fn read_optional_config(path: &Path) -> Result<ProjectInitConfig, ProjectInitError> {
    match fs::read_to_string(path) {
        Ok(source) => ProjectInitConfig::parse(&source, path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ProjectInitConfig::default()),
        Err(error) => Err(script_read_error(path, error)),
    }
}

pub(super) fn create_parent(path: &Path) -> Result<(), ProjectInitError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| ProjectInitError::ScriptWrite {
        path: parent.display().to_string(),
        message: error.to_string(),
    })
}

pub(super) fn create_if_missing(path: &Path, contents: &str) -> Result<(), ProjectInitError> {
    create_parent(path)?;
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return validate_regular_file_if_present(path);
        }
        Err(error) => {
            return Err(ProjectInitError::ScriptWrite {
                path: path.display().to_string(),
                message: error.to_string(),
            });
        }
    };
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| ProjectInitError::ScriptWrite {
            path: path.display().to_string(),
            message: error.to_string(),
        })
}

pub(super) fn script_read_error(path: &Path, error: io::Error) -> ProjectInitError {
    ProjectInitError::ScriptRead {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
