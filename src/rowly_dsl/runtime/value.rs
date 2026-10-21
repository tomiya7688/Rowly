//! 値の型変換・検査・説明を行う。

use super::*;

impl<'a> Runtime<'a> {
    // {
    //   責務: [convert_integer: 値をIntegerへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Integer値]
    //   エラー: [ExecutionError: Integerとして解釈できない値]
    // }
    pub(super) fn convert_integer(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Integer(value) => Ok(Value::Integer(value)),
            Value::Text(value) => value
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| conversion_error("String", value, "Integer")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Integer",
            )),
        }
    }

    // {
    //   責務: [convert_decimal: 値を有限Decimalへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Decimal値]
    //   エラー: [ExecutionError: 数値化できない値または非有限数]
    // }
    pub(super) fn convert_decimal(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Decimal(value) => Ok(Value::Decimal(value)),
            Value::Integer(value) => Ok(Value::Decimal(value as f64)),
            Value::Text(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Value::Decimal)
                .ok_or_else(|| conversion_error("String", value, "Decimal")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Decimal",
            )),
        }
    }

    // {
    //   責務: [convert_boolean: Boolean値またはtrue/false文字列をBooleanへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Boolean値]
    //   エラー: [ExecutionError: true/falseとして解釈できない値]
    // }
    pub(super) fn convert_boolean(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Boolean(value) => Ok(Value::Boolean(value)),
            Value::Text(value) if value.eq_ignore_ascii_case("true") => Ok(Value::Boolean(true)),
            Value::Text(value) if value.eq_ignore_ascii_case("false") => Ok(Value::Boolean(false)),
            Value::Text(value) => Err(conversion_error("String", value, "Boolean")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Boolean",
            )),
        }
    }

    // {
    //   責務: [convert_string: object以外の値を表示文字列へ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Text値]
    //   エラー: [ExecutionError: objectは暗黙に文字列化しない]
    // }
    pub(super) fn convert_string(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Object(_) => Err(ExecutionError::ExpectedText {
                context: "String conversion".to_owned(),
            }),
            value => Ok(Value::Text(self.describe_value(&value))),
        }
    }

    // {
    //   責務: [expect_integer: 値がIntegerか検査してi64を取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<i64, ExecutionError>: Integer値]
    //   エラー: [ExecutionError: 値がIntegerでない]
    // }
    pub(super) fn expect_integer(
        &self,
        value: Value,
        context: &str,
    ) -> Result<i64, ExecutionError> {
        match value {
            Value::Integer(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedInteger {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [expect_one_based_index: 正のInteger値をusizeの1-based indexとして検証する]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<usize, ExecutionError>: 1以上のindex]
    //   エラー: [ExecutionError: Integerでない、1未満、またはusize範囲外]
    // }
    pub(super) fn expect_one_based_index(
        &self,
        value: Value,
        context: &str,
    ) -> Result<usize, ExecutionError> {
        let value = self.expect_integer(value, context)?;
        if value < 1 {
            return Err(ExecutionError::InvalidOneBasedIndex {
                context: context.to_owned(),
            });
        }
        usize::try_from(value).map_err(|_| ExecutionError::InvalidOneBasedIndex {
            context: context.to_owned(),
        })
    }

    // {
    //   責務: [expect_text: 値がTextか検査してStringを取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<String, ExecutionError>: Text内容]
    //   エラー: [ExecutionError: 値がTextでない]
    // }
    pub(super) fn expect_text(
        &self,
        value: Value,
        context: &str,
    ) -> Result<String, ExecutionError> {
        match value {
            Value::Text(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedText {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [expect_boolean: 値がBooleanか検査してboolを取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<bool, ExecutionError>: Boolean値]
    //   エラー: [ExecutionError: 値がBooleanでない]
    // }
    pub(super) fn expect_boolean(
        &self,
        value: Value,
        context: &str,
    ) -> Result<bool, ExecutionError> {
        match value {
            Value::Boolean(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedBoolean {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [cell_text: scalar値をCSV cellへ書けるtextへ変換する]
    //   引数: [value: cellへ書き込む値]
    //   戻り値: [Result<String, ExecutionError>: cell文字列]
    //   エラー: [ExecutionError: object値はCSV cellへ書き込めない]
    // }
    pub(super) fn cell_text(&self, value: Value) -> Result<String, ExecutionError> {
        match value {
            Value::Text(value) => Ok(value),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Decimal(value) => Ok(value.to_string()),
            Value::Boolean(value) => Ok(value.to_string()),
            Value::Object(_) => Err(ExecutionError::ExpectedText {
                context: "cell assignment".to_owned(),
            }),
        }
    }

    // {
    //   責務: [scalar_text: scalar値を実行レポート用文字列へ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Option<String>: scalarの文字列表現。objectはNone]
    // }
    pub(super) fn scalar_text(&self, value: &Value) -> Option<String> {
        match value {
            Value::Text(value) => Some(value.clone()),
            Value::Integer(value) => Some(value.to_string()),
            Value::Decimal(value) => Some(value.to_string()),
            Value::Boolean(value) => Some(value.to_string()),
            Value::Object(_) => None,
        }
    }

    // {
    //   責務: [describe_value: eventとerrorに使う実行時値の文字列表現を作る]
    //   引数: [value: 説明する値]
    //   戻り値: [String: scalar表現またはclass名付きobject表現]
    // }
    pub(super) fn describe_value(&self, value: &Value) -> String {
        match value {
            Value::Text(value) => value.clone(),
            Value::Integer(value) => value.to_string(),
            Value::Decimal(value) => value.to_string(),
            Value::Boolean(value) => value.to_string(),
            Value::Object(object_id) => self
                .objects
                .get(*object_id)
                .map(|object| format!("<{} object>", object.class_name))
                .unwrap_or_else(|| "<object>".to_owned()),
        }
    }
}

// {
//   責務: [contains_japanese: 文字列に対象の日本語Unicode範囲が含まれるか調べる]
//   引数: [value: 検査する文字列]
//   戻り値: [bool: かな、漢字等の対象範囲が含まれればtrue]
// }
pub(super) fn contains_japanese(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '\u{3040}'..='\u{309f}'
                | '\u{30a0}'..='\u{30ff}'
                | '\u{31f0}'..='\u{31ff}'
                | '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
                | '\u{ff66}'..='\u{ff9f}'
        )
    })
}

// {
//   責務: [is_integer_value: 値がInteger、またはi64にparse可能なTextか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Integerとして解釈できればtrue]
// }
pub(super) fn is_integer_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) => true,
        Value::Text(value) => value.parse::<i64>().is_ok(),
        _ => false,
    }
}

// {
//   責務: [is_decimal_value: 値が有限数値型、またはfinite f64にparse可能なTextか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Decimalとして解釈できればtrue]
// }
pub(super) fn is_decimal_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Decimal(_) => true,
        Value::Text(value) => value.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()),
        _ => false,
    }
}

// {
//   責務: [is_boolean_value: 値がBoolean、またはtrue/false Textか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Booleanとして解釈できればtrue]
// }
pub(super) fn is_boolean_value(value: &Value) -> bool {
    match value {
        Value::Boolean(_) => true,
        Value::Text(value) => {
            value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
        }
        _ => false,
    }
}

// {
//   責務: [value_type: runtime値の型名をerror表示向けに返す]
//   引数: [value: 型名を調べる値]
//   戻り値: [&'static str: Text、Integer、Decimal、Boolean、Objectの型名]
// }
pub(super) fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Text(_) => "String",
        Value::Integer(_) => "Integer",
        Value::Decimal(_) => "Decimal",
        Value::Boolean(_) => "Boolean",
        Value::Object(_) => "Object",
    }
}

// {
//   責務: [conversion_error: 型変換失敗の詳細を持つExecutionErrorを作成する]
//   引数: [value_type: 入力型名, value: 入力表示値, target: 変換先型名]
//   戻り値: [ExecutionError: 変換元と変換先を含むConversion error]
// }
pub(super) fn conversion_error(
    value_type: &'static str,
    value: String,
    target: &'static str,
) -> ExecutionError {
    ExecutionError::Conversion {
        value_type,
        value,
        target,
    }
}
