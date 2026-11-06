//! ```text
//! 責務: [display_views::conditional_syntax: 条件付き書式のrule・条件・右辺のDSL値の解釈と直列化を持つ]
//! 処理: [1: 条件はAnd・Or・Notを含めて再帰的に読み書きする 2: 未知の名前・型の合わない引数はerrorにする]
//! 補足: [rule全体の検証（型の整合など）はConditionalFormat::add_ruleが行う]
//! ```
use crate::process::conditional_format::{
    CompareOp, ConditionalRule, Literal, Predicate, ValueSource,
};
use crate::process::view_style::StyleRule;

use super::super::aggregation_views::{Value, argument, optional_argument, single_text};
use super::super::quote;
use super::style_syntax::{interpret_style, interpret_target, name_of, parse_bool, value_of};

// 条件付き書式の名前表。
pub(super) const SOURCES: [(&str, ValueSource); 5] = [
    ("Raw", ValueSource::Raw),
    ("Number", ValueSource::Number),
    ("Date", ValueSource::Date),
    ("DerivedText", ValueSource::DerivedText),
    ("DerivedNumber", ValueSource::DerivedNumber),
];
pub(super) const OPERATORS: [(&str, CompareOp); 6] = [
    ("Eq", CompareOp::Eq),
    ("Ne", CompareOp::Ne),
    ("Lt", CompareOp::Lt),
    ("Le", CompareOp::Le),
    ("Gt", CompareOp::Gt),
    ("Ge", CompareOp::Ge),
];

pub(super) fn interpret_style_rule(value: &Value) -> Result<StyleRule, String> {
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

pub(super) fn interpret_conditional_rule(value: &Value) -> Result<ConditionalRule, String> {
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

pub(super) fn interpret_literal(value: &Value) -> Result<Literal, String> {
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

pub(super) fn serialize_literal(literal: &Literal) -> String {
    match literal {
        Literal::Text(text) => format!("Text({})", quote(text)),
        Literal::Number(text) => format!("Number({})", quote(text)),
        Literal::Date(text) => format!("Date({})", quote(text)),
    }
}

pub(super) fn interpret_predicate(value: &Value) -> Result<Predicate, String> {
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

pub(super) fn serialize_predicate(predicate: &Predicate) -> String {
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

pub(super) fn serialize_predicates(children: &[Predicate]) -> String {
    children
        .iter()
        .map(serialize_predicate)
        .collect::<Vec<_>>()
        .join(", ")
}
