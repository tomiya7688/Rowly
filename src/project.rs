//! Versioned `.rwprj` project manifests. The manifest stores references only;
//! CSV contents remain in their source files.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

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
    /// Optional boundary for a bounded relink search.
    pub search_root: Option<PathBuf>,
    /// Optional ordered CSV header signature used to identify a moved source.
    pub schema: Option<Vec<String>>,
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
                search_root: source
                    .search_root
                    .as_ref()
                    .map(|path| resolve_project_reference(project_path.as_ref(), path)),
                schema: source.schema.clone(),
            })
            .collect()
    }

    /// Resolve missing file sources only inside each source's declared search
    /// root. A relink is applied only when filename and saved header schema
    /// identify exactly one candidate; the stable source id is never changed.
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

    /// Snapshot CSV headers for file sources so later relink searches can
    /// distinguish same-named files inside the declared search root.
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

pub(crate) fn validate_project_value(value: Value) -> Result<(), ProjectError> {
    RowlyProject::from_value(value).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub id: String,
    pub kind: SourceKind,
    pub path: PathBuf,
    pub recursive: bool,
    pub search_root: Option<PathBuf>,
    pub schema: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    Available,
    Missing,
    Relinked,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceResolution {
    pub id: String,
    pub status: SourceStatus,
    pub path: Option<PathBuf>,
}

fn source_path_exists(kind: SourceKind, path: &Path) -> bool {
    match kind {
        SourceKind::File => path.is_file(),
        SourceKind::Directory => path.is_dir(),
    }
}

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
