//! ```text
//! 責務: [aggregation_views: Group By / Pivot定義をgenerated init DSLの1行宣言として保存・復元する]
//! 処理: [1: 定義を `SET This.Project.Source(..).Worksheet.Aggregation(..).Definition = GroupBy(..)|Pivot(..)` に直列化する 2: 同じ文法を読み戻す 3: 列は見出し名で持つので、列が欠けても定義は捨てず、解決時に休眠させる]
//! 補足: [bare CSVにはproject DSLが無いので保存しない（session内だけ）。user.rlyは読むだけで書き換えない]
//! ```
use std::collections::BTreeMap;

use crate::process::{
    aggregation_definition::{
        AggregateDefinition, AggregationDefinition, AggregationKind, FilterDefinition, InputScope,
    },
    calculation::AggregateFunction,
};

use super::{ProjectInitConfig, ProjectInitError, parse_quoted, quote, strip_prefix_ci};

/// 宣言の文法。
const PREFIX: &str = "SET This.Project.Source(";
const MIDDLE: &str = ").Worksheet.Aggregation(";
const DEFINITION: &str = ").Definition =";

/// DSLの値。文字列・リスト・名前付き呼び出しだけを持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Text(String),
    List(Vec<Value>),
    Call(String, Vec<Value>),
}

impl ProjectInitConfig {
    // {
    //   責務: [update_aggregation_views: sourceのgenerated定義を、渡された定義の集合で置き換える]
    //   処理: [名前の重複・空の名前を拒否し、user.rlyが同名を定義している定義はuser側を優先して保存しない。他sourceの定義は変えない]
    //   引数: [source_id: 対象source, definitions: 保存する定義の全集合, user_config: user.rlyの設定]
    //   戻り値: [Result<(), ProjectInitError>: 成功、または名前の不備]
    // }
    pub(super) fn update_aggregation_views(
        &mut self,
        source_id: &str,
        definitions: &[AggregationDefinition],
        user_config: &ProjectInitConfig,
    ) -> Result<(), ProjectInitError> {
        let mut names = std::collections::HashSet::new();
        for definition in definitions {
            if definition.name.trim().is_empty() {
                return Err(ProjectInitError::InvalidAggregationDefinition(
                    "aggregation definition name must not be empty".to_owned(),
                ));
            }
            if !names.insert(definition.name.as_str()) {
                return Err(ProjectInitError::InvalidAggregationDefinition(format!(
                    "aggregation definition name `{}` is duplicated",
                    definition.name
                )));
            }
        }
        self.aggregation_views
            .retain(|(entry_source, _), _| entry_source != source_id);
        for definition in definitions {
            let key = (source_id.to_owned(), definition.name.clone());
            if user_config.aggregation_views.contains_key(&key) {
                continue;
            }
            self.aggregation_views.insert(key, definition.clone());
        }
        Ok(())
    }

    // {
    //   責務: [source_aggregation_views: sourceの定義を名前順で返す]
    // }
    pub(super) fn source_aggregation_views(&self, source_id: &str) -> Vec<AggregationDefinition> {
        self.aggregation_views
            .iter()
            .filter(|((entry_source, _), _)| entry_source == source_id)
            .map(|(_, definition)| definition.clone())
            .collect()
    }

    // {
    //   責務: [serialize_aggregation_views: 全定義を1定義1行のDSLにする]
    // }
    pub(super) fn serialize_aggregation_views(&self, output: &mut String) {
        for ((source_id, name), definition) in &self.aggregation_views {
            output.push_str(PREFIX);
            output.push_str(&quote(source_id));
            output.push_str(MIDDLE);
            output.push_str(&quote(name));
            output.push_str(DEFINITION);
            output.push(' ');
            output.push_str(&serialize_definition(definition));
            output.push('\n');
        }
    }
}

/// 行が集計定義の宣言か判定する。
pub(super) fn is_aggregation_declaration(line: &str) -> bool {
    line.to_ascii_lowercase()
        .starts_with(&format!("{}\"", PREFIX.to_ascii_lowercase()))
        && line
            .to_ascii_lowercase()
            .contains(".worksheet.aggregation(")
}

