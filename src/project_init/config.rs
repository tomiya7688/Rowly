use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use crate::process::{
    ColumnType, CsvDocument, ValidationRule, ValidationTarget,
    calculation::{CalculationTarget, CalculationTrigger},
};

use super::{
    ProjectCalculationBinding, ProjectInitError, is_project_calculation_declaration,
    is_rem_comment, parse_column_type_declaration, parse_project_calculation_declaration,
    parse_project_validation_declaration, quote, serialize_calculation_target,
    serialize_validation_expression, validation_statement_needs_continuation,
};

#[derive(Debug, Default, Clone)]
pub(super) struct ProjectInitConfig {
    pub(super) column_types: BTreeMap<(String, String), ColumnType>,
    pub(super) validation_rules: BTreeMap<(String, String), ValidationRule>,
    pub(super) calculation_bindings:
        BTreeMap<(String, CalculationTarget), ProjectCalculationBinding>,
}

impl ProjectInitConfig {
    pub(super) fn apply_user_overrides(&mut self, user_config: Self) {
        self.column_types.extend(user_config.column_types);
        self.validation_rules.extend(user_config.validation_rules);

        let user_ranges = user_config
            .calculation_bindings
            .keys()
            .filter_map(|(source_id, target)| match target {
                CalculationTarget::Range(range) => Some((source_id.clone(), *range)),
                CalculationTarget::Cell(_) | CalculationTarget::ColumnRelative { .. } => None,
            })
            .collect::<Vec<_>>();
        self.calculation_bindings.retain(|(source_id, target), _| {
            let Some(cell) = target.single_cell() else {
                return true;
            };
            !user_ranges.iter().any(|(user_source, range)| {
                user_source == source_id
                    && super::calculation_target_contains(CalculationTarget::Range(*range), cell)
            })
        });
        self.calculation_bindings
            .extend(user_config.calculation_bindings);
    }

    pub(super) fn parse(source: &str, path: &Path) -> Result<Self, ProjectInitError> {
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

            if is_project_calculation_declaration(line) {
                let start_line = index + 1;
                let mut statement = line.to_owned();
                index += 1;
                while statement.trim_end().ends_with('=') && index < lines.len() {
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
                    parse_project_calculation_declaration(&statement).map_err(|message| {
                        ProjectInitError::InvalidConfig {
                            path: path.display().to_string(),
                            line: start_line,
                            message,
                        }
                    })?;
                let binding_key = (declaration.source_id, declaration.target);
                if declaration.trigger.is_some()
                    && declaration.expression.is_none()
                    && !config
                        .calculation_bindings
                        .get(&binding_key)
                        .is_some_and(|binding| binding.expression.is_some())
                {
                    return Err(ProjectInitError::InvalidConfig {
                        path: path.display().to_string(),
                        line: start_line,
                        message: "Calculation.Trigger must follow its Calculation.Expression"
                            .to_owned(),
                    });
                }
                let binding = config.calculation_bindings.entry(binding_key).or_default();
                if let Some(expression) = declaration.expression {
                    binding.expression = Some(expression);
                }
                if let Some(trigger) = declaration.trigger {
                    binding.trigger = Some(trigger);
                }
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

    pub(super) fn serialize(&self) -> String {
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
        for ((source_id, target), binding) in &self.calculation_bindings {
            let (target_kind, target_text) = serialize_calculation_target(*target);
            output.push_str("SET This.Project.Source(");
            output.push_str(&quote(source_id));
            output.push_str(").Worksheet.Editor.");
            output.push_str(target_kind);
            output.push('(');
            output.push_str(&quote(&target_text));
            output.push_str(").Calculation.Expression = ");
            output.push_str(binding.expression.as_deref().unwrap_or_default());
            output.push('\n');
            output.push_str("SET This.Project.Source(");
            output.push_str(&quote(source_id));
            output.push_str(").Worksheet.Editor.");
            output.push_str(target_kind);
            output.push('(');
            output.push_str(&quote(&target_text));
            output.push_str(").Calculation.Trigger = ");
            output.push_str(
                match binding
                    .trigger
                    .unwrap_or(CalculationTrigger::DependencyChange)
                {
                    CalculationTrigger::DependencyChange => "DependencyChange",
                    CalculationTrigger::Manual => "Manual",
                },
            );
            output.push('\n');
        }
        output
    }

    pub(super) fn update_column_types(
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
                    // document上の値はuser.rlyの有効な上書きなので、generated.rlyの値を維持する。
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

    pub(super) fn apply_source(
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
