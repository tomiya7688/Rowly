//! 組み込み関数と標準namespaceを呼び出す。

use super::*;

impl<'a> Runtime<'a> {
    // {
    //   責務: [call_statement_builtin: transaction制御関数を文として処理する]
    //   処理: [引数なしを確認し、CsvDocumentのbegin/commit/rollbackへ委譲する]
    //   引数: [name: 呼び出し名, arguments: DSL引数式列]
    //   戻り値: [Result<bool, ExecutionError>: 対応builtinとして処理したか]
    //   エラー: [ExecutionError: 引数数またはdocument transaction操作の失敗]
    // }
    pub(super) fn call_statement_builtin(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<bool, ExecutionError> {
        let canonical = if name.eq_ignore_ascii_case("begintransaction") {
            Some("BeginTransaction")
        } else if name.eq_ignore_ascii_case("committransaction") {
            Some("CommitTransaction")
        } else if name.eq_ignore_ascii_case("rollbacktransaction") {
            Some("RollbackTransaction")
        } else {
            None
        };

        let Some(canonical) = canonical else {
            return Ok(false);
        };
        if !arguments.is_empty() {
            return Err(ExecutionError::ArgumentCount {
                name: canonical.to_owned(),
                expected: 0,
                actual: arguments.len(),
            });
        }

        match canonical {
            "BeginTransaction" => self.document.begin_transaction()?,
            "CommitTransaction" => self.document.commit_transaction()?,
            "RollbackTransaction" => self.document.rollback_transaction()?,
            _ => unreachable!(),
        }
        Ok(true)
    }

    // {
    //   責務: [call_standard_namespace: Text/Number/Boolean標準関数を評価する]
    //   引数: [namespace: 標準namespace, name: 関数名, arguments: 引数式列]
    //   戻り値: [Result<Value, ExecutionError>: 標準関数の結果]
    //   エラー: [ExecutionError: 未知関数、引数数、引数型、または変換の失敗]
    // }
    pub(super) fn call_standard_namespace(
        &mut self,
        namespace: StandardNamespace,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let namespace_name = match namespace {
            StandardNamespace::Text => "Text",
            StandardNamespace::Number => "Number",
            StandardNamespace::Boolean => "Boolean",
        };
        let canonical = match namespace {
            StandardNamespace::Text => {
                if name.eq_ignore_ascii_case("contains") {
                    Some(("Contains", 2))
                } else if name.eq_ignore_ascii_case("startswith") {
                    Some(("StartsWith", 2))
                } else if name.eq_ignore_ascii_case("endswith") {
                    Some(("EndsWith", 2))
                } else if name.eq_ignore_ascii_case("isjapanese") {
                    Some(("IsJapanese", 1))
                } else {
                    None
                }
            }
            StandardNamespace::Number => {
                if name.eq_ignore_ascii_case("isinteger") {
                    Some(("IsInteger", 1))
                } else if name.eq_ignore_ascii_case("isdecimal") {
                    Some(("IsDecimal", 1))
                } else {
                    None
                }
            }
            StandardNamespace::Boolean => {
                if name.eq_ignore_ascii_case("isvalid") {
                    Some(("IsValid", 1))
                } else {
                    None
                }
            }
        };
        let (canonical, expected) =
            canonical.ok_or_else(|| ExecutionError::UnknownStandardFunction {
                namespace: namespace_name,
                name: name.to_owned(),
            })?;
        let full_name = format!("{namespace_name}.{canonical}");
        if arguments.len() != expected {
            return Err(ExecutionError::ArgumentCount {
                name: full_name,
                expected,
                actual: arguments.len(),
            });
        }

        let mut values = self.evaluate_arguments(arguments)?.into_iter();
        match (namespace, canonical) {
            (StandardNamespace::Text, "Contains") => {
                let haystack = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.Contains first argument",
                )?;
                let needle = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.Contains second argument",
                )?;
                Ok(Value::Boolean(haystack.contains(&needle)))
            }
            (StandardNamespace::Text, "StartsWith") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.StartsWith first argument",
                )?;
                let prefix = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.StartsWith second argument",
                )?;
                Ok(Value::Boolean(value.starts_with(&prefix)))
            }
            (StandardNamespace::Text, "EndsWith") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.EndsWith first argument",
                )?;
                let suffix = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.EndsWith second argument",
                )?;
                Ok(Value::Boolean(value.ends_with(&suffix)))
            }
            (StandardNamespace::Text, "IsJapanese") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.IsJapanese argument",
                )?;
                Ok(Value::Boolean(contains_japanese(&value)))
            }
            (StandardNamespace::Number, "IsInteger") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_integer_value(&value)))
            }
            (StandardNamespace::Number, "IsDecimal") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_decimal_value(&value)))
            }
            (StandardNamespace::Boolean, "IsValid") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_boolean_value(&value)))
            }
            _ => unreachable!(),
        }
    }

    // {
    //   責務: [call_builtin: 型変換、CSV読み書き、行列数等の組み込み関数を評価する]
    //   処理: [大文字小文字を無視して名前を解決し、引数評価後にdocument操作を行う]
    //   引数: [name: 呼び出し名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: builtin結果。対象外の名前はNone]
    //   エラー: [ExecutionError: 引数数、型、参照、変換、またはdocument操作の失敗]
    // }
    pub(super) fn call_builtin(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        let canonical = if name.eq_ignore_ascii_case("integer") {
            Some("Integer")
        } else if name.eq_ignore_ascii_case("decimal") {
            Some("Decimal")
        } else if name.eq_ignore_ascii_case("boolean") {
            Some("Boolean")
        } else if name.eq_ignore_ascii_case("string") {
            Some("String")
        } else if name.eq_ignore_ascii_case("cellvalue") {
            Some("CellValue")
        } else if name.eq_ignore_ascii_case("rowcount") {
            Some("RowCount")
        } else if name.eq_ignore_ascii_case("columncount") {
            Some("ColumnCount")
        } else if name.eq_ignore_ascii_case("cellvalueat") {
            Some("CellValueAt")
        } else if name.eq_ignore_ascii_case("setcellvalueat") {
            Some("SetCellValueAt")
        } else if name.eq_ignore_ascii_case("columnindex") {
            Some("ColumnIndex")
        } else if name.eq_ignore_ascii_case("cellvaluebyheader") {
            Some("CellValueByHeader")
        } else if name.eq_ignore_ascii_case("setcellvaluebyheader") {
            Some("SetCellValueByHeader")
        } else {
            None
        };

        let Some(canonical) = canonical else {
            return Ok(None);
        };
        let expected = match canonical {
            "RowCount" | "ColumnCount" => 0,
            "CellValueAt" | "CellValueByHeader" => 2,
            "SetCellValueAt" | "SetCellValueByHeader" => 3,
            _ => 1,
        };
        if arguments.len() != expected {
            return Err(ExecutionError::ArgumentCount {
                name: canonical.to_owned(),
                expected,
                actual: arguments.len(),
            });
        }

        let mut values = self.evaluate_arguments(arguments)?.into_iter();
        Ok(Some(match canonical {
            "Integer" => {
                self.convert_integer(values.next().expect("validated builtin function arity"))?
            }
            "Decimal" => {
                self.convert_decimal(values.next().expect("validated builtin function arity"))?
            }
            "Boolean" => {
                self.convert_boolean(values.next().expect("validated builtin function arity"))?
            }
            "String" => {
                self.convert_string(values.next().expect("validated builtin function arity"))?
            }
            "CellValue" => {
                let reference = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "CellValue argument",
                )?;
                let value = self
                    .document
                    .cell_a1(&reference)?
                    .map(str::to_owned)
                    .ok_or(ExecutionError::MissingCell(reference))?;
                Value::Text(value)
            }
            "RowCount" => Value::Integer(self.document.row_count() as i64),
            "ColumnCount" => Value::Integer(self.document.column_count() as i64),
            "CellValueAt" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueAt row",
                )?;
                let column = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueAt column",
                )?;
                let value = self.document.cell(row - 1, column - 1).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, column {column}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueAt" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueAt row",
                )?;
                let column = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueAt column",
                )?;
                let value =
                    self.cell_text(values.next().expect("validated builtin function arity"))?;
                self.document.set_cell(row - 1, column - 1, value.clone())?;
                Value::Text(value)
            }
            "ColumnIndex" => {
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "ColumnIndex header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                Value::Integer((column + 1) as i64)
            }
            "CellValueByHeader" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueByHeader row",
                )?;
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "CellValueByHeader header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                let value = self.document.cell(row - 1, column).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, header {header}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueByHeader" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueByHeader row",
                )?;
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueByHeader header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                let value =
                    self.cell_text(values.next().expect("validated builtin function arity"))?;
                self.document.set_cell(row - 1, column, value.clone())?;
                Value::Text(value)
            }
            _ => unreachable!(),
        }))
    }
}
