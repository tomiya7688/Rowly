//! Safe, declarative project init scripts for persistent Viewer configuration.

use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use crate::{
    process::{ColumnType, CsvDocument},
    project::{ProjectError, RowlyProject, write_project_file_atomic},
};

const INIT_TEMPLATE: &str = "INCLUDE GENERATED\nINCLUDE USER\n";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnTypeDeclaration {
    source_id: String,
    header: String,
    column_type: ColumnType,
}

#[derive(Debug, Default)]
struct ProjectInitConfig {
    column_types: BTreeMap<(String, String), ColumnType>,
}

impl ProjectInitConfig {
    fn parse(source: &str, path: &Path) -> Result<Self, ProjectInitError> {
        let mut config = Self::default();
        for (index, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('\'') || is_rem_comment(line) {
                continue;
            }
            let declaration = parse_column_type_declaration(line).ok_or_else(|| {
                ProjectInitError::InvalidConfig {
                    path: path.display().to_string(),
                    line: index + 1,
                    message: "only project column type configuration is allowed in init scripts"
                        .into(),
                }
            })?;
            config.column_types.insert(
                (declaration.source_id, declaration.header),
                declaration.column_type,
            );
        }
        Ok(config)
    }

    fn serialize(&self) -> String {
        let mut output = String::new();
        for ((source_id, header), column_type) in &self.column_types {
            output.push_str("SET This.Project.Source(");
            output.push_str(&quote(source_id));
            output.push_str(").Worksheet.Column(");
            output.push_str(&quote(header));
            output.push_str(").Type = ");
            output.push_str(column_type.as_metadata_str());
            output.push('\n');
        }
        output
    }

    fn replace_source(&mut self, source_id: &str, document: &CsvDocument) {
        self.column_types
            .retain(|(existing_source, _), _| existing_source != source_id);
        for (header, column_type) in document.column_type_declarations() {
            self.column_types
                .insert((source_id.to_owned(), header.to_owned()), column_type);
        }
    }

    fn apply_source(&self, source_id: &str, document: &mut CsvDocument) {
        let headers = document
            .rows()
            .next()
            .map(|row| row.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();
        for ((entry_source, header), column_type) in &self.column_types {
            if entry_source == source_id && headers.contains(header) {
                let _ = document.set_column_type_declaration_by_header(header, *column_type);
            }
        }
    }
}

/// Save one source's Viewer column type declarations to the generated init DSL.
/// The generated file is app-owned; the user script is only created when absent
/// and is never rewritten by this operation.
pub(crate) fn save_generated_column_types(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &CsvDocument,
) -> Result<(), ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;

    let mut config = read_optional_config(&generated_path)?;
    config.replace_source(source_id, document);
    create_parent(&generated_path)?;
    write_project_file_atomic(&generated_path, config.serialize().as_bytes())?;
    create_if_missing(&user_path, "")?;
    create_if_missing(&init_path, INIT_TEMPLATE)?;
    Ok(())
}

/// Apply only the config-only statements referenced by `init.rly`.
/// Arbitrary DSL, macro execution, and filesystem operations are rejected.
pub(crate) fn apply_safe_init(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &mut CsvDocument,
) -> Result<(), ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;

    let init_source = match fs::read_to_string(&init_path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(script_read_error(&init_path, error)),
    };
    for include in parse_init_includes(&init_source, &init_path)? {
        let path = match include {
            InitInclude::Generated => &generated_path,
            InitInclude::User => &user_path,
        };
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(script_read_error(path, error)),
        };
        ProjectInitConfig::parse(&source, path)?.apply_source(source_id, document);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum InitInclude {
    Generated,
    User,
}

fn parse_init_includes(source: &str, path: &Path) -> Result<Vec<InitInclude>, ProjectInitError> {
    let mut includes = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('\'') || is_rem_comment(line) {
            continue;
        }
        let include = if line.eq_ignore_ascii_case("INCLUDE GENERATED") {
            InitInclude::Generated
        } else if line.eq_ignore_ascii_case("INCLUDE USER") {
            InitInclude::User
        } else {
            return Err(ProjectInitError::InvalidInit {
                path: path.display().to_string(),
                line: index + 1,
                message: "init.rly may only include GENERATED and USER configuration".into(),
            });
        };
        if includes.iter().any(|entry| {
            matches!(
                (entry, include),
                (InitInclude::Generated, InitInclude::Generated)
                    | (InitInclude::User, InitInclude::User)
            )
        }) {
            return Err(ProjectInitError::InvalidInit {
                path: path.display().to_string(),
                line: index + 1,
                message: "an init source may only be included once".into(),
            });
        }
        includes.push(include);
    }
    if !matches!(
        includes.as_slice(),
        [InitInclude::Generated, InitInclude::User]
    ) {
        return Err(ProjectInitError::InvalidInit {
            path: path.display().to_string(),
            line: source.lines().count().max(1),
            message: "init.rly must include GENERATED followed by USER".into(),
        });
    }
    Ok(includes)
}

