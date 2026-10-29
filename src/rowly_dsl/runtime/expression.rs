//! 条件・算術式・比較を評価する。

use super::*;

impl<'a> Runtime<'a> {
    // {
    //   責務: [evaluate_condition: 条件ASTをBooleanへ評価する]
    //   処理: [and/orは短絡評価し、column条件は現時点のdocument状態を参照する]
    //   引数: [condition: 評価する条件AST]
    //   戻り値: [Result<bool, ExecutionError>: 条件結果]
    //   エラー: [ExecutionError: 式型、列参照、または比較が不正]
    // }
    pub(super) fn evaluate_condition(
        &mut self,
        condition: &Condition,
    ) -> Result<bool, ExecutionError> {
        match condition {
            Condition::ColumnExists { selector } => Ok(match selector {
                ColumnSelector::Index(column) => *column < self.document.column_count(),
                ColumnSelector::Header(header) => {
                    !self.document.column_indices_by_header(header).is_empty()
                }
            }),
            Condition::ColumnTitleEquals { selector, expected } => {
                let column = resolve_column(selector, self.document)?;
                let expected = self.evaluate_expression(expected)?;
                let expected = self.expect_text(expected, "column title comparison")?;
                Ok(self.document.cell(0, column) == Some(expected.as_str()))
            }
            Condition::Compare {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_expression(left)?;
                let right = self.evaluate_expression(right)?;
                self.compare_values(left, *operator, right)
            }
            Condition::Expression(expression) => {
                let value = self.evaluate_expression(expression)?;
                self.expect_boolean(value, "condition")
            }
            Condition::Not(inner) => Ok(!self.evaluate_condition(inner)?),
            Condition::And(left, right) => {
                if !self.evaluate_condition(left)? {
                    return Ok(false);
                }
                self.evaluate_condition(right)
            }
            Condition::Or(left, right) => {
                if self.evaluate_condition(left)? {
                    return Ok(true);
                }
                self.evaluate_condition(right)
            }
        }
    }

    // {
    //   責務: [compare_values: DSL値を指定比較演算子で比較する]
    //   処理: [数値はInteger/Decimal間を比較し、等値比較では同型object identityも扱う]
    //   引数: [left: 左値, operator: 比較演算子, right: 右値]
    //   戻り値: [Result<bool, ExecutionError>: 比較結果]
    //   エラー: [ExecutionError: 順序比較できない値の組み合わせ]
    // }
    pub(super) fn compare_values(
        &self,
        left: Value,
        operator: ComparisonOperator,
        right: Value,
    ) -> Result<bool, ExecutionError> {
        use ComparisonOperator::{Equal, Greater, GreaterOrEqual, Less, LessOrEqual, NotEqual};

        if matches!(operator, Equal | NotEqual) {
            let equal = match (&left, &right) {
                (Value::Text(left), Value::Text(right)) => left == right,
                (Value::Integer(left), Value::Integer(right)) => left == right,
                (Value::Decimal(left), Value::Decimal(right)) => left == right,
                (Value::Integer(left), Value::Decimal(right)) => (*left as f64) == *right,
                (Value::Decimal(left), Value::Integer(right)) => *left == (*right as f64),
                (Value::Boolean(left), Value::Boolean(right)) => left == right,
                (Value::Object(left), Value::Object(right)) => left == right,
                _ => false,
            };
            return Ok(if matches!(operator, Equal) {
                equal
            } else {
                !equal
            });
        }

        let ordering = match (&left, &right) {
            (Value::Text(left), Value::Text(right)) => left.cmp(right),
            (Value::Integer(left), Value::Integer(right)) => left.cmp(right),
            (Value::Integer(left), Value::Decimal(right)) => compare_f64(*left as f64, *right),
            (Value::Decimal(left), Value::Integer(right)) => compare_f64(*left, *right as f64),
            (Value::Decimal(left), Value::Decimal(right)) => compare_f64(*left, *right),
            _ => {
                return Err(ExecutionError::IncomparableValues {
                    left_type: value_type(&left),
                    right_type: value_type(&right),
                    operator: comparison_operator_text(operator),
                });
            }
        };

        Ok(match operator {
            Less => ordering == Ordering::Less,
            LessOrEqual => ordering != Ordering::Greater,
            Greater => ordering == Ordering::Greater,
            GreaterOrEqual => ordering != Ordering::Less,
            Equal | NotEqual => unreachable!(),
        })
    }

