//! ```text
//! 責務: [display_views: 列の表示書式・View style・行高・条件付き書式をgenerated init DSLの1行宣言として保存・復元する]
//! 処理: [1: 設定を `SET This.Project.Source(..).Worksheet.Display.<Format|Style|RowHeight|Conditional>(key) = ..` に直列化する 2: 同じ文法を読み戻し、style・条件付き書式・行高は各modelの検証を通す 3: 列は見出し名で持つので、列が欠けても設定は捨てず、解決時（display_settings::resolve）に休眠させる]
//! 補足: [bare CSVにはproject DSLが無いので保存しない（session内だけ）。user.rlyは読むだけで書き換えない。保存するのは設定だけで、CSVにも側のfileにも表示結果は保存しない]
//! ```
use std::collections::{BTreeMap, HashSet};

use crate::process::{
    conditional_format::{
        CompareOp, ConditionalFormat, ConditionalRule, Literal, Predicate, ValueSource,
    },
    display_format::{DateStyle, DisplayFormat},
    display_settings::{ColumnFormat, DisplaySettings},
    view_style::{
        BorderEdge, BorderEdgeSetting, BorderLine, BorderStyle, CellStyle, Color, HorizontalAlign,
        StyleRule, StyleTarget, TextOverflow, VerticalAlign, ViewStyle,
    },
};

use super::aggregation_views::{Value, argument, optional_argument, parse_value, single_text};
use super::{ProjectInitError, parse_quoted, quote, strip_prefix_ci};

const PREFIX: &str = "SET This.Project.Source(";
const MIDDLE: &str = ").Worksheet.Display.";

/// 1行の宣言が表す内容。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum DisplayItem {
    Format {
        column: String,
        format: DisplayFormat,
    },
    Height {
        row: usize,
        millimeters: f64,
    },
    Style {
        index: usize,
        rule: StyleRule,
    },
    Conditional {
        index: usize,
        rule: ConditionalRule,
    },
}

/// 全sourceの表示設定。
#[derive(Debug, Default, Clone)]
pub(super) struct DisplayStore {
    formats: BTreeMap<(String, String), DisplayFormat>,
    heights: BTreeMap<(String, usize), f64>,
    styles: BTreeMap<(String, usize), StyleRule>,
    conditionals: BTreeMap<(String, usize), ConditionalRule>,
}

impl DisplayStore {
    // {
    //   責務: [insert: 読み戻した1行をsourceの設定へ入れる。同じキーは後の行が勝つ]
    // }
    pub(super) fn insert(&mut self, source_id: String, item: DisplayItem) {
        match item {
            DisplayItem::Format { column, format } => {
                self.formats.insert((source_id, column), format);
            }
            DisplayItem::Height { row, millimeters } => {
                self.heights.insert((source_id, row), millimeters);
            }
            DisplayItem::Style { index, rule } => {
                self.styles.insert((source_id, index), rule);
            }
            DisplayItem::Conditional { index, rule } => {
                self.conditionals.insert((source_id, index), rule);
            }
        }
    }

    // {
    //   責務: [extend: 別ファイル（generated同士）の設定を重ねる。キーごとに後が勝つ]
    // }
    pub(super) fn extend(&mut self, other: DisplayStore) {
        self.formats.extend(other.formats);
        self.heights.extend(other.heights);
        self.styles.extend(other.styles);
        self.conditionals.extend(other.conditionals);
    }

    // {
    //   責務: [apply_user_overrides: user.rlyの設定をgeneratedの上に重ねる]
    //   処理: [列の表示書式と行高はキー（見出し・行位置）ごとにuserが勝つ。style ruleと条件付き書式は順序が意味を持つので、userが1つでも定義したsourceではそのカテゴリのgeneratedを使わない]
    // }
    pub(super) fn apply_user_overrides(&mut self, user: DisplayStore) {
        self.formats.extend(user.formats);
        self.heights.extend(user.heights);
        let style_sources: HashSet<&String> =
            user.styles.keys().map(|(source, _)| source).collect();
        self.styles
            .retain(|(source, _), _| !style_sources.contains(source));
        let conditional_sources: HashSet<&String> =
            user.conditionals.keys().map(|(source, _)| source).collect();
        self.conditionals
            .retain(|(source, _), _| !conditional_sources.contains(source));
        self.styles.extend(user.styles);
        self.conditionals.extend(user.conditionals);
    }