// {
//   責務: [parse_aggregation_declaration: 1行の宣言を(source id, 定義)に読み戻す]
//   戻り値: [Result<(String, AggregationDefinition), String>: 成功、または文法error]
// }
pub(super) fn parse_aggregation_declaration(
    line: &str,
) -> Result<(String, AggregationDefinition), String> {
    let rest = strip_prefix_ci(line, PREFIX).ok_or("expected an aggregation declaration")?;
    let (source_id, rest) = parse_quoted(rest).ok_or("expected a quoted project source id")?;
    if source_id.trim().is_empty() {
        return Err("project source id must not be empty".to_owned());
    }
    let rest = strip_prefix_ci(rest, MIDDLE).ok_or("expected `.Worksheet.Aggregation(`")?;
    let (name, rest) = parse_quoted(rest).ok_or("expected a quoted aggregation name")?;
    if name.trim().is_empty() {
        return Err("aggregation name must not be empty".to_owned());
    }
    let rest = strip_prefix_ci(rest, DEFINITION).ok_or("expected `).Definition =`")?;
    let (value, rest) = parse_value(rest)?;
    if !rest.trim().is_empty() {
        return Err("unexpected text after the aggregation definition".to_owned());
    }
    Ok((source_id, interpret(name, &value)?))
}

fn parse_value(input: &str) -> Result<(Value, &str), String> {
    let input = input.trim_start();
    if input.starts_with('"') {
        let (text, rest) = parse_quoted(input).ok_or("unterminated string")?;
        return Ok((Value::Text(text), rest));
    }
    if let Some(rest) = input.strip_prefix('[') {
        let (items, rest) = parse_sequence(rest, ']')?;
        return Ok((Value::List(items), rest));
    }
    let end = input
        .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .unwrap_or(input.len());
    if end == 0 {
        return Err("expected a string, list or function name".to_owned());
    }
    let (name, rest) = input.split_at(end);
    let rest = rest
        .trim_start()
        .strip_prefix('(')
        .ok_or_else(|| format!("expected `(` after `{name}`"))?;
    let (arguments, rest) = parse_sequence(rest, ')')?;
    Ok((Value::Call(name.to_owned(), arguments), rest))
}

fn parse_sequence(mut input: &str, close: char) -> Result<(Vec<Value>, &str), String> {
    let mut items = Vec::new();
    loop {
        input = input.trim_start();
        if let Some(rest) = input.strip_prefix(close) {
            return Ok((items, rest));
        }
        if !items.is_empty() {
            input = input
                .strip_prefix(',')
                .ok_or_else(|| format!("expected `,` or `{close}`"))?;
            input = input.trim_start();
            if let Some(rest) = input.strip_prefix(close) {
                return Ok((items, rest));
            }
        }
        let (item, rest) = parse_value(input)?;
        items.push(item);
        input = rest;
    }
}

/// 名前付き引数を1つだけ取り出す。
fn argument<'a>(arguments: &'a [Value], name: &str) -> Result<&'a [Value], String> {
    let mut found = arguments.iter().filter_map(|value| match value {
        Value::Call(call, inner) if call.eq_ignore_ascii_case(name) => Some(inner.as_slice()),
        _ => None,
    });
    let first = found
        .next()
        .ok_or_else(|| format!("missing `{name}(..)`"))?;
    if found.next().is_some() {
        return Err(format!("`{name}(..)` appears more than once"));
    }
    Ok(first)
}

fn optional_argument<'a>(arguments: &'a [Value], name: &str) -> Result<&'a [Value], String> {
    if arguments
        .iter()
        .any(|value| matches!(value, Value::Call(call, _) if call.eq_ignore_ascii_case(name)))
    {
        argument(arguments, name)
    } else {
        Ok(&[])
    }
}

fn text_list(inner: &[Value]) -> Result<Vec<String>, String> {
    match inner {
        [Value::List(items)] => items
            .iter()
            .map(|item| match item {
                Value::Text(text) => Ok(text.clone()),
                _ => Err("expected a list of quoted strings".to_owned()),
            })
            .collect(),
        _ => Err("expected one list of quoted strings".to_owned()),
    }
}

fn single_text(inner: &[Value]) -> Result<String, String> {
    match inner {
        [Value::Text(text)] => Ok(text.clone()),
        _ => Err("expected one quoted string".to_owned()),
    }
}

fn aggregate_list(inner: &[Value]) -> Result<Vec<AggregateDefinition>, String> {
    let [Value::List(items)] = inner else {
        return Err("expected one list of aggregates".to_owned());
    };
    items
        .iter()
        .map(|item| {
            let Value::Call(name, arguments) = item else {
                return Err("expected an aggregate such as Sum(\"header\")".to_owned());
            };
            if name.eq_ignore_ascii_case("CountRows") {
                return if arguments.is_empty() {
                    Ok(AggregateDefinition::CountRows)
                } else {
                    Err("CountRows() takes no arguments".to_owned())
                };
            }
            let header = single_text(arguments)?;
            if name.eq_ignore_ascii_case("DistinctCount") {
                return Ok(AggregateDefinition::DistinctCount { header });
            }
            let function = match name.to_ascii_lowercase().as_str() {
                "sum" => AggregateFunction::Sum,
                "count" => AggregateFunction::Count,
                "counta" => AggregateFunction::CountA,
                "average" => AggregateFunction::Average,
                "min" => AggregateFunction::Min,
                "max" => AggregateFunction::Max,
                _ => return Err(format!("unsupported aggregate `{name}`")),
            };
            Ok(AggregateDefinition::Column { function, header })
        })
        .collect()
}

