//! Versioned `.rwprj` project manifests. The manifest stores references only;
//! CSV contents remain in their source files.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use crate::process::{ColumnType, CsvDocument};
use serde_json::{Value, json};
use thiserror::Error;

pub const PROJECT_FORMAT: &str = "rowly-project";
pub const PROJECT_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowlyProject {
    pub name: String,
    pub sources: Vec<ProjectSource>,
    pub scripts: ProjectScripts,
    pub history: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSource {
    pub id: String,
    pub kind: SourceKind,
    pub path: PathBuf,
    pub recursive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScripts {
    pub init: PathBuf,
    pub generated: PathBuf,
    pub user: PathBuf,
    pub macros: PathBuf,
}

impl RowlyProject {
    /// Load and validate a project manifest. Relative references are retained as
    /// written and are resolved against `project_path` when requested.
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

    /// Save this manifest as pretty JSON. No source data is copied or embedded.
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
        fs::write(path, bytes).map_err(|error| ProjectError::Write {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    }

    /// Resolve only declared source paths. This does not enumerate directories
    /// or verify that referenced files exist.
    pub fn resolve_sources(&self, project_path: impl AsRef<Path>) -> Vec<ResolvedSource> {
        self.sources
            .iter()
            .map(|source| ResolvedSource {
                id: source.id.clone(),
                kind: source.kind,
                path: resolve_project_reference(project_path.as_ref(), &source.path),
                recursive: source.recursive,
            })
            .collect()
    }

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

impl ProjectScripts {
    /// Conventional script locations used by a new Rowly project.
    pub fn standard() -> Self {
        Self {
            init: PathBuf::from("data/scripts/rowlydsl/init.rly"),
            generated: PathBuf::from("data/scripts/rowlydsl/init/generated.rly"),
            user: PathBuf::from("data/scripts/rowlydsl/init/user.rly"),
            macros: PathBuf::from("data/scripts/rowlydsl/macros"),
        }
    }

    /// Create the script layout without replacing any existing file. The init
    /// file is a two-entry import manifest; user code is never run by this API.
    pub fn initialize_layout(&self, project_file: impl AsRef<Path>) -> Result<(), ProjectError> {
        let root = project_root(project_file.as_ref());
        self.validate_script_layout()?;
        fs::create_dir_all(&root).map_err(|error| path_error(&root, error))?;
        let init_path = internal_script_path(&root, &self.init)?;
        let generated_path = internal_script_path(&root, &self.generated)?;
        let user_path = internal_script_path(&root, &self.user)?;
        let macros_path = internal_script_path(&root, &self.macros)?;

        for path in [&init_path, &generated_path, &user_path] {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| path_error(parent, error))?;
            }
        }
        fs::create_dir_all(&macros_path).map_err(|error| path_error(&macros_path, error))?;
        let imports = self.expected_init_contents()?;
        create_if_missing(&init_path, imports.as_bytes())?;
        create_if_missing(&generated_path, b"")?;
        create_if_missing(&user_path, b"")?;
        Ok(())
    }

    /// Persist column-type declarations as a restricted generated init DSL.
    /// This operation replaces only `generated.rly`; `user.rly` is untouched.
    pub fn save_generated_column_types(
        &self,
        project_file: impl AsRef<Path>,
        document: &CsvDocument,
    ) -> Result<(), ProjectError> {
        self.initialize_layout(&project_file)?;
        let root = project_root(project_file.as_ref());
        let init_path = internal_script_path(&root, &self.init)?;
        self.validate_init_manifest(&init_path)?;
        let generated_path = internal_script_path(&root, &self.generated)?;
        let mut content = String::new();
        for (header, column_type) in document.column_type_declarations() {
            let args = serde_json::to_string(&[header, column_type.as_metadata_str()])
                .map_err(|error| ProjectError::Schema(error.to_string()))?;
            content.push_str("SET_COLUMN_TYPE(");
            content.push_str(&args);
            content.push_str(")\n");
        }
        fs::write(&generated_path, content.as_bytes())
            .map_err(|error| path_error(&generated_path, error))
    }

    /// Restore only the supported generated column-type directives. Arbitrary
    /// DSL and user macros are not parsed or executed during project loading.
    pub fn restore_generated_column_types(
        &self,
        project_file: impl AsRef<Path>,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectError> {
        self.initialize_layout(&project_file)?;
        let root = project_root(project_file.as_ref());
        let init_path = internal_script_path(&root, &self.init)?;
        self.validate_init_manifest(&init_path)?;
        let generated_path = internal_script_path(&root, &self.generated)?;
        let text = fs::read_to_string(&generated_path)
            .map_err(|error| path_error(&generated_path, error))?;
        let mut declarations = Vec::new();
        let mut headers = HashSet::new();
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let args = line
                .trim()
                .strip_prefix("SET_COLUMN_TYPE(")
                .and_then(|value| value.strip_suffix(')'))
                .ok_or_else(|| {
                    ProjectError::Schema(format!(
                        "unsupported generated init directive on line {}",
                        line_number + 1
                    ))
                })?;
            let values: Vec<String> = serde_json::from_str(args).map_err(|error| {
                ProjectError::Schema(format!(
                    "invalid generated init line {}: {error}",
                    line_number + 1
                ))
            })?;
            if values.len() != 2 {
                return Err(ProjectError::Schema(format!(
                    "SET_COLUMN_TYPE on line {} requires a header and type",
                    line_number + 1
                )));
            }
            let column_type = ColumnType::from_metadata_str(&values[1]).ok_or_else(|| {
                ProjectError::Schema(format!("unsupported column type `{}`", values[1]))
            })?;
            if !headers.insert(values[0].clone()) {
                return Err(ProjectError::Schema(format!(
                    "generated init declares column `{}` more than once",
                    values[0]
                )));
            }
            document
                .column_type_declaration_by_header(&values[0])
                .map_err(|error| {
                    ProjectError::Schema(format!(
                        "generated column `{}` cannot be restored: {error}",
                        values[0]
                    ))
                })?;
            declarations.push((values[0].clone(), column_type));
        }
        document.clear_column_type_declarations();
        for (header, column_type) in declarations {
            document
                .set_column_type_declaration_by_header(&header, column_type)
                .map_err(|error| ProjectError::Schema(error.to_string()))?;
        }
        Ok(())
    }

    fn expected_init_contents(&self) -> Result<String, ProjectError> {
        self.validate_script_layout()?;
        validate_project_relative(&self.generated)?;
        validate_project_relative(&self.user)?;
        let generated = serde_json::to_string(&self.generated.to_string_lossy())
            .map_err(|error| ProjectError::Schema(error.to_string()))?;
        let user = serde_json::to_string(&self.user.to_string_lossy())
            .map_err(|error| ProjectError::Schema(error.to_string()))?;
        Ok(format!("IMPORT({generated})\nIMPORT({user})\n"))
    }

    fn validate_init_manifest(&self, init_path: &Path) -> Result<(), ProjectError> {
        let current =
            fs::read_to_string(init_path).map_err(|error| path_error(init_path, error))?;
        if current != self.expected_init_contents()? {
            return Err(ProjectError::Schema(format!(
                "project init manifest `{}` contains unsupported content",
                init_path.display()
            )));
        }
        Ok(())
    }

    fn validate_script_layout(&self) -> Result<(), ProjectError> {
        let paths = [&self.init, &self.generated, &self.user, &self.macros];
        let mut unique = HashSet::new();
        for path in paths {
            validate_project_relative(path)?;
            let normalized = normalize_project_path(path).ok_or_else(|| {
                ProjectError::Schema(format!("invalid project script path: {}", path.display()))
            })?;
            if !unique.insert(normalized) {
                return Err(ProjectError::Schema(
                    "project script paths must be distinct".into(),
                ));
            }
        }
        let macros = normalize_project_path(&self.macros).unwrap();
        for file in [&self.init, &self.generated, &self.user] {
            let file = normalize_project_path(file).unwrap();
            if file.starts_with(&macros) {
                return Err(ProjectError::Schema(
                    "macros directory must not contain init files".into(),
                ));
            }
        }
        Ok(())
    }
}

fn project_root(project_file: &Path) -> PathBuf {
    project_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn validate_project_relative(path: &Path) -> Result<(), ProjectError> {
    if path.is_absolute() {
        return Err(ProjectError::Schema(format!(
            "project script path must be relative: {}",
            path.display()
        )));
    }
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => {
                return Err(ProjectError::Schema(format!(
                    "project script path escapes the project: {}",
                    path.display()
                )));
            }
        }
    }
    if depth == 0 {
        return Err(ProjectError::Schema(
            "project script path must not be empty".into(),
        ));
    }
    Ok(())
}

fn normalize_project_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir if normalized.pop() => {}
            _ => return None,
        }
    }
    Some(normalized)
}