    // {
    //   責務: [update: sourceのgenerated設定を、渡された設定一式で置き換える]
    //   処理: [重複・空の見出しとmodelの検証に通らないstyle・条件付き書式・行高を拒否する。user.rlyが定義している項目（書式・行高はキー単位、style・条件付き書式はsource単位）はuser側を優先して保存しない。他sourceは変えない。列が現在のCSVにあるかは検証しない]
    //   引数: [source_id: 対象source, settings: 保存する設定の全集合, user: user.rlyの設定]
    // }
    pub(super) fn update(
        &mut self,
        source_id: &str,
        settings: &DisplaySettings,
        user: &DisplayStore,
    ) -> Result<(), ProjectInitError> {
        let invalid = |message: String| ProjectInitError::InvalidDisplaySettings(message);
        let mut columns = HashSet::new();
        for entry in &settings.formats {
            if entry.column.trim().is_empty() {
                return Err(invalid("format column must not be empty".to_owned()));
            }
            if !columns.insert(entry.column.as_str()) {
                return Err(invalid(format!(
                    "column `{}` has more than one display format",
                    entry.column
                )));
            }
        }
        let mut checked = ViewStyle::default();
        for rule in &settings.style.rules {
            checked
                .add_rule(rule.clone())
                .map_err(|error| invalid(error.to_string()))?;
        }
        for (row, height) in &settings.style.row_heights {
            checked
                .set_row_height(*row, *height)
                .map_err(|error| invalid(error.to_string()))?;
        }
        let mut conditional = ConditionalFormat::default();
        for rule in &settings.conditional.rules {
            conditional
                .add_rule(rule.clone())
                .map_err(|error| invalid(error.to_string()))?;
        }

        self.formats.retain(|(source, _), _| source != source_id);
        self.heights.retain(|(source, _), _| source != source_id);
        self.styles.retain(|(source, _), _| source != source_id);
        self.conditionals
            .retain(|(source, _), _| source != source_id);
        for entry in &settings.formats {
            let key = (source_id.to_owned(), entry.column.clone());
            if !user.formats.contains_key(&key) {
                self.formats.insert(key, entry.format.clone());
            }
        }
        for (row, height) in &settings.style.row_heights {
            let key = (source_id.to_owned(), *row);
            if !user.heights.contains_key(&key) {
                self.heights.insert(key, *height);
            }
        }
        if !user.styles.keys().any(|(source, _)| source == source_id) {
            for (index, rule) in settings.style.rules.iter().enumerate() {
                self.styles
                    .insert((source_id.to_owned(), index), rule.clone());
            }
        }
        if !user
            .conditionals
            .keys()
            .any(|(source, _)| source == source_id)
        {
            for (index, rule) in settings.conditional.rules.iter().enumerate() {
                self.conditionals
                    .insert((source_id.to_owned(), index), rule.clone());
            }
        }
        Ok(())
    }

    // {
    //   責務: [source_settings: sourceの設定一式を返す]
    //   処理: [列の表示書式は見出し順、style・条件付き書式は保存した順序で返す]
    // }
    pub(super) fn source_settings(&self, source_id: &str) -> DisplaySettings {
        let mut settings = DisplaySettings::default();
        for ((source, column), format) in &self.formats {
            if source == source_id {
                settings.formats.push(ColumnFormat {
                    column: column.clone(),
                    format: format.clone(),
                });
            }
        }
        for ((source, row), height) in &self.heights {
            if source == source_id {
                settings.style.row_heights.insert(*row, *height);
            }
        }
        for ((source, _), rule) in &self.styles {
            if source == source_id {
                settings.style.rules.push(rule.clone());
            }
        }
        for ((source, _), rule) in &self.conditionals {
            if source == source_id {
                settings.conditional.rules.push(rule.clone());
            }
        }
        settings
    }