    // {
    //   責務: [evaluate_expression: 式ASTを実行時Valueへ評価する]
    //   処理: [variable、call、standard namespace、object、field、arithmeticを解決する]
    //   引数: [expression: 評価する式AST]
    //   戻り値: [Result<Value, ExecutionError>: 式の評価値]
    //   エラー: [ExecutionError: 未定義名、戻り値なし、型不一致、または評価失敗]
    // }
    pub(super) fn evaluate_expression(
        &mut self,
        expression: &Expression,
    ) -> Result<Value, ExecutionError> {
        match expression {
            Expression::Literal(value) => Ok(Value::Text(value.clone())),
            Expression::Unary { operator, operand } => {
                let value = self.evaluate_expression(operand)?;
                self.evaluate_unary(*operator, value)
            }
            Expression::Arithmetic {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_expression(left)?;
                let right = self.evaluate_expression(right)?;
                self.evaluate_arithmetic(left, *operator, right)
            }
            Expression::Variable(name) => self
                .lookup_variable(name)
                .cloned()
                .ok_or_else(|| ExecutionError::UnknownVariable(name.clone())),
            Expression::Call { name, arguments } => {
                if let Some(value) = self.call_builtin(name, arguments)? {
                    return Ok(value);
                }
                self.call_function(name, arguments)?
                    .ok_or_else(|| ExecutionError::MissingReturnValue(name.clone()))
            }
            Expression::StandardCall {
                namespace,
                name,
                arguments,
            } => self.call_standard_namespace(*namespace, name, arguments),
            Expression::New {
                class_name,
                arguments,
            } => self.instantiate_class(class_name, arguments),
            Expression::Field { target, field } => {
                let object_id = self.resolve_object(target)?;
                self.get_field(object_id, field)
            }
            Expression::MethodCall {
                target,
                name,
                arguments,
            } => {
                let return_value = if target.eq_ignore_ascii_case("Super") {
                    self.call_super_method(name, arguments)?
                } else {
                    let object_id = self.resolve_object(target)?;
                    self.call_method(object_id, name, arguments)?
                };
                return_value
                    .ok_or_else(|| ExecutionError::MissingReturnValue(format!("{target}.{name}")))
            }
        }
    }

    // {
    //   責務: [evaluate_unary: 単項演算子を実行時値へ適用する]
    //   引数: [operator: 単項演算子, value: 演算対象]
    //   戻り値: [Result<Value, ExecutionError>: 演算結果]
    //   エラー: [ExecutionError: 非数値への適用またはInteger符号反転overflow]
    // }
    pub(super) fn evaluate_unary(
        &self,
        operator: UnaryOperator,
        value: Value,
    ) -> Result<Value, ExecutionError> {
        match operator {
            UnaryOperator::Negate => match value {
                Value::Integer(value) => value
                    .checked_neg()
                    .map(Value::Integer)
                    .ok_or(ExecutionError::ArithmeticOverflow { operator: "-" }),
                Value::Decimal(value) => Ok(Value::Decimal(-value)),
                other => Err(ExecutionError::InvalidUnaryArithmetic {
                    value_type: value_type(&other),
                    operator: "-",
                }),
            },
        }
    }