fn internal_script_path(root: &Path, relative: &Path) -> Result<PathBuf, ProjectError> {
    validate_project_relative(relative)?;
    let canonical_root = fs::canonicalize(root).map_err(|error| path_error(root, error))?;
    let target = root.join(relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            if let Ok(metadata) = fs::symlink_metadata(&current) {
                if metadata.file_type().is_symlink() {
                    return Err(ProjectError::Schema(format!(
                        "project script path contains a symbolic link: {}",
                        current.display()
                    )));
                }
            }
        }
    }
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(|| {
            ProjectError::Schema(format!(
                "project script path has no existing parent: {}",
                target.display()
            ))
        })?;
    }
    let canonical_ancestor =
        fs::canonicalize(ancestor).map_err(|error| path_error(ancestor, error))?;
    if !canonical_ancestor.starts_with(canonical_root) {
        return Err(ProjectError::Schema(format!(
            "project script path resolves outside the project: {}",
            target.display()
        )));
    }
    Ok(target)
}

fn create_if_missing(path: &Path, contents: &[u8]) -> Result<(), ProjectError> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents).map_err(|error| path_error(path, error))
}

fn path_error(path: &Path, error: std::io::Error) -> ProjectError {
    ProjectError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

pub(crate) fn validate_project_value(value: Value) -> Result<(), ProjectError> {
    RowlyProject::from_value(value).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub id: String,
    pub kind: SourceKind,
    pub path: PathBuf,
    pub recursive: bool,
}

/// Resolve any project-relative script, history, or source reference using the
/// manifest's parent directory. Absolute references remain unchanged.
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

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("failed to access project file `{path}`: {message}")]
    Io { path: String, message: String },
    #[error("failed to read Rowly project `{path}`: {message}")]
    Read { path: String, message: String },
    #[error("failed to parse Rowly project `{path}`: {message}")]
    Parse { path: String, message: String },
    #[error("invalid Rowly project schema: {0}")]
    Schema(String),
    #[error("failed to write Rowly project `{path}`: {message}")]
    Write { path: String, message: String },
}
