use std::collections::{BTreeMap, HashMap};

use crate::{
    process::CsvDocument,
    process::{
        ValidationComparisonOperator, ValidationExpression, ValidationOperand, ValidationRule,
        ValidationTarget,
    },
    rowly_dsl::{ColumnSelector, Statement},
};

use super::{ProjectInitConfig, ProjectInitError, parse_quoted, quote, strip_prefix_ci};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidationRuleDeclaration {
    pub(super) source_id: String,
    pub(super) header: String,
    pub(super) rule: ValidationRule,
}

impl ProjectInitConfig {
    pub(super) fn update_validation_rules(
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
                // documentにはuser.rlyの有効な上書きが反映されているため、generated側の宣言を維持する。
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
}

pub(super) fn parse_project_validation_declaration(
    line: &str,
) -> Result<ValidationRuleDeclaration, String> {
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

pub(super) fn validation_statement_needs_continuation(statement: &str) -> bool {
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

pub(super) fn serialize_validation_expression(expression: &ValidationExpression) -> String {
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
