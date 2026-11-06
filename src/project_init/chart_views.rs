//! ```text
//! 責務: [chart_views: Chart定義をgenerated init DSLの1行宣言として保存・復元する]
//! 処理: [1: 定義を `SET This.Project.Source(..).Worksheet.Chart(..).Definition = Line(Source(..), Category(..), Series([..]))` に直列化する 2: 同じ文法を読み戻す 3: Chartのdata sourceや列は保存時に検証しない。sourceや列が欠けても定義は捨てず、解決時（chart::resolve）にUnresolvedとして扱う]
//! 補足: [bare CSVにはproject DSLが無いので保存しない（session内だけ）。user.rlyは読むだけで書き換えない。保存するのは定義だけで、描画結果やCSV値は保存しない]
//! ```
use crate::process::chart::{ChartDefinition, ChartKind, SeriesDefinition};

use super::aggregation_views::{Value, argument, optional_argument, parse_value, single_text};
use super::{
    ProjectInitConfig, ProjectInitError, declaration_continues_with, parse_quoted, quote,
    strip_prefix_ci,
};

const PREFIX: &str = "SET This.Project.Source(";
const MIDDLE: &str = ").Worksheet.Chart(";
const DEFINITION: &str = ").Definition =";

impl ProjectInitConfig {
    // {
    //   責務: [update_chart_views: sourceのgenerated Chart定義を、渡された定義の集合で置き換える]
    //   処理: [名前の重複・空の名前・空のdata source idを拒否し、user.rlyが同名を定義している定義はuser側を優先して保存しない。他sourceの定義は変えない。data sourceや列が現在のCSVに存在するかは検証しない]
    //   引数: [source_id: 対象のproject source, definitions: 保存する定義の全集合, user_config: user.rlyの設定]
    //   戻り値: [Result<(), ProjectInitError>: 成功、または定義の不備]
    // }
    pub(super) fn update_chart_views(
        &mut self,
        source_id: &str,
        definitions: &[ChartDefinition],
        user_config: &ProjectInitConfig,
    ) -> Result<(), ProjectInitError> {
        let mut names = std::collections::HashSet::new();
        for definition in definitions {
            if definition.name.trim().is_empty() {
                return Err(ProjectInitError::InvalidChartDefinition(
                    "chart name must not be empty".to_owned(),
                ));
            }
            if definition.source_id.trim().is_empty() {
                return Err(ProjectInitError::InvalidChartDefinition(format!(
                    "chart `{}` must name a data source",
                    definition.name
                )));
            }
            if !names.insert(definition.name.as_str()) {
                return Err(ProjectInitError::InvalidChartDefinition(format!(
                    "chart name `{}` is duplicated",
                    definition.name
                )));
            }
        }
        self.chart_views
            .retain(|(entry_source, _), _| entry_source != source_id);
        for definition in definitions {
            let key = (source_id.to_owned(), definition.name.clone());
            if user_config.chart_views.contains_key(&key) {
                continue;
            }
            self.chart_views.insert(key, definition.clone());
        }
        Ok(())
    }

    // {
    //   責務: [source_chart_views: sourceのChart定義を名前順で返す]
    // }
    pub(super) fn source_chart_views(&self, source_id: &str) -> Vec<ChartDefinition> {
        self.chart_views
            .iter()
            .filter(|((entry_source, _), _)| entry_source == source_id)
            .map(|(_, definition)| definition.clone())
            .collect()
    }

    // {
    //   責務: [serialize_chart_views: 全Chart定義を1定義1行のDSLにする]
    // }
    pub(super) fn serialize_chart_views(&self, output: &mut String) {
        for ((source_id, name), definition) in &self.chart_views {
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

/// 行がChart定義の宣言か判定する。
pub(super) fn is_chart_declaration(line: &str) -> bool {
    declaration_continues_with(line, PREFIX, MIDDLE)
}

// {
//   責務: [parse_chart_declaration: 1行の宣言を(project source id, 定義)に読み戻す]
//   戻り値: [Result<(String, ChartDefinition), String>: 成功、または文法error]
// }
pub(super) fn parse_chart_declaration(line: &str) -> Result<(String, ChartDefinition), String> {
    let rest = strip_prefix_ci(line, PREFIX).ok_or("expected a chart declaration")?;
    let (source_id, rest) = parse_quoted(rest).ok_or("expected a quoted project source id")?;
    if source_id.trim().is_empty() {
        return Err("project source id must not be empty".to_owned());
    }
    let rest = strip_prefix_ci(rest, MIDDLE).ok_or("expected `.Worksheet.Chart(`")?;
    let (name, rest) = parse_quoted(rest).ok_or("expected a quoted chart name")?;
    if name.trim().is_empty() {
        return Err("chart name must not be empty".to_owned());
    }
    let rest = strip_prefix_ci(rest, DEFINITION).ok_or("expected `).Definition =`")?;
    let (value, rest) = parse_value(rest)?;
    if !rest.trim().is_empty() {
        return Err("unexpected text after the chart definition".to_owned());
    }
    Ok((source_id, interpret(name, &value)?))
}

fn interpret(name: String, value: &Value) -> Result<ChartDefinition, String> {
    let Value::Call(kind_name, arguments) = value else {
        return Err("expected a chart kind such as Line(..)".to_owned());
    };
    let kind = match kind_name.to_ascii_lowercase().as_str() {
        "line" => ChartKind::Line,
        "bar" => ChartKind::Bar,
        "column" => ChartKind::Column,
        "area" => ChartKind::Area,
        "scatter" => ChartKind::Scatter,
        "pie" => ChartKind::Pie,
        _ => return Err(format!("unknown chart kind `{kind_name}`")),
    };
    let source_id = single_text(argument(arguments, "Source")?)?;
    let category_header = match optional_argument(arguments, "Category")? {
        [] => None,
        inner => Some(single_text(inner)?),
    };
    let [Value::List(items)] = argument(arguments, "Series")? else {
        return Err("expected Series([..])".to_owned());
    };
    let series = items
        .iter()
        .map(|item| match item {
            Value::Call(call, arguments) if call.eq_ignore_ascii_case("Series") => {
                match arguments.as_slice() {
                    [Value::Text(header)] => Ok(SeriesDefinition {
                        name: None,
                        header: header.clone(),
                    }),
                    [Value::Text(header), Value::Text(name)] => Ok(SeriesDefinition {
                        name: Some(name.clone()),
                        header: header.clone(),
                    }),
                    _ => Err("Series expects a header and an optional name".to_owned()),
                }
            }
            _ => Err("expected Series(\"header\") in the series list".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChartDefinition {
        name,
        kind,
        source_id,
        category_header,
        series,
    })
}

fn serialize_definition(definition: &ChartDefinition) -> String {
    let kind = match definition.kind {
        ChartKind::Line => "Line",
        ChartKind::Bar => "Bar",
        ChartKind::Column => "Column",
        ChartKind::Area => "Area",
        ChartKind::Scatter => "Scatter",
        ChartKind::Pie => "Pie",
    };
    let category = definition
        .category_header
        .as_ref()
        .map(|header| format!(", Category({})", quote(header)))
        .unwrap_or_default();
    let series: Vec<String> = definition
        .series
        .iter()
        .map(|series| match &series.name {
            Some(name) => format!("Series({}, {})", quote(&series.header), quote(name)),
            None => format!("Series({})", quote(&series.header)),
        })
        .collect();
    format!(
        "{kind}(Source({}){category}, Series([{}]))",
        quote(&definition.source_id),
        series.join(", ")
    )
}
