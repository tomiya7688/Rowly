//! 文列の実行と制御フローを扱う。

use super::*;

impl<'a> Runtime<'a> {
    // {
    //   責務: [execute_statements: 文列を順に評価し制御flowを伝播する]
    //   処理: [条件分岐、loop、宣言、代入、呼び出し、document操作を実行する]
    //   引数: [statements: 評価するAST文列]
    //   戻り値: [Result<Flow, ExecutionError>: 継続またはreturn値]
    //   エラー: [ExecutionError: 式評価、binding、object、document操作の失敗]
    // }
    pub(super) fn execute_statements(
        &mut self,
        statements: &[Statement],
    ) -> Result<Flow, ExecutionError> {
        for statement in statements {
            match statement {
                Statement::If {
                    condition,
                    body,
                    else_body,
                } => {
                    let result = self.evaluate_condition(condition)?;
                    self.events.push(ExecutionEvent::ConditionEvaluated {
                        condition: condition.clone(),
                        result,
                    });
                    let branch = if result { body } else { else_body };
                    let flow = self.execute_statements(branch)?;
                    if !matches!(flow, Flow::Continue) {
                        return Ok(flow);
                    }
                }
                Statement::For {
                    variable,
                    start,
                    end,
                    step,
                    body,
                } => {
                    let start = self.evaluate_expression(start)?;
                    let end = self.evaluate_expression(end)?;
                    let step = step
                        .as_ref()
                        .map(|expression| self.evaluate_expression(expression))
                        .transpose()?;
                    let mut current = self.expect_integer(start, "For start")?;
                    let end = self.expect_integer(end, "For end")?;
                    let step = match step {
                        Some(value) => self.expect_integer(value, "For Step")?,
                        None => 1,
                    };
                    if step == 0 {
                        return Err(ExecutionError::ZeroLoopStep);
                    }

                    loop {
                        let in_range = if step > 0 {
                            current <= end
                        } else {
                            current >= end
                        };
                        if !in_range {
                            break;
                        }

                        // 各反復の宣言を独立させ、CONST の再宣言や外側への漏出を防ぐ。
                        self.scopes.push(HashMap::from([(
                            normalize_identifier(variable),
                            Binding::variable(Value::Integer(current)),
                        )]));
                        let execution = self.execute_statements(body);
                        self.scopes.pop();
                        let flow = execution?;
                        if !matches!(flow, Flow::Continue) {
                            return Ok(flow);
                        }

                        current = current
                            .checked_add(step)
                            .ok_or(ExecutionError::LoopCounterOverflow)?;
                    }
                }
                Statement::Declare { kind, name, value } => {
                    let key = normalize_identifier(name);
                    // RHS の関数呼び出し等による副作用より先に、宣言先を検証する。
                    if self.current_scope_mut().contains_key(&key) {
                        return Err(ExecutionError::DuplicateVariable(name.clone()));
                    }
                    let value = self.evaluate_expression(value)?;
                    let event_value = self.describe_value(&value);
                    self.current_scope_mut()
                        .insert(key, Binding { value, kind: *kind });
                    self.events.push(ExecutionEvent::VariableSet {
                        name: name.clone(),
                        value: event_value,
                    });
                }
                Statement::Assign { name, value } => {
                    let key = normalize_identifier(name);
                    let scope = self
                        .scopes
                        .iter()
                        .rposition(|scope| scope.contains_key(&key))
                        .ok_or_else(|| ExecutionError::UnknownVariable(name.clone()))?;
                    if self.scopes[scope][&key].kind == DeclarationKind::Const {
                        return Err(ExecutionError::ConstantAssignment(name.clone()));
                    }
                    let value = self.evaluate_expression(value)?;
                    let event_value = self.describe_value(&value);
                    self.scopes[scope]
                        .get_mut(&key)
                        .expect("assignment target was resolved before evaluation")
                        .value = value;
                    self.events.push(ExecutionEvent::VariableSet {
                        name: name.clone(),
                        value: event_value,
                    });
                }
                Statement::Return { value } => {
                    let value = value
                        .as_ref()
                        .map(|expression| self.evaluate_expression(expression))
                        .transpose()?;
                    return Ok(Flow::Return(value));
                }
                Statement::Call { name, arguments } => {
                    if self.call_statement_builtin(name, arguments)? {
                        continue;
                    }
                    if self.call_builtin(name, arguments)?.is_none() {
                        let _ = self.call_function(name, arguments)?;
                    }
                }
                Statement::MethodCall {
                    target,
                    name,
                    arguments,
                } => {
                    if target.eq_ignore_ascii_case("Super") {
                        let _ = self.call_super_method(name, arguments)?;
                    } else {
                        let object_id = self.resolve_object(target)?;
                        let _ = self.call_method(object_id, name, arguments)?;
                    }
                }
                Statement::SetField {
                    target,
                    field,
                    value,
                } => {
                    let value = self.evaluate_expression(value)?;
                    let object_id = self.resolve_object(target)?;
                    self.set_field(object_id, field, value.clone())?;
                    self.events.push(ExecutionEvent::FieldSet {
                        target: target.clone(),
                        field: field.clone(),
                        value: self.describe_value(&value),
                    });
                }
                Statement::SetRangeValue { range, value } => {
                    let value = self.evaluate_expression(value)?;
                    let value = self.cell_text(value)?;
                    self.document.set_range_value(*range, value.clone())?;
                    self.events.push(ExecutionEvent::RangeValueSet {
                        range: *range,
                        value,
                    });
                }
                Statement::SetCalculationExpression { target, expression } => {
                    // Bindingはcalculation engineだけを更新し、canonical CSVには式を書き込まない。
                    let definition = CalculationBindingDefinition {
                        target: *target,
                        expression: expression.clone(),
                        trigger: CalculationTrigger::DependencyChange,
                    };
                    if let Some(engine) = self.calculation_engine.as_deref_mut() {
                        let target_cells = expand_calculation_target(*target, self.document)?;
                        let origin = calculation_target_origin(*target);
                        let calculation_expression = compile_calculation_expression(expression)?;
                        let expanded = target_cells
                            .into_iter()
                            .map(|target_cell| {
                                let row_offset = isize::try_from(target_cell.row())
                                    .and_then(|row| {
                                        isize::try_from(origin.row()).map(|origin| row - origin)
                                    })
                                    .map_err(|_| {
                                        ExecutionError::InvalidCalculationExpression(
                                            "calculation target row is too large".to_owned(),
                                        )
                                    })?;
                                let column_offset = isize::try_from(target_cell.column())
                                    .and_then(|column| {
                                        isize::try_from(origin.column())
                                            .map(|origin| column - origin)
                                    })
                                    .map_err(|_| {
                                        ExecutionError::InvalidCalculationExpression(
                                            "calculation target column is too large".to_owned(),
                                        )
                                    })?;
                                let expression = calculation_expression
                                    .shift_cell_references(row_offset, column_offset)
                                    .ok_or_else(|| {
                                        ExecutionError::InvalidCalculationExpression(format!(
                                            "relative cell reference is outside the table for target {target_cell}"
                                        ))
                                    })?;
                                let id = CalculationBindingId::new(format!("dsl:{target_cell}"))?;
                                Ok::<_, ExecutionError>((target_cell, id, expression))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        for (target_cell, id, _) in &expanded {
                            if engine.bindings().any(|binding| {
                                binding.target().single_cell() == Some(*target_cell)
                                    && binding.id() != id
                            }) {
                                return Err(CalculationError::DuplicateTarget(*target_cell).into());
                            }
                        }
                        if matches!(target, CalculationTarget::Range(_)) {
                            let mut staged_engine = engine.clone();
                            for (target_cell, id, expression) in expanded {
                                staged_engine.set_binding(
                                    self.document,
                                    id,
                                    CalculationTarget::Cell(target_cell),
                                    expression,
                                    CalculationTrigger::DependencyChange,
                                )?;
                            }
                            *engine = staged_engine;
                        } else {
                            for (target_cell, id, expression) in expanded {
                                engine.set_binding(
                                    self.document,
                                    id,
                                    CalculationTarget::Cell(target_cell),
                                    expression,
                                    CalculationTrigger::DependencyChange,
                                )?;
                            }
                        }
                    }
                    self.calculation_bindings.push(definition.clone());
                    self.events
                        .push(ExecutionEvent::CalculationBindingSet { definition });
                }
                Statement::SetCalculationTrigger { target, trigger } => {
                    let target_cells = expand_calculation_target(*target, self.document)?;
                    if let Some(engine) = self.calculation_engine.as_deref_mut() {
                        let updates = target_cells
                            .into_iter()
                            .map(|target_cell| {
                                let binding_id =
                                    CalculationBindingId::new(format!("dsl:{target_cell}"))?;
                                let binding = engine.binding(&binding_id).ok_or_else(|| {
                                ExecutionError::InvalidCalculationExpression(format!(
                                    "cell {target_cell} has no calculation expression to configure"
                                ))
                            })?;
                                let expression = binding.expression().clone();
                                Ok::<_, ExecutionError>((target_cell, binding_id, expression))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if matches!(target, CalculationTarget::Range(_)) {
                            let mut staged_engine = engine.clone();
                            for (target_cell, binding_id, expression) in updates {
                                staged_engine.set_binding(
                                    self.document,
                                    binding_id,
                                    CalculationTarget::Cell(target_cell),
                                    expression,
                                    *trigger,
                                )?;
                            }
                            *engine = staged_engine;
                        } else {
                            for (target_cell, binding_id, expression) in updates {
                                engine.set_binding(
                                    self.document,
                                    binding_id,
                                    CalculationTarget::Cell(target_cell),
                                    expression,
                                    *trigger,
                                )?;
                            }
                        }
                    } else if !self
                        .calculation_bindings
                        .iter()
                        .any(|definition| definition.target == *target)
                    {
                        return Err(ExecutionError::InvalidCalculationExpression(format!(
                            "calculation target {target:?} has no expression to configure"
                        )));
                    }
                    if let Some(definition) = self
                        .calculation_bindings
                        .iter_mut()
                        .rev()
                        .find(|definition| definition.target == *target)
                    {
                        definition.trigger = *trigger;
                    }
                    self.events
                        .push(ExecutionEvent::CalculationBindingTriggerSet {
                            target: *target,
                            trigger: *trigger,
                        });
                }
                Statement::ValidateColumnType {
                    selector,
                    column_type,
                } => {
                    let column = resolve_column(selector, self.document)?;
                    let report = self.document.validate_column_type(column, *column_type)?;
                    self.events.push(ExecutionEvent::ColumnTypeChecked {
                        selector: selector.clone(),
                        report,
                    });
                }
                Statement::CheckJapanese { selector } => {
                    let column = resolve_column(selector, self.document)?;
                    let report = self.document.check_column_japanese(column)?;
                    self.events.push(ExecutionEvent::JapaneseChecked {
                        selector: selector.clone(),
                        report,
                    });
                }
                Statement::SetValidationRule { selector, rule } => {
                    let column = resolve_column(selector, self.document)?;
                    let target = match selector {
                        ColumnSelector::Index(_) => ValidationTarget::Index(column),
                        ColumnSelector::Header(header) => ValidationTarget::Header(header.clone()),
                    };
                    self.document.set_validation_rule(target, rule.clone())?;
                    let definition = ValidationRuleDefinition {
                        selector: selector.clone(),
                        rule: rule.clone(),
                    };
                    self.validation_rules.push(definition.clone());
                    self.events
                        .push(ExecutionEvent::ValidationRuleSet { definition });
                }
            }
        }
        Ok(Flow::Continue)
    }
}