    // {
    //   責務: [serialize: 全設定を1項目1行のDSLにする]
    // }
    pub(super) fn serialize(&self, output: &mut String) {
        for ((source_id, column), format) in &self.formats {
            line(
                output,
                source_id,
                "Format",
                &quote(column),
                &serialize_format(format),
            );
        }
        for ((source_id, row), height) in &self.heights {
            line(
                output,
                source_id,
                "RowHeight",
                &quote(&row.to_string()),
                &format!("Millimeters({})", quote(&height.to_string())),
            );
        }
        for ((source_id, index), rule) in &self.styles {
            let body = format!(
                "Rule(Target({}), {})",
                serialize_target(&rule.target),
                serialize_style(&rule.style)
            );
            line(
                output,
                source_id,
                "Style",
                &quote(&index.to_string()),
                &body,
            );
        }
        for ((source_id, index), rule) in &self.conditionals {
            let body = format!(
                "Rule(Target({}), Source({}), When({}), {}, Stop({}))",
                serialize_target(&rule.target),
                quote(name_of(&SOURCES, rule.source)),
                serialize_predicate(&rule.predicate),
                serialize_style(&rule.style),
                quote(&rule.stop_if_true.to_string())
            );
            line(
                output,
                source_id,
                "Conditional",
                &quote(&index.to_string()),
                &body,
            );
        }
    }
}

fn line(output: &mut String, source_id: &str, kind: &str, key: &str, body: &str) {
    output.push_str(PREFIX);
    output.push_str(&quote(source_id));
    output.push_str(MIDDLE);
    output.push_str(kind);
    output.push('(');
    output.push_str(key);
    output.push_str(") = ");
    output.push_str(body);
    output.push('\n');
}

/// 行が表示設定の宣言か判定する。
pub(super) fn is_display_declaration(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with(&format!("{}\"", PREFIX.to_ascii_lowercase()))
        && lower.contains(&MIDDLE.to_ascii_lowercase())
}

// {
//   責務: [parse_display_declaration: 1行の宣言を(project source id, 内容)に読み戻す]
//   戻り値: [Result<(String, DisplayItem), String>: 成功、または文法・検証error]
// }
pub(super) fn parse_display_declaration(line: &str) -> Result<(String, DisplayItem), String> {
    let rest = strip_prefix_ci(line, PREFIX).ok_or("expected a display declaration")?;
    let (source_id, rest) = parse_quoted(rest).ok_or("expected a quoted project source id")?;
    if source_id.trim().is_empty() {
        return Err("project source id must not be empty".to_owned());
    }
    let rest = strip_prefix_ci(rest, MIDDLE).ok_or("expected `).Worksheet.Display.`")?;
    let open = rest
        .find('(')
        .ok_or("expected `(` after the display kind")?;
    let kind = rest[..open].to_ascii_lowercase();
    let (key, rest) = parse_quoted(&rest[open + 1..]).ok_or("expected a quoted key")?;
    let rest = strip_prefix_ci(rest, ")").ok_or("expected `)` after the key")?;
    let rest = rest
        .trim_start()
        .strip_prefix('=')
        .ok_or("expected `=` after the key")?;
    let (value, rest) = parse_value(rest)?;
    if !rest.trim().is_empty() {
        return Err("unexpected text after the display setting".to_owned());
    }
    let item = match kind.as_str() {
        "format" => {
            if key.trim().is_empty() {
                return Err("format column must not be empty".to_owned());
            }
            DisplayItem::Format {
                column: key,
                format: interpret_format(&value)?,
            }
        }
        "rowheight" => {
            let row = parse_index(&key)?;
            let Value::Call(name, arguments) = &value else {
                return Err("expected Millimeters(\"..\")".to_owned());
            };
            if !name.eq_ignore_ascii_case("Millimeters") {
                return Err("expected Millimeters(\"..\")".to_owned());
            }
            let millimeters: f64 = single_text(arguments)?
                .parse()
                .map_err(|_| "row height is not a number".to_owned())?;
            ViewStyle::default()
                .set_row_height(row, millimeters)
                .map_err(|error| error.to_string())?;
            DisplayItem::Height { row, millimeters }
        }
        "style" => {
            let rule = interpret_style_rule(&value)?;
            ViewStyle::default()
                .add_rule(rule.clone())
                .map_err(|error| error.to_string())?;
            DisplayItem::Style {
                index: parse_index(&key)?,
                rule,
            }
        }
        "conditional" => {
            let rule = interpret_conditional_rule(&value)?;
            ConditionalFormat::default()
                .add_rule(rule.clone())
                .map_err(|error| error.to_string())?;
            DisplayItem::Conditional {
                index: parse_index(&key)?,
                rule,
            }
        }
        other => return Err(format!("unknown display setting `{other}`")),
    };
    Ok((source_id, item))
}

fn parse_index(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a non-negative integer"))
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("`{other}` is not true or false")),
    }
}

