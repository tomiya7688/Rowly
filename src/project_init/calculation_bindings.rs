use std::collections::BTreeMap;

use crate::{
    process::{
        CellRange, CellRef,
        calculation::{CalculationEngine, CalculationTarget, CalculationTrigger},
    },
    rowly_dsl::Statement,
};

use super::{ProjectInitConfig, ProjectInitError, parse_quoted, quote, strip_prefix_ci};

#[derive(Debug, Default, Clone)]
pub(super) struct ProjectCalculationBinding {
    pub(super) expression: Option<String>,
    pub(super) trigger: Option<CalculationTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CalculationBindingDeclaration {
    pub(super) source_id: String,
    pub(super) target: CalculationTarget,
    pub(super) expression: Option<String>,
    pub(super) trigger: Option<CalculationTrigger>,
}

impl ProjectInitConfig {
    pub(super) fn update_calculation_bindings(
        &mut self,
        source_id: &str,
        engine: &CalculationEngine,
        user_config: &ProjectInitConfig,
    ) -> Result<(), ProjectInitError> {
        let mut engine_bindings = BTreeMap::new();
        for binding in engine.bindings() {
            let Some(target) = binding.target().single_cell() else {
                return Err(ProjectInitError::UnsupportedCalculationTarget);
            };
            engine_bindings.insert(
                (source_id.to_owned(), CalculationTarget::Cell(target)),
                ProjectCalculationBinding {
                    expression: Some(binding.expression().to_dsl_source()),
                    trigger: Some(binding.trigger()),
                },
            );
        }

        let existing_source_targets = self
            .calculation_bindings
            .keys()
            .filter(|(entry_source, _)| entry_source == source_id)
            .cloned()
            .collect::<Vec<_>>();
        for key in existing_source_targets {
            if !engine_bindings.contains_key(&key) {
                self.calculation_bindings.remove(&key);
            }
        }
        for (key, binding) in engine_bindings {
            if user_config
                .calculation_bindings
                .keys()
                .any(|(entry_source, configured_target)| {
                    entry_source == &key.0
                        && key.1.single_cell().is_some_and(|cell| {
                            calculation_target_contains(*configured_target, cell)
                        })
                })
            {
                continue;
            }
            self.calculation_bindings.insert(key, binding);
        }
        Ok(())
    }
}

/// ```text
/// 責務: [serialize_calculation_target: calculation targetをproject DSLの種類とA1文字列に変換する]
/// 引数: [target: 保存する計算対象]
/// 戻り値: [(&'static str, String): target構文名とA1表記]
/// ```
pub(super) fn serialize_calculation_target(target: CalculationTarget) -> (&'static str, String) {
    match target {
        CalculationTarget::Cell(cell) => ("Cell", cell.to_string()),
        CalculationTarget::Range(range) => ("Range", format!("{}:{}", range.start(), range.end())),
        CalculationTarget::ColumnRelative { .. } => ("ColumnRelative", String::new()),
    }
}

/// ```text
/// 責務: [expand_project_calculation_target: project設定targetを含まれるcellへ展開する]
/// 引数: [target: 展開する計算対象]
/// 戻り値: [Vec<CellRef>: targetに含まれるcell]
/// ```
pub(super) fn expand_project_calculation_target(target: CalculationTarget) -> Vec<CellRef> {
    match target {
        CalculationTarget::Cell(cell) => vec![cell],
        CalculationTarget::Range(range) => range.iter().collect(),
        CalculationTarget::ColumnRelative { .. } => Vec::new(),
    }
}

/// ```text
/// 責務: [calculation_target_contains: project設定targetがcellを含むか判定する]
/// 引数: [target: 検査するtarget, cell: 検査するcell]
/// 戻り値: [bool: targetがcellを含む場合true]
/// ```
pub(super) fn calculation_target_contains(target: CalculationTarget, cell: CellRef) -> bool {
    match target {
        CalculationTarget::Cell(target_cell) => target_cell == cell,
        CalculationTarget::Range(range) => {
            let start = range.start();
            let end = range.end();
            cell.row() >= start.row()
                && cell.row() <= end.row()
                && cell.column() >= start.column()
                && cell.column() <= end.column()
        }
        CalculationTarget::ColumnRelative { .. } => false,
    }
}

pub(super) fn calculation_targets_overlap(
    left: CalculationTarget,
    right: CalculationTarget,
) -> bool {
    match (left, right) {
        (CalculationTarget::Cell(cell), target) | (target, CalculationTarget::Cell(cell)) => {
            calculation_target_contains(target, cell)
        }
        (CalculationTarget::Range(left), CalculationTarget::Range(right)) => {
            let left_start = left.start();
            let left_end = left.end();
            let right_start = right.start();
            let right_end = right.end();
            left_start.row() <= right_end.row()
                && right_start.row() <= left_end.row()
                && left_start.column() <= right_end.column()
                && right_start.column() <= left_end.column()
        }
        _ => false,
    }
}

pub(super) fn is_project_calculation_declaration(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("set this.project.source(")
        && (lower.contains(").worksheet.editor.cell(")
            || lower.contains(").worksheet.editor.range("))
}

pub(super) fn parse_project_calculation_declaration(
    line: &str,
) -> Result<CalculationBindingDeclaration, String> {
    let rest = strip_prefix_ci(line, "SET This.Project.Source(")
        .ok_or_else(|| "expected a project source calculation declaration".to_owned())?;
    let (source_id, rest) =
        parse_quoted(rest).ok_or_else(|| "expected a quoted project source id".to_owned())?;
    let (target_prefix, target_kind) =
        if let Some(rest) = strip_prefix_ci(rest, ").Worksheet.Editor.Cell(") {
            (rest, "Cell")
        } else if let Some(rest) = strip_prefix_ci(rest, ").Worksheet.Editor.Range(") {
            (rest, "Range")
        } else {
            return Err("expected a project source cell or range target".to_owned());
        };
    let rest = target_prefix;
    let (target_text, rest) = parse_quoted(rest)
        .ok_or_else(|| "expected a quoted A1 cell or range reference".to_owned())?;
    if source_id.trim().is_empty() {
        return Err("project source id must not be empty".to_owned());
    }
    let target = if target_kind == "Cell" {
        CalculationTarget::Cell(
            target_text
                .parse::<CellRef>()
                .map_err(|error| format!("invalid calculation target: {error}"))?,
        )
    } else {
        CalculationTarget::Range(
            target_text
                .parse::<CellRange>()
                .map_err(|error| format!("invalid calculation target: {error}"))?,
        )
    };
    let (target_dsl_kind, target_dsl_text) = serialize_calculation_target(target);
    let (expression, trigger, dsl) = if let Some(expression_source) =
        strip_prefix_ci(rest, ").Calculation.Expression =")
    {
        let expression_source = expression_source.trim();
        if expression_source.is_empty() {
            return Err("calculation expression must not be empty".to_owned());
        }
        let dsl = format!(
            "SET This.Worksheet.Editor.{target_dsl_kind}({}).Calculation.Expression = {expression_source}",
            quote(&target_dsl_text)
        );
        (Some(expression_source.to_owned()), None, dsl)
    } else if let Some(trigger_source) = strip_prefix_ci(rest, ").Calculation.Trigger =") {
        let trigger_source = trigger_source.trim();
        let trigger = if trigger_source.eq_ignore_ascii_case("Manual") {
            CalculationTrigger::Manual
        } else if trigger_source.eq_ignore_ascii_case("DependencyChange")
            || trigger_source.eq_ignore_ascii_case("Automatic")
        {
            CalculationTrigger::DependencyChange
        } else {
            return Err("Calculation.Trigger must be Manual or DependencyChange".to_owned());
        };
        let dsl = format!(
            "SET This.Worksheet.Editor.{target_dsl_kind}({}).Calculation.Trigger = {trigger_source}",
            quote(&target_dsl_text)
        );
        (None, Some(trigger), dsl)
    } else {
        return Err("expected `.Calculation.Expression` or `.Calculation.Trigger`".to_owned());
    };
    let program = crate::rowly_dsl::parse(&dsl)
        .map_err(|error| format!("invalid calculation binding DSL: {}", error.message()))?;
    if !program.classes().is_empty() || !program.functions().is_empty() {
        return Err("project init calculation entries must be declarative rules".to_owned());
    }
    let parsed_target = match program.statements() {
        [Statement::SetCalculationExpression { target, .. }] => target,
        [Statement::SetCalculationTrigger { target, .. }] => target,
        _ => {
            return Err("project init allows only calculation expression declarations".to_owned());
        }
    };
    if parsed_target != &target {
        return Err("project calculation target does not match its DSL target".to_owned());
    }
    Ok(CalculationBindingDeclaration {
        source_id,
        target,
        expression,
        trigger,
    })
}