fn parse_column_type_declaration(line: &str) -> Option<ColumnTypeDeclaration> {
    let rest = strip_prefix_ci(line, "SET This.Project.Source(")?;
    let (source_id, rest) = parse_quoted(rest)?;
    let rest = strip_prefix_ci(rest, ").Worksheet.Column(")?;
    let (header, rest) = parse_quoted(rest)?;
    let rest = strip_prefix_ci(rest, ").Type = ")?;
    let column_type = [
        ColumnType::String,
        ColumnType::Integer,
        ColumnType::Decimal,
        ColumnType::Boolean,
    ]
    .into_iter()
    .find(|column_type| {
        rest.trim()
            .eq_ignore_ascii_case(column_type.as_metadata_str())
    })?;
    if source_id.trim().is_empty() || header.trim().is_empty() {
        return None;
    }
    Some(ColumnTypeDeclaration {
        source_id,
        header,
        column_type,
    })
}

fn parse_quoted(input: &str) -> Option<(String, &str)> {
    let mut chars = input.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in chars {
        if escaped {
            value.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                _ => return None,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some((value, &input[index + ch.len_utf8()..]));
        } else {
            value.push(ch);
        }
    }
    None
}

fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|actual| actual.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn is_rem_comment(line: &str) -> bool {
    line.get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("REM "))
}

fn project_script_path(
    manifest_path: &Path,
    reference: &Path,
) -> Result<PathBuf, ProjectInitError> {
    if reference.is_absolute()
        || reference.as_os_str().is_empty()
        || !reference
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(ProjectInitError::UnsafeScriptPath(
            reference.display().to_string(),
        ));
    }
    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(root).map_err(|error| script_read_error(root, error))?;
    let path = root.join(reference);
    let mut current = root.clone();
    for component in reference.components() {
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

fn ensure_distinct_paths(
    init: &Path,
    generated: &Path,
    user: &Path,
) -> Result<(), ProjectInitError> {
    if init == generated || init == user || generated == user {
        return Err(ProjectInitError::ScriptPathCollision);
    }
    Ok(())
}

fn read_optional_config(path: &Path) -> Result<ProjectInitConfig, ProjectInitError> {
    match fs::read_to_string(path) {
        Ok(source) => ProjectInitConfig::parse(&source, path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ProjectInitConfig::default()),
        Err(error) => Err(script_read_error(path, error)),
    }
}

fn create_parent(path: &Path) -> Result<(), ProjectInitError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| ProjectInitError::ScriptWrite {
        path: parent.display().to_string(),
        message: error.to_string(),
    })
}

fn create_if_missing(path: &Path, contents: &str) -> Result<(), ProjectInitError> {
    create_parent(path)?;
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
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

fn script_read_error(path: &Path, error: io::Error) -> ProjectInitError {
    ProjectInitError::ScriptRead {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

#[derive(Debug, Error)]
pub enum ProjectInitError {
    #[error("project source id must not be empty")]
    EmptySourceId,
    #[error("project init script path `{0}` must be project-relative and must not traverse links")]
    UnsafeScriptPath(String),
    #[error("project init script paths collide")]
    ScriptPathCollision,
    #[error("failed to read project init script `{path}`: {message}")]
    ScriptRead { path: String, message: String },
    #[error("failed to write project init script `{path}`: {message}")]
    ScriptWrite { path: String, message: String },
    #[error("invalid project init script `{path}` at line {line}: {message}")]
    InvalidInit {
        path: String,
        line: usize,
        message: String,
    },
    #[error("invalid project config script `{path}` at line {line}: {message}")]
    InvalidConfig {
        path: String,
        line: usize,
        message: String,
    },
    #[error(transparent)]
    Project(#[from] ProjectError),
}