// 名前と値の対応表。保存形式の名前は表で1か所に定める。
const HORIZONTAL: [(&str, HorizontalAlign); 3] = [
    ("Left", HorizontalAlign::Left),
    ("Center", HorizontalAlign::Center),
    ("Right", HorizontalAlign::Right),
];
const VERTICAL: [(&str, VerticalAlign); 3] = [
    ("Top", VerticalAlign::Top),
    ("Middle", VerticalAlign::Middle),
    ("Bottom", VerticalAlign::Bottom),
];
const OVERFLOW: [(&str, TextOverflow); 3] = [
    ("Wrap", TextOverflow::Wrap),
    ("Truncate", TextOverflow::Truncate),
    ("Ellipsis", TextOverflow::Ellipsis),
];
const EDGES: [(&str, BorderEdge); 6] = [
    ("Top", BorderEdge::Top),
    ("Bottom", BorderEdge::Bottom),
    ("Left", BorderEdge::Left),
    ("Right", BorderEdge::Right),
    ("DiagonalDown", BorderEdge::DiagonalDown),
    ("DiagonalUp", BorderEdge::DiagonalUp),
];
const BORDER_STYLES: [(&str, BorderStyle); 6] = [
    ("Thin", BorderStyle::Thin),
    ("Medium", BorderStyle::Medium),
    ("Thick", BorderStyle::Thick),
    ("Dashed", BorderStyle::Dashed),
    ("Dotted", BorderStyle::Dotted),
    ("Double", BorderStyle::Double),
];
const DATE_STYLES: [(&str, DateStyle); 5] = [
    ("Iso", DateStyle::Iso),
    ("Slash", DateStyle::Slash),
    ("Japanese", DateStyle::Japanese),
    ("DayMonthYear", DateStyle::DayMonthYear),
    ("MonthDayYear", DateStyle::MonthDayYear),
];
const SOURCES: [(&str, ValueSource); 5] = [
    ("Raw", ValueSource::Raw),
    ("Number", ValueSource::Number),
    ("Date", ValueSource::Date),
    ("DerivedText", ValueSource::DerivedText),
    ("DerivedNumber", ValueSource::DerivedNumber),
];
const OPERATORS: [(&str, CompareOp); 6] = [
    ("Eq", CompareOp::Eq),
    ("Ne", CompareOp::Ne),
    ("Lt", CompareOp::Lt),
    ("Le", CompareOp::Le),
    ("Gt", CompareOp::Gt),
    ("Ge", CompareOp::Ge),
];

fn name_of<T: PartialEq + Copy>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, candidate)| *candidate == value)
        .map(|(name, _)| *name)
        .unwrap_or_default()
}

fn value_of<T: Copy>(table: &[(&str, T)], name: &str, what: &str) -> Result<T, String> {
    table
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("unknown {what} `{name}`"))
}

fn interpret_format(value: &Value) -> Result<DisplayFormat, String> {
    let Value::Call(name, arguments) = value else {
        return Err("expected a display format such as Decimal(..)".to_owned());
    };
    let thousands =
        || -> Result<bool, String> { parse_bool(&single_text(argument(arguments, "Thousands")?)?) };
    let places = || -> Result<u8, String> {
        single_text(argument(arguments, "Places")?)?
            .parse()
            .map_err(|_| "Places is not a small non-negative integer".to_owned())
    };
    let date_style = || -> Result<DateStyle, String> {
        value_of(
            &DATE_STYLES,
            &single_text(argument(arguments, "Style")?)?,
            "date style",
        )
    };
    Ok(match name.to_ascii_lowercase().as_str() {
        "text" if arguments.is_empty() => DisplayFormat::Text,
        "integer" => DisplayFormat::Integer {
            thousands_separator: thousands()?,
        },
        "decimal" => DisplayFormat::Decimal {
            places: places()?,
            thousands_separator: thousands()?,
        },
        "percent" => DisplayFormat::Percent { places: places()? },
        "date" => DisplayFormat::Date(date_style()?),
        "datetime" => DisplayFormat::DateTime {
            style: date_style()?,
            show_seconds: parse_bool(&single_text(argument(arguments, "Seconds")?)?)?,
        },
        "boolean" => DisplayFormat::Boolean {
            true_label: single_text(argument(arguments, "True")?)?,
            false_label: single_text(argument(arguments, "False")?)?,
        },
        "custom" => DisplayFormat::Custom(single_text(arguments)?),
        other => return Err(format!("unknown display format `{other}`")),
    })
}

