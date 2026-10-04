//! Safe, declarative project init scripts for persistent Viewer configuration.

use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use crate::{
    process::{
        ColumnType, CsvDocument, ValidationComparisonOperator, ValidationExpression,
        ValidationOperand, ValidationRule, ValidationTarget,
    },
    project::{
        ProjectError, RowlyProject, SourceKind, resolve_project_reference,
        write_project_file_atomic,
    },
    rowly_dsl::{ColumnSelector, Statement},
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
    validation_rules: BTreeMap<(String, String), ValidationRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidationRuleDeclaration {
    source_id: String,
    header: String,
    rule: ValidationRule,
}

impl ProjectInitConfig {
    fn parse(source: &str, path: &Path) -> Result<Self, ProjectInitError> {
        let mut config = Self::default();
        let lines = source.lines().collect::<Vec<_>>();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index].trim();
            if line.is_empty() || line.starts_with('\'') || is_rem_comment(line) {
                index += 1;
                continue;
            }
            if let Some(declaration) = parse_column_type_declaration(line) {
                config.column_types.insert(
                    (declaration.source_id, declaration.header),
                    declaration.column_type,
                );
                index += 1;
                continue;
            }

            let start_line = index + 1;
            let mut statement = line.to_owned();
            index += 1;
            while validation_statement_needs_continuation(&statement) && index < lines.len() {
                let continuation = lines[index].trim();
                index += 1;
                if continuation.is_empty()
                    || continuation.starts_with('\'')
                    || is_rem_comment(continuation)
                {
                    continue;
                }
                statement.push(' ');
                statement.push_str(continuation);
            }
            let declaration =
                parse_project_validation_declaration(&statement).map_err(|message| {
                    ProjectInitError::InvalidConfig {
                        path: path.display().to_string(),
                        line: start_line,
                        message,
                    }
                })?;
            config.validation_rules.insert(
                (declaration.source_id, declaration.header),
                declaration.rule,
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
        for ((source_id, header), rule) in &self.validation_rules {
            output.push_str("SET This.Project.Source(");
            output.push_str(&quote(source_id));
            output.push_str(").Worksheet.Editor.Column(");
            output.push_str(&quote(header));
            match rule {
                ValidationRule::AllowedValues(values) => {
                    output.push_str(").Validation.AllowedValues = [");
                    output.push_str(
                        &values
                            .iter()
                            .map(|value| quote(value))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    output.push_str("]\n");
                }
                ValidationRule::Expression(expression) => {
                    output.push_str(").Validation.Expression = ");
                    output.push_str(&serialize_validation_expression(expression));
                    output.push('\n');
                }
            }
        }
        output
    }

    fn update_column_types(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
        user_config: &ProjectInitConfig,
    ) {
        let headers = document
            .rows()
            .next()
            .map(|row| {
                row.iter()
                    .cloned()
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        let declarations = document
            .column_type_declarations()
            .map(|(header, column_type)| (header.to_owned(), column_type))
            .collect::<BTreeMap<_, _>>();
        for header in headers {
            let key = (source_id.to_owned(), header.clone());
            match declarations.get(&key.1).copied() {
                Some(column_type)
                    if user_config.column_types.get(&key).copied() == Some(column_type) =>
                {
                    // This is the effective user override. Preserve the existing generated
                    // value instead of copying the merged document back into generated.rly.
                }
                Some(column_type) => {
                    self.column_types.insert(key, column_type);
                }
                None => {
                    self.column_types.remove(&key);
                }
            }
        }
    }

    fn update_validation_rules(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
        user_config: &ProjectInitConfig,
    ) -> Result<(), ProjectInitError> {
        let headers = document
            .rows()
            .next()
            .map(|row| row.to_vec())
            .unwrap_or_default();
        let header_counts = headers.iter().fold(HashMap::new(), |mut counts, header| {
            *counts.entry(header.clone()).or_insert(0_usize) += 1;
            counts
        });
        let mut document_rules = BTreeMap::new();
        for (target, rule) in document.validation_rules() {
            let header = match target {
                ValidationTarget::Header(header) => header.clone(),
                ValidationTarget::Index(index) => headers
                    .get(*index)
                    .cloned()
                    .ok_or(ProjectInitError::UnstableValidationTarget(*index))?,
            };
            if header_counts.get(&header).is_some_and(|count| *count > 1) {
                return Err(ProjectInitError::AmbiguousHeader(header));
            }
            let key = (source_id.to_owned(), header);
            if document_rules.insert(key.clone(), rule.clone()).is_some() {
                return Err(ProjectInitError::DuplicateValidationTarget(key.1));
            }
        }

        for ((entry_source, header), rule) in &document_rules {
            if entry_source != source_id {
                continue;
            }
            let key = (entry_source.clone(), header.clone());
            if user_config.validation_rules.get(&key) == Some(rule) {
                // The document contains the effective user override. Keep the generated layer.
                continue;
            }
            self.validation_rules.insert(key, rule.clone());
        }

        for header in &headers {
            let key = (source_id.to_owned(), header.clone());
            if document_rules.contains_key(&key) || user_config.validation_rules.contains_key(&key)
            {
                continue;
            }
            self.validation_rules.remove(&key);
        }
        Ok(())
    }

    fn apply_source(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectInitError> {
        let mut header_counts = HashMap::<String, usize>::new();
        if let Some(row) = document.rows().next() {
            for header in row {
                *header_counts.entry(header.clone()).or_insert(0_usize) += 1;
            }
        }
        for (entry_source, header) in self.column_types.keys() {
            if entry_source == source_id
                && header_counts.get(header).is_some_and(|count| *count > 1)
            {
                return Err(ProjectInitError::AmbiguousHeader(header.clone()));
            }
        }
        let mut validation_rules = BTreeMap::new();
        for ((entry_source, header), rule) in &self.validation_rules {
            if entry_source != source_id {
                continue;
            }
            if header_counts.get(header).is_some_and(|count| *count > 1) {
                return Err(ProjectInitError::AmbiguousHeader(header.clone()));
            }
            validation_rules.insert(ValidationTarget::Header(header.clone()), rule.clone());
        }
        for ((entry_source, header), column_type) in &self.column_types {
            if entry_source == source_id && header_counts.contains_key(header) {
                document.set_column_type_declaration_by_header(header, *column_type)?;
            }
        }
        document.replace_project_validation_rules(validation_rules);
        Ok(())
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
    save_generated_configuration(project, manifest_path, source_id, document, true, false)
}

/// Save one source's validation rules to the generated init DSL.
pub(crate) fn save_generated_validation_rules(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &CsvDocument,
) -> Result<(), ProjectInitError> {
    save_generated_configuration(project, manifest_path, source_id, document, false, true)
}

fn save_generated_configuration(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &CsvDocument,
    update_column_types: bool,
    update_validation_rules: bool,
) -> Result<(), ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;
    ensure_generated_path_does_not_alias_project_data(
        project,
        manifest_path,
        &init_path,
        &generated_path,
        &user_path,
    )?;
    for path in [&init_path, &generated_path, &user_path] {
        validate_regular_file_if_present(path)?;
    }

    let mut config = read_optional_config(&generated_path)?;
    let user_config = read_optional_config(&user_path)?;
    if update_column_types {
        config.update_column_types(source_id, document, &user_config);
    }
    if update_validation_rules {
        config.update_validation_rules(source_id, document, &user_config)?;
    }
    match fs::read_to_string(&init_path) {
        Ok(init_source) => {
            parse_init_includes(&init_source, &init_path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(script_read_error(&init_path, error)),
    }
    // Prepare all included files before replacing generated.rly so an error
    // cannot leave the new configuration persisted with an incomplete setup.
    create_if_missing(&user_path, "")?;
    create_if_missing(&init_path, INIT_TEMPLATE)?;
    create_parent(&generated_path)?;
    write_project_file_atomic(&generated_path, config.serialize().as_bytes())?;
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
        config.column_types.extend(parsed.column_types);
        config.validation_rules.extend(parsed.validation_rules);
    }
    config.apply_source(source_id, document)
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
    if source_id.trim().is_empty() {
        return None;
    }
    Some(ColumnTypeDeclaration {
        source_id,
        header,
        column_type,
    })
}

fn parse_project_validation_declaration(line: &str) -> Result<ValidationRuleDeclaration, String> {
    let rest = strip_prefix_ci(line, "SET This.Project.Source(")
        .ok_or_else(|| "only project column types and validation DSL are allowed".to_owned())?;
    let (source_id, rest) =
        parse_quoted(rest).ok_or_else(|| "expected a quoted source id".to_owned())?;
    let rest = strip_prefix_ci(rest, ").Worksheet.Editor.Column(")
        .ok_or_else(|| "expected a project source column target".to_owned())?;
    let (header, rest) =
        parse_quoted(rest).ok_or_else(|| "expected a quoted column header".to_owned())?;
    let validation = strip_prefix_ci(rest, ").")
        .ok_or_else(|| "expected a validation rule after the column target".to_owned())?;
    if source_id.trim().is_empty() {
        return Err("project source id must not be empty".to_owned());
    }

    let dsl = format!(
        "SET This.Worksheet.Editor.Column({}).{}",
        quote(&header),
        validation
    );
    let program = crate::rowly_dsl::parse(&dsl)
        .map_err(|error| format!("invalid Rowly validation DSL: {}", error.message()))?;
    if !program.classes().is_empty() || !program.functions().is_empty() {
        return Err("project init validation entries must be declarative rules".to_owned());
    }
    let [Statement::SetValidationRule { selector, rule }] = program.statements() else {
        return Err("project init allows only validation rule declarations".to_owned());
    };
    if selector != &ColumnSelector::Header(header.clone()) {
        return Err("project validation target must use a quoted unique header".to_owned());
    }
    Ok(ValidationRuleDeclaration {
        source_id,
        header,
        rule: rule.clone(),
    })
}

fn validation_statement_needs_continuation(statement: &str) -> bool {
    let Some(rest) = project_validation_property(statement) else {
        return false;
    };
    let Some(equal_offset) = rest.value.find('=') else {
        return false;
    };
    let value = rest.value[equal_offset + 1..].trim();
    if rest.allowed_values {
        return value.is_empty() || has_unclosed_bracket(value);
    }
    if value.is_empty() {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    [" and", " or", " =", " !=", " <=", " >=", " <", " >"]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

struct ValidationProperty<'a> {
    value: &'a str,
    allowed_values: bool,
}

fn project_validation_property(statement: &str) -> Option<ValidationProperty<'_>> {
    let rest = strip_prefix_ci(statement, "SET This.Project.Source(")?;
    let (_, rest) = parse_quoted(rest)?;
    let rest = strip_prefix_ci(rest, ").Worksheet.Editor.Column(")?;
    let (_, rest) = parse_quoted(rest)?;
    let rest = strip_prefix_ci(rest, ").Validation.")?;
    if let Some(value) = strip_prefix_ci(rest, "AllowedValues") {
        Some(ValidationProperty {
            value,
            allowed_values: true,
        })
    } else {
        strip_prefix_ci(rest, "Expression").map(|value| ValidationProperty {
            value,
            allowed_values: false,
        })
    }
}

fn has_unclosed_bracket(value: &str) -> bool {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            match ch {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    depth > 0
}

fn serialize_validation_expression(expression: &ValidationExpression) -> String {
    match expression {
        ValidationExpression::Compare {
            left,
            operator,
            right,
        } => format!(
            "{} {} {}",
            serialize_validation_operand(left),
            match operator {
                ValidationComparisonOperator::Equal => "=",
                ValidationComparisonOperator::NotEqual => "!=",
                ValidationComparisonOperator::Less => "<",
                ValidationComparisonOperator::LessOrEqual => "<=",
                ValidationComparisonOperator::Greater => ">",
                ValidationComparisonOperator::GreaterOrEqual => ">=",
            },
            serialize_validation_operand(right)
        ),
        ValidationExpression::Not(inner) => {
            format!("NOT ({})", serialize_validation_expression(inner))
        }
        ValidationExpression::And(left, right) => format!(
            "({}) AND ({})",
            serialize_validation_expression(left),
            serialize_validation_expression(right)
        ),
        ValidationExpression::Or(left, right) => format!(
            "({}) OR ({})",
            serialize_validation_expression(left),
            serialize_validation_expression(right)
        ),
    }
}

fn serialize_validation_operand(operand: &ValidationOperand) -> String {
    match operand {
        ValidationOperand::Value => "Value".to_owned(),
        ValidationOperand::Literal(value) => quote(value),
    }
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

fn ensure_distinct_paths(
    init: &Path,
    generated: &Path,
    user: &Path,
) -> Result<(), ProjectInitError> {
    if same_path(init, generated) || same_path(init, user) || same_path(generated, user) {
        return Err(ProjectInitError::ScriptPathCollision);
    }
    Ok(())
}

fn ensure_generated_path_does_not_alias_project_data(
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

fn same_path(left: &Path, right: &Path) -> bool {
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

fn normalized_path(path: &Path) -> PathBuf {
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

fn validate_regular_file_if_present(path: &Path) -> Result<(), ProjectInitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(ProjectInitError::NotRegularScriptFile(
            path.display().to_string(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(script_read_error(path, error)),
    }
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
    #[error("generated script path `{0}` aliases project data or another project file")]
    GeneratedPathAliasesProjectData(String),
    #[error("project init script target `{0}` must be a regular file")]
    NotRegularScriptFile(String),
    #[error("project column type cannot target duplicate header `{0}`")]
    AmbiguousHeader(String),
    #[error("project validation target `{0}` resolves to more than one column")]
    DuplicateValidationTarget(String),
    #[error("column index {0} has no stable header identity for project persistence")]
    UnstableValidationTarget(usize),
    #[error(transparent)]
    Document(#[from] crate::process::DocumentError),
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