fn filter_list(inner: &[Value]) -> Result<Vec<FilterDefinition>, String> {
    let [Value::List(items)] = inner else {
        return Err("expected one list of filters".to_owned());
    };
    items
        .iter()
        .map(|item| match item {
            Value::Call(name, arguments) if name.eq_ignore_ascii_case("Filter") => {
                let [Value::Text(header), allowed] = arguments.as_slice() else {
                    return Err("Filter expects a header and a list of values".to_owned());
                };
                Ok(FilterDefinition {
                    header: header.clone(),
                    allowed: text_list(std::slice::from_ref(allowed))?,
                })
            }
            _ => Err("expected Filter(\"header\", [..])".to_owned()),
        })
        .collect()
}

fn interpret(name: String, value: &Value) -> Result<AggregationDefinition, String> {
    let Value::Call(kind, arguments) = value else {
        return Err("expected GroupBy(..) or Pivot(..)".to_owned());
    };
    let scope = match single_text(argument(arguments, "Scope")?)?.as_str() {
        "AllRows" => InputScope::AllRows,
        "ViewFiltered" => InputScope::ViewFiltered,
        "Selection" => InputScope::Selection,
        other => return Err(format!("unknown input scope `{other}`")),
    };
    let kind = if kind.eq_ignore_ascii_case("GroupBy") {
        AggregationKind::GroupBy {
            keys: text_list(argument(arguments, "Keys")?)?,
            aggregates: aggregate_list(argument(arguments, "Aggregates")?)?,
        }
    } else if kind.eq_ignore_ascii_case("Pivot") {
        AggregationKind::Pivot {
            rows: text_list(argument(arguments, "Rows")?)?,
            columns: text_list(argument(arguments, "Columns")?)?,
            values: aggregate_list(argument(arguments, "Values")?)?,
            filters: match optional_argument(arguments, "Filters")? {
                [] => Vec::new(),
                inner => filter_list(inner)?,
            },
        }
    } else {
        return Err(format!("unknown aggregation kind `{kind}`"));
    };
    Ok(AggregationDefinition { name, scope, kind })
}

fn serialize_texts(texts: &[String]) -> String {
    let items: Vec<String> = texts.iter().map(|text| quote(text)).collect();
    format!("[{}]", items.join(", "))
}

fn serialize_aggregates(definitions: &[AggregateDefinition]) -> String {
    let items: Vec<String> = definitions
        .iter()
        .map(|definition| match definition {
            AggregateDefinition::CountRows => "CountRows()".to_owned(),
            AggregateDefinition::DistinctCount { header } => {
                format!("DistinctCount({})", quote(header))
            }
            AggregateDefinition::Column { function, header } => {
                let name = match function {
                    AggregateFunction::Sum => "Sum",
                    AggregateFunction::Count => "Count",
                    AggregateFunction::CountA => "CountA",
                    AggregateFunction::Average => "Average",
                    AggregateFunction::Min => "Min",
                    AggregateFunction::Max => "Max",
                };
                format!("{name}({})", quote(header))
            }
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn serialize_definition(definition: &AggregationDefinition) -> String {
    let scope = match definition.scope {
        InputScope::AllRows => "AllRows",
        InputScope::ViewFiltered => "ViewFiltered",
        InputScope::Selection => "Selection",
    };
    match &definition.kind {
        AggregationKind::GroupBy { keys, aggregates } => format!(
            "GroupBy(Scope(\"{scope}\"), Keys({}), Aggregates({}))",
            serialize_texts(keys),
            serialize_aggregates(aggregates)
        ),
        AggregationKind::Pivot {
            rows,
            columns,
            values,
            filters,
        } => {
            let filters: Vec<String> = filters
                .iter()
                .map(|filter| {
                    format!(
                        "Filter({}, {})",
                        quote(&filter.header),
                        serialize_texts(&filter.allowed)
                    )
                })
                .collect();
            format!(
                "Pivot(Scope(\"{scope}\"), Rows({}), Columns({}), Values({}), Filters([{}]))",
                serialize_texts(rows),
                serialize_texts(columns),
                serialize_aggregates(values),
                filters.join(", ")
            )
        }
    }
}

/// 集計定義のmap型。
pub(super) type AggregationViews = BTreeMap<(String, String), AggregationDefinition>;