fn serialize_format(format: &DisplayFormat) -> String {
    let flag = |value: bool| quote(&value.to_string());
    match format {
        DisplayFormat::Text => "Text()".to_owned(),
        DisplayFormat::Integer {
            thousands_separator,
        } => format!("Integer(Thousands({}))", flag(*thousands_separator)),
        DisplayFormat::Decimal {
            places,
            thousands_separator,
        } => format!(
            "Decimal(Places({}), Thousands({}))",
            quote(&places.to_string()),
            flag(*thousands_separator)
        ),
        DisplayFormat::Percent { places } => {
            format!("Percent(Places({}))", quote(&places.to_string()))
        }
        DisplayFormat::Date(style) => {
            format!("Date(Style({}))", quote(name_of(&DATE_STYLES, *style)))
        }
        DisplayFormat::DateTime {
            style,
            show_seconds,
        } => format!(
            "DateTime(Style({}), Seconds({}))",
            quote(name_of(&DATE_STYLES, *style)),
            flag(*show_seconds)
        ),
        DisplayFormat::Boolean {
            true_label,
            false_label,
        } => format!(
            "Boolean(True({}), False({}))",
            quote(true_label),
            quote(false_label)
        ),
        DisplayFormat::Custom(id) => format!("Custom({})", quote(id)),
    }
}