    // {
    //   責務: [evaluate_arithmetic: 算術演算子を数値値へ適用する]
    //   処理: [Integer演算はchecked arithmetic、Decimalを含む演算はfinite値を検査する]
    //   引数: [left: 左値, operator: 算術演算子, right: 右値]
    //   戻り値: [Result<Value, ExecutionError>: IntegerまたはDecimalの結果]
    //   エラー: [ExecutionError: 型不一致、0除算、または演算overflow]
    // }
    pub(super) fn evaluate_arithmetic(
        &self,
        left: Value,
        operator: ArithmeticOperator,
        right: Value,
    ) -> Result<Value, ExecutionError> {
        use ArithmeticOperator::{Add, Divide, Multiply, Subtract};

        if matches!(operator, Divide) {
            let (left_number, right_number) =
                arithmetic_numbers(&left, &right, arithmetic_operator_text(operator))?;
            if right_number == 0.0 {
                return Err(ExecutionError::DivisionByZero);
            }
            let result = left_number / right_number;
            if !result.is_finite() {
                return Err(ExecutionError::ArithmeticOverflow {
                    operator: arithmetic_operator_text(operator),
                });
            }
            return Ok(Value::Decimal(result));
        }

        match (&left, &right) {
            (Value::Integer(left), Value::Integer(right)) => {
                let result = match operator {
                    Add => left.checked_add(*right),
                    Subtract => left.checked_sub(*right),
                    Multiply => left.checked_mul(*right),
                    Divide => unreachable!(),
                };
                result
                    .map(Value::Integer)
                    .ok_or(ExecutionError::ArithmeticOverflow {
                        operator: arithmetic_operator_text(operator),
                    })
            }
            (Value::Integer(_), Value::Decimal(_))
            | (Value::Decimal(_), Value::Integer(_))
            | (Value::Decimal(_), Value::Decimal(_)) => {
                let (left_number, right_number) =
                    arithmetic_numbers(&left, &right, arithmetic_operator_text(operator))?;
                let result = match operator {
                    Add => left_number + right_number,
                    Subtract => left_number - right_number,
                    Multiply => left_number * right_number,
                    Divide => unreachable!(),
                };
                if result.is_finite() {
                    Ok(Value::Decimal(result))
                } else {
                    Err(ExecutionError::ArithmeticOverflow {
                        operator: arithmetic_operator_text(operator),
                    })
                }
            }
            _ => Err(ExecutionError::IncomparableArithmetic {
                left_type: value_type(&left),
                right_type: value_type(&right),
                operator: arithmetic_operator_text(operator),
            }),
        }
    }
}

// {
//   責務: [compare_f64: 有限f64値を比較順序へ変換する]
//   引数: [left: 左値, right: 右値]
//   戻り値: [Ordering: 数値の比較順序]
//   前提: [Decimal値は生成時に有限値として検証される]
// }
pub(super) fn compare_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right)
        .expect("Rowly DSL decimal values are always finite")
}

// {
//   責務: [arithmetic_numbers: 数値value対をf64へ変換する]
//   引数: [left: 左値, right: 右値, operator: error表示用演算子]
//   戻り値: [Result<(f64, f64), ExecutionError>: 両辺の数値]
//   エラー: [ExecutionError: いずれかのoperandがInteger/Decimalでない]
// }
pub(super) fn arithmetic_numbers(
    left: &Value,
    right: &Value,
    operator: &'static str,
) -> Result<(f64, f64), ExecutionError> {
    let number = |value: &Value| match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Decimal(value) => Some(*value),
        _ => None,
    };
    match (number(left), number(right)) {
        (Some(left), Some(right)) => Ok((left, right)),
        _ => Err(ExecutionError::IncomparableArithmetic {
            left_type: value_type(left),
            right_type: value_type(right),
            operator,
        }),
    }
}

// {
//   責務: [arithmetic_operator_text: 算術演算子を記号表現へ変換する]
//   引数: [operator: DSL算術演算子]
//   戻り値: [&'static str: +、-、*、/のいずれか]
// }
pub(super) fn arithmetic_operator_text(operator: ArithmeticOperator) -> &'static str {
    match operator {
        ArithmeticOperator::Add => "+",
        ArithmeticOperator::Subtract => "-",
        ArithmeticOperator::Multiply => "*",
        ArithmeticOperator::Divide => "/",
    }
}

// {
//   責務: [comparison_operator_text: 比較演算子を記号表現へ変換する]
//   引数: [operator: DSL比較演算子]
//   戻り値: [&'static str: 比較演算子の記号]
// }
pub(super) fn comparison_operator_text(operator: ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Equal => "=",
        ComparisonOperator::NotEqual => "!=",
        ComparisonOperator::Less => "<",
        ComparisonOperator::LessOrEqual => "<=",
        ComparisonOperator::Greater => ">",
        ComparisonOperator::GreaterOrEqual => ">=",
    }
}
