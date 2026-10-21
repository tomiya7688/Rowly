use std::{fs, io, path::Path};

use crate::{process::ColumnType, project::RowlyProject};

use super::{
    ProjectInitConfig, ProjectInitError, ensure_distinct_paths, project_script_path,
    script_read_error,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ColumnTypeDeclaration {
    pub(super) source_id: String,
    pub(super) header: String,
    pub(super) column_type: ColumnType,
}

pub(super) fn read_safe_init_config(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
) -> Result<ProjectInitConfig, ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;

    let init_source = match fs::read_to_string(&init_path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ProjectInitConfig::default());
        }
        Err(error) => return Err(script_read_error(&init_path, error)),
    };
    let mut config = ProjectInitConfig::default();
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
        let parsed = ProjectInitConfig::parse(&source, path)?;
        if matches!(include, InitInclude::User) {
            config.apply_user_overrides(parsed);
        } else {
            config.column_types.extend(parsed.column_types);
            config.validation_rules.extend(parsed.validation_rules);
            config
                .calculation_bindings
                .extend(parsed.calculation_bindings);
        }
    }
    Ok(config)
}

#[derive(Debug, Clone, Copy)]
pub(super) enum InitInclude {
    Generated,
    User,
}

pub(super) fn parse_init_includes(
    source: &str,
    path: &Path,
) -> Result<Vec<InitInclude>, ProjectInitError> {
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

pub(super) fn parse_column_type_declaration(line: &str) -> Option<ColumnTypeDeclaration> {
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
    if source_id.trim().is_empty() {
        return None;
    }
    Some(ColumnTypeDeclaration {
        source_id,
        header,
        column_type,
    })
}

pub(super) fn parse_quoted(input: &str) -> Option<(String, &str)> {
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

pub(super) fn quote(value: &str) -> String {
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

pub(super) fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|actual| actual.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

pub(super) fn is_rem_comment(line: &str) -> bool {
    line.get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("REM "))
}