fn interpret_target(arguments: &[Value]) -> Result<StyleTarget, String> {
    let [Value::Call(name, inner)] = arguments else {
        return Err("Target expects one of Column(..), Cell(..), Range(..)".to_owned());
    };
    match (name.to_ascii_lowercase().as_str(), inner.as_slice()) {
        ("column", [Value::Text(column)]) => Ok(StyleTarget::Column(column.clone())),
        ("cell", [Value::Text(column), Value::Text(row)]) => Ok(StyleTarget::Cell {
            column: column.clone(),
            row: parse_index(row)?,
        }),
        ("range", [Value::List(items), Value::Text(first), Value::Text(last)]) => {
            let columns = items
                .iter()
                .map(|item| match item {
                    Value::Text(text) => Ok(text.clone()),
                    _ => Err("Range expects a list of quoted headers".to_owned()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(StyleTarget::Range {
                columns,
                first_row: parse_index(first)?,
                last_row: parse_index(last)?,
            })
        }
        _ => Err("Target expects Column(\"h\"), Cell(\"h\", \"row\") or Range([..], \"first\", \"last\")".to_owned()),
    }
}

fn serialize_target(target: &StyleTarget) -> String {
    match target {
        StyleTarget::Column(column) => format!("Column({})", quote(column)),
        StyleTarget::Cell { column, row } => {
            format!("Cell({}, {})", quote(column), quote(&row.to_string()))
        }
        StyleTarget::Range {
            columns,
            first_row,
            last_row,
        } => {
            let columns: Vec<String> = columns.iter().map(|column| quote(column)).collect();
            format!(
                "Range([{}], {}, {})",
                columns.join(", "),
                quote(&first_row.to_string()),
                quote(&last_row.to_string())
            )
        }
    }
}

fn interpret_color(text: &str) -> Result<Color, String> {
    Color::parse(text).ok_or_else(|| format!("`{text}` is not a #rrggbb color"))
}

fn interpret_style(arguments: &[Value]) -> Result<CellStyle, String> {
    let mut style = CellStyle::default();
    let mut seen = HashSet::new();
    for argument in arguments {
        let Value::Call(name, inner) = argument else {
            return Err("expected a style property such as Bold(\"true\")".to_owned());
        };
        let key = name.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            return Err(format!("`{name}(..)` appears more than once"));
        }
        match key.as_str() {
            "horizontal" => {
                style.horizontal = Some(value_of(&HORIZONTAL, &single_text(inner)?, "alignment")?);
            }
            "vertical" => {
                style.vertical = Some(value_of(&VERTICAL, &single_text(inner)?, "alignment")?);
            }
            "overflow" => {
                style.overflow = Some(value_of(&OVERFLOW, &single_text(inner)?, "overflow")?);
            }
            "bold" => style.bold = Some(parse_bool(&single_text(inner)?)?),
            "italic" => style.italic = Some(parse_bool(&single_text(inner)?)?),
            "underline" => style.underline = Some(parse_bool(&single_text(inner)?)?),
            "textcolor" => style.text_color = Some(interpret_color(&single_text(inner)?)?),
            "background" => style.background = Some(interpret_color(&single_text(inner)?)?),
            "borders" => {
                let [Value::List(items)] = inner.as_slice() else {
                    return Err("expected Borders([..])".to_owned());
                };
                for item in items {
                    let (edge, setting) = interpret_border(item)?;
                    if style.borders.insert(edge, setting).is_some() {
                        return Err("a border edge appears more than once".to_owned());
                    }
                }
            }
            other => return Err(format!("unknown style property `{other}`")),
        }
    }
    Ok(style)
}

fn interpret_border(item: &Value) -> Result<(BorderEdge, BorderEdgeSetting), String> {
    let Value::Call(name, arguments) = item else {
        return Err("expected Border(\"edge\", \"style\")".to_owned());
    };
    if !name.eq_ignore_ascii_case("Border") {
        return Err("expected Border(\"edge\", \"style\")".to_owned());
    }
    let texts = arguments
        .iter()
        .map(|argument| match argument {
            Value::Text(text) => Ok(text.as_str()),
            _ => Err("Border expects quoted strings".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    match texts.as_slice() {
        [edge, "Clear"] => Ok((
            value_of(&EDGES, edge, "border edge")?,
            BorderEdgeSetting::Clear,
        )),
        [edge, style] => Ok((
            value_of(&EDGES, edge, "border edge")?,
            BorderEdgeSetting::Line(BorderLine {
                style: value_of(&BORDER_STYLES, style, "border style")?,
                color: None,
            }),
        )),
        [edge, style, color] => Ok((
            value_of(&EDGES, edge, "border edge")?,
            BorderEdgeSetting::Line(BorderLine {
                style: value_of(&BORDER_STYLES, style, "border style")?,
                color: Some(interpret_color(color)?),
            }),
        )),
        _ => Err("Border expects an edge, a style and an optional color".to_owned()),
    }
}

fn serialize_style(style: &CellStyle) -> String {
    let mut parts = Vec::new();
    if let Some(value) = style.horizontal {
        parts.push(format!(
            "Horizontal({})",
            quote(name_of(&HORIZONTAL, value))
        ));
    }
    if let Some(value) = style.vertical {
        parts.push(format!("Vertical({})", quote(name_of(&VERTICAL, value))));
    }
    if let Some(value) = style.overflow {
        parts.push(format!("Overflow({})", quote(name_of(&OVERFLOW, value))));
    }
    for (name, value) in [
        ("Bold", style.bold),
        ("Italic", style.italic),
        ("Underline", style.underline),
    ] {
        if let Some(value) = value {
            parts.push(format!("{name}({})", quote(&value.to_string())));
        }
    }
    if let Some(color) = &style.text_color {
        parts.push(format!("TextColor({})", quote(color.as_str())));
    }
    if let Some(color) = &style.background {
        parts.push(format!("Background({})", quote(color.as_str())));
    }
    if !style.borders.is_empty() {
        let borders: Vec<String> = style
            .borders
            .iter()
            .map(|(edge, setting)| {
                let edge = quote(name_of(&EDGES, *edge));
                match setting {
                    BorderEdgeSetting::Clear => format!("Border({edge}, \"Clear\")"),
                    BorderEdgeSetting::Line(line) => {
                        let style = quote(name_of(&BORDER_STYLES, line.style));
                        match &line.color {
                            Some(color) => {
                                format!("Border({edge}, {style}, {})", quote(color.as_str()))
                            }
                            None => format!("Border({edge}, {style})"),
                        }
                    }
                }
            })
            .collect();
        parts.push(format!("Borders([{}])", borders.join(", ")));
    }
    format!("Style({})", parts.join(", "))
}

fn interpret_style_rule(value: &Value) -> Result<StyleRule, String> {
    let Value::Call(name, arguments) = value else {
        return Err("expected Rule(Target(..), Style(..))".to_owned());
    };
    if !name.eq_ignore_ascii_case("Rule") {
        return Err("expected Rule(Target(..), Style(..))".to_owned());
    }
    Ok(StyleRule {
        target: interpret_target(argument(arguments, "Target")?)?,
        style: interpret_style(argument(arguments, "Style")?)?,
    })
}

fn interpret_conditional_rule(value: &Value) -> Result<ConditionalRule, String> {
    let Value::Call(name, arguments) = value else {
        return Err("expected Rule(Target(..), Source(..), When(..), Style(..))".to_owned());
    };
    if !name.eq_ignore_ascii_case("Rule") {
        return Err("expected Rule(Target(..), Source(..), When(..), Style(..))".to_owned());
    }
    let [predicate] = argument(arguments, "When")? else {
        return Err("When expects one condition".to_owned());
    };
    let stop_if_true = match optional_argument(arguments, "Stop")? {
        [] => false,
        inner => parse_bool(&single_text(inner)?)?,
    };
    Ok(ConditionalRule {
        target: interpret_target(argument(arguments, "Target")?)?,
        source: value_of(
            &SOURCES,
            &single_text(argument(arguments, "Source")?)?,
            "value source",
        )?,
        predicate: interpret_predicate(predicate)?,
        style: interpret_style(argument(arguments, "Style")?)?,
        stop_if_true,
    })
}

fn interpret_literal(value: &Value) -> Result<Literal, String> {
    let Value::Call(name, arguments) = value else {
        return Err("expected Text(..), Number(..) or Date(..)".to_owned());
    };
    let text = single_text(arguments)?;
    match name.to_ascii_lowercase().as_str() {
        "text" => Ok(Literal::Text(text)),
        "number" => Ok(Literal::Number(text)),
        "date" => Ok(Literal::Date(text)),
        other => Err(format!("unknown literal `{other}`")),
    }
}

fn serialize_literal(literal: &Literal) -> String {
    match literal {
        Literal::Text(text) => format!("Text({})", quote(text)),
        Literal::Number(text) => format!("Number({})", quote(text)),
        Literal::Date(text) => format!("Date({})", quote(text)),
    }
}

fn interpret_predicate(value: &Value) -> Result<Predicate, String> {
    let Value::Call(name, arguments) = value else {
        return Err("expected a condition such as Compare(..)".to_owned());
    };
    let predicates = |inner: &[Value]| -> Result<Vec<Predicate>, String> {
        let [Value::List(items)] = inner else {
            return Err("expected one list of conditions".to_owned());
        };
        items.iter().map(interpret_predicate).collect()
    };
    Ok(
        match (name.to_ascii_lowercase().as_str(), arguments.as_slice()) {
            ("compare", [Value::Text(op), literal]) => Predicate::Compare {
                op: value_of(&OPERATORS, op, "operator")?,
                literal: interpret_literal(literal)?,
            },
            ("between", [low, high]) => Predicate::Between {
                low: interpret_literal(low)?,
                high: interpret_literal(high)?,
            },
            ("contains", [Value::Text(text)]) => Predicate::Contains(text.clone()),
            ("startswith", [Value::Text(text)]) => Predicate::StartsWith(text.clone()),
            ("isempty", []) => Predicate::IsEmpty,
            ("isnotempty", []) => Predicate::IsNotEmpty,
            ("and", inner) => Predicate::And(predicates(inner)?),
            ("or", inner) => Predicate::Or(predicates(inner)?),
            ("not", [inner]) => Predicate::Not(Box::new(interpret_predicate(inner)?)),
            (other, _) => return Err(format!("unknown or malformed condition `{other}`")),
        },
    )
}

fn serialize_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Compare { op, literal } => format!(
            "Compare({}, {})",
            quote(name_of(&OPERATORS, *op)),
            serialize_literal(literal)
        ),
        Predicate::Between { low, high } => format!(
            "Between({}, {})",
            serialize_literal(low),
            serialize_literal(high)
        ),
        Predicate::Contains(text) => format!("Contains({})", quote(text)),
        Predicate::StartsWith(text) => format!("StartsWith({})", quote(text)),
        Predicate::IsEmpty => "IsEmpty()".to_owned(),
        Predicate::IsNotEmpty => "IsNotEmpty()".to_owned(),
        Predicate::And(children) => format!("And([{}])", serialize_predicates(children)),
        Predicate::Or(children) => format!("Or([{}])", serialize_predicates(children)),
        Predicate::Not(inner) => format!("Not({})", serialize_predicate(inner)),
    }
}

fn serialize_predicates(children: &[Predicate]) -> String {
    children
        .iter()
        .map(serialize_predicate)
        .collect::<Vec<_>>()
        .join(", ")
}
