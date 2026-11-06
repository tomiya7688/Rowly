//! ```text
//! 責務: [display_views::style_syntax: 表示書式・target・styleのDSL値の解釈と直列化、および名前表と共通の小さな関数を持つ]
//! 処理: [1: 名前と値の対応は表で1か所に定める 2: 解釈は未知の名前・重複・不正な値をerrorにして黙って捨てない 3: 直列化は指定された項目だけを書く]
//! 補足: [宣言全体の文法と保存・優先規則はdisplay_views側が扱う]
//! ```
use std::collections::HashSet;

use crate::process::{
    display_format::{DateStyle, DisplayFormat},
    view_style::{
        BorderEdge, BorderEdgeSetting, BorderLine, BorderStyle, CellStyle, Color, HorizontalAlign,
        StyleTarget, TextOverflow, VerticalAlign,
    },
};

use super::super::aggregation_views::{Value, argument, single_text};
use super::super::quote;

pub(super) fn parse_index(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a non-negative integer"))
}

pub(super) fn parse_bool(text: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("`{other}` is not true or false")),
    }
}

// 名前と値の対応表。保存形式の名前は表で1か所に定める。
pub(super) const HORIZONTAL: [(&str, HorizontalAlign); 3] = [
    ("Left", HorizontalAlign::Left),
    ("Center", HorizontalAlign::Center),
    ("Right", HorizontalAlign::Right),
];
pub(super) const VERTICAL: [(&str, VerticalAlign); 3] = [
    ("Top", VerticalAlign::Top),
    ("Middle", VerticalAlign::Middle),
    ("Bottom", VerticalAlign::Bottom),
];
pub(super) const OVERFLOW: [(&str, TextOverflow); 3] = [
    ("Wrap", TextOverflow::Wrap),
    ("Truncate", TextOverflow::Truncate),
    ("Ellipsis", TextOverflow::Ellipsis),
];
pub(super) const EDGES: [(&str, BorderEdge); 6] = [
    ("Top", BorderEdge::Top),
    ("Bottom", BorderEdge::Bottom),
    ("Left", BorderEdge::Left),
    ("Right", BorderEdge::Right),
    ("DiagonalDown", BorderEdge::DiagonalDown),
    ("DiagonalUp", BorderEdge::DiagonalUp),
];
pub(super) const BORDER_STYLES: [(&str, BorderStyle); 6] = [
    ("Thin", BorderStyle::Thin),
    ("Medium", BorderStyle::Medium),
    ("Thick", BorderStyle::Thick),
    ("Dashed", BorderStyle::Dashed),
    ("Dotted", BorderStyle::Dotted),
    ("Double", BorderStyle::Double),
];
pub(super) const DATE_STYLES: [(&str, DateStyle); 5] = [
    ("Iso", DateStyle::Iso),
    ("Slash", DateStyle::Slash),
    ("Japanese", DateStyle::Japanese),
    ("DayMonthYear", DateStyle::DayMonthYear),
    ("MonthDayYear", DateStyle::MonthDayYear),
];

pub(super) fn name_of<T: PartialEq + Copy>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, candidate)| *candidate == value)
        .map(|(name, _)| *name)
        .unwrap_or_default()
}

pub(super) fn value_of<T: Copy>(table: &[(&str, T)], name: &str, what: &str) -> Result<T, String> {
    table
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("unknown {what} `{name}`"))
}

pub(super) fn interpret_format(value: &Value) -> Result<DisplayFormat, String> {
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

pub(super) fn serialize_format(format: &DisplayFormat) -> String {
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

pub(super) fn interpret_target(arguments: &[Value]) -> Result<StyleTarget, String> {
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

pub(super) fn serialize_target(target: &StyleTarget) -> String {
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

pub(super) fn interpret_color(text: &str) -> Result<Color, String> {
    Color::parse(text).ok_or_else(|| format!("`{text}` is not a #rrggbb color"))
}

pub(super) fn interpret_style(arguments: &[Value]) -> Result<CellStyle, String> {
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

pub(super) fn interpret_border(item: &Value) -> Result<(BorderEdge, BorderEdgeSetting), String> {
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

pub(super) fn serialize_style(style: &CellStyle) -> String {
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
