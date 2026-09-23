use thiserror::Error;

use crate::process::{CellRange, ColumnType};

use super::ast::{
    ArithmeticOperator, ClassDefinition, ColumnSelector, ComparisonOperator, Condition,
    DeclarationKind, Expression, FieldDefinition, FunctionDefinition, Program, Statement,
    UnaryOperator,
};

pub fn parse(source: &str) -> Result<Program, ParseError> {
    Parser::new(source).parse_program()
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Rowly DSL parse error on line {line}: {message}")]
pub struct ParseError {
    line: usize,
    message: String,
}

impl ParseError {
    pub fn line(&self) -> usize {
        self.line
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone)]
struct SourceLine {
    number: usize,
    text: String,
}

struct Parser {
    lines: Vec<SourceLine>,
    position: usize,
}

impl Parser {
    fn new(source: &str) -> Self {
        let lines = source
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let text = line.trim();
                if text.is_empty() || text.starts_with('\'') || is_rem_comment(text) {
                    None
                } else {
                    Some(SourceLine {
                        number: index + 1,
                        text: text.to_owned(),
                    })
                }
            })
            .collect();
        Self { lines, position: 0 }
    }

    fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut classes = Vec::new();
        let mut functions = Vec::new();
        let mut statements = Vec::new();

        while let Some(line) = self.lines.get(self.position).cloned() {
            if starts_with_ci(&line.text, "class ") {
                let class = self.parse_class(&line)?;
                if classes
                    .iter()
                    .any(|item: &ClassDefinition| item.name.eq_ignore_ascii_case(&class.name))
                {
                    return Err(parse_error(
                        line.number,
                        format!("duplicate class `{}`", class.name),
                    ));
                }
                classes.push(class);
                continue;
            }
            if starts_with_ci(&line.text, "def ") {
                let function = self.parse_function(&line)?;
                if functions
                    .iter()
                    .any(|item: &FunctionDefinition| item.name.eq_ignore_ascii_case(&function.name))
                {
                    return Err(parse_error(
                        line.number,
                        format!("duplicate function `{}`", function.name),
                    ));
                }
                functions.push(function);
                continue;
            }
            if is_terminator(&line.text) || eq_ci(&line.text, "else") {
                return Err(parse_error(line.number, "unexpected block terminator"));
            }
            statements.push(self.parse_statement_or_if()?);
        }

        Ok(Program {
            classes,
            functions,
            statements,
        })
    }

    fn parse_class(&mut self, line: &SourceLine) -> Result<ClassDefinition, ParseError> {
        let header = strip_prefix_ci(&line.text, "class ")
            .map(str::trim)
            .ok_or_else(|| parse_error(line.number, "expected `Class name`"))?;
        let (name, parent) =
            if let Some((name, parent)) = split_keyword_top_level(header, " extends ") {
                let name = name.trim();
                let parent = parent.trim();
                validate_identifier(name, line.number)?;
                validate_identifier(parent, line.number)?;
                (name.to_owned(), Some(parent.to_owned()))
            } else {
                validate_identifier(header, line.number)?;
                (header.to_owned(), None)
            };
        self.position += 1;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_class(&current.text) {
                self.position += 1;
                return Ok(ClassDefinition {
                    name,
                    parent,
                    fields,
                    methods,
                });
            }
            if starts_with_ci(&current.text, "field ") {
                let field = parse_field_definition(&current)?;
                if fields
                    .iter()
                    .any(|item: &FieldDefinition| item.name.eq_ignore_ascii_case(&field.name))
                {
                    return Err(parse_error(
                        current.number,
                        format!("duplicate field `{}`", field.name),
                    ));
                }
                fields.push(field);
                self.position += 1;
                continue;
            }
            if starts_with_ci(&current.text, "def ") {
                let method = self.parse_function(&current)?;
                if methods
                    .iter()
                    .any(|item: &FunctionDefinition| item.name.eq_ignore_ascii_case(&method.name))
                {
                    return Err(parse_error(
                        current.number,
                        format!("duplicate method `{}`", method.name),
                    ));
                }
                methods.push(method);
                continue;
            }
            return Err(parse_error(
                current.number,
                "class bodies only support `Field` and `Def` declarations",
            ));
        }

        Err(parse_error(line.number, "missing `End Class`"))
    }

    fn parse_function(&mut self, line: &SourceLine) -> Result<FunctionDefinition, ParseError> {
        let (name, parameters) = parse_function_signature(line)?;
        self.position += 1;
        let mut body = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_def(&current.text) {
                self.position += 1;
                return Ok(FunctionDefinition {
                    name,
                    parameters,
                    body,
                });
            }
            if is_end_class(&current.text)
                || eq_ci(&current.text, "else")
                || is_end_if(&current.text)
                || is_next(&current.text)
            {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            body.push(self.parse_statement_or_if()?);
        }
        Err(parse_error(line.number, "missing `End Def`"))
    }

    fn parse_statement_or_if(&mut self) -> Result<Statement, ParseError> {
        let line = self
            .lines
            .get(self.position)
            .cloned()
            .expect("parser position is valid");
        if starts_with_ci(&line.text, "if ") {
            return self.parse_if(&line);
        }
        if starts_with_ci(&line.text, "for ") {
            return self.parse_for(&line);
        }
        if starts_with_ci(&line.text, "def ") || starts_with_ci(&line.text, "class ") {
            return Err(parse_error(
                line.number,
                "definitions are only allowed at the top level or as class methods",
            ));
        }
        let statement = parse_statement(&line)?;
        self.position += 1;
        Ok(statement)
    }

    fn parse_for(&mut self, line: &SourceLine) -> Result<Statement, ParseError> {
        let rest = strip_prefix_ci(&line.text, "for ")
            .ok_or_else(|| parse_error(line.number, "expected `For name = start To end`"))?;
        let (variable, range) = split_top_level_once(rest, '=')
            .ok_or_else(|| parse_error(line.number, "expected `For name = start To end`"))?;
        let variable = variable.trim();
        validate_binding_name(variable, line.number)?;

        let (start_text, end_and_step) = split_keyword_top_level(range.trim(), " to ")
            .ok_or_else(|| parse_error(line.number, "expected `To` in For loop"))?;
        let (end_text, step_text) =
            if let Some((end, step)) = split_keyword_top_level(end_and_step.trim(), " step ") {
                (end.trim(), Some(step.trim()))
            } else {
                (end_and_step.trim(), None)
            };

        let start = parse_expression(start_text.trim(), line.number)?;
        let end = parse_expression(end_text, line.number)?;
        let step = step_text
            .map(|text| parse_expression(text, line.number))
            .transpose()?;

        self.position += 1;
        let mut body = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_next(&current.text) {
                if let Some(name) = next_variable(&current.text) {
                    if !name.eq_ignore_ascii_case(variable) {
                        return Err(parse_error(
                            current.number,
                            format!(
                                "Next variable `{name}` does not match For variable `{variable}`"
                            ),
                        ));
                    }
                }
                self.position += 1;
                return Ok(Statement::For {
                    variable: variable.to_owned(),
                    start,
                    end,
                    step,
                    body,
                });
            }
            if is_end_def(&current.text) || is_end_class(&current.text) || is_end_if(&current.text)
            {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            body.push(self.parse_statement_or_if()?);
        }

        Err(parse_error(line.number, "missing `Next`"))
    }

    fn parse_if(&mut self, line: &SourceLine) -> Result<Statement, ParseError> {
        let condition = parse_if_condition(line)?;
        self.position += 1;
        let mut body = Vec::new();
        let mut else_body = Vec::new();
        let mut in_else = false;

        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_if(&current.text) {
                self.position += 1;
                return Ok(Statement::If {
                    condition,
                    body,
                    else_body,
                });
            }
            if eq_ci(&current.text, "else") {
                if in_else {
                    return Err(parse_error(current.number, "duplicate `Else`"));
                }
                in_else = true;
                self.position += 1;
                continue;
            }
            if is_end_def(&current.text) || is_end_class(&current.text) || is_next(&current.text) {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            let statement = self.parse_statement_or_if()?;
            if in_else {
                else_body.push(statement);
            } else {
                body.push(statement);
            }
        }

        Err(parse_error(line.number, "missing `End If`"))
    }
}

fn is_next(text: &str) -> bool {
    eq_ci(text.trim(), "next") || starts_with_ci(text.trim(), "next ")
}

fn next_variable(text: &str) -> Option<&str> {
    strip_prefix_ci(text.trim(), "next ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_function_signature(line: &SourceLine) -> Result<(String, Vec<String>), ParseError> {
    let signature = strip_prefix_ci(&line.text, "def ")
        .ok_or_else(|| parse_error(line.number, "expected `Def name(...)`"))?;
    let (name, arguments) = parse_named_call(signature.trim(), line.number)?;
    let parameters = split_arguments(arguments, line.number)?
        .into_iter()
        .map(|item| {
            let item = item.trim();
            validate_binding_name(item, line.number)?;
            Ok(item.to_owned())
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    for (index, parameter) in parameters.iter().enumerate() {
        if parameters[..index]
            .iter()
            .any(|item| item.eq_ignore_ascii_case(parameter))
        {
            return Err(parse_error(
                line.number,
                format!("duplicate parameter `{parameter}`"),
            ));
        }
    }
    Ok((name, parameters))
}

fn parse_field_definition(line: &SourceLine) -> Result<FieldDefinition, ParseError> {
    let rest = strip_prefix_ci(&line.text, "field ")
        .ok_or_else(|| parse_error(line.number, "expected `Field name = value`"))?;
    let (name, value) = split_top_level_once(rest, '=')
        .ok_or_else(|| parse_error(line.number, "expected `Field name = value`"))?;
    let name = name.trim();
    validate_identifier(name, line.number)?;
    Ok(FieldDefinition {
        name: name.to_owned(),
        default: parse_expression(value.trim(), line.number)?,
    })
}

fn parse_if_condition(line: &SourceLine) -> Result<Condition, ParseError> {
    let lower = line.text.to_ascii_lowercase();
    if !lower.starts_with("if ") || !lower.ends_with(" then") {
        return Err(parse_error(line.number, "expected `If <condition> Then`"));
    }
    parse_condition(line.text[3..line.text.len() - 5].trim(), line.number)
}

fn parse_condition(text: &str, line: usize) -> Result<Condition, ParseError> {
    let text = strip_outer_parentheses(text.trim());
    if let Some((left, right)) = split_keyword_top_level(text, " or ") {
        return Ok(Condition::Or(
            Box::new(parse_condition(left, line)?),
            Box::new(parse_condition(right, line)?),
        ));
    }
    if let Some((left, right)) = split_keyword_top_level(text, " and ") {
        return Ok(Condition::And(
            Box::new(parse_condition(left, line)?),
            Box::new(parse_condition(right, line)?),
        ));
    }
    if let Some(rest) = strip_prefix_ci(text, "not ") {
        return Ok(Condition::Not(Box::new(parse_condition(
            rest.trim(),
            line,
        )?)));
    }
    if starts_with_ci(text, "this.worksheet.column(") {
        let (selector, remainder) = parse_column_target(text, line)?;
        if eq_ci(remainder, ".exists") {
            return Ok(Condition::ColumnExists { selector });
        }
        if let Some(rest) = strip_prefix_ci(remainder, ".title") {
            let value = rest.trim();
            if let Some(value) = value.strip_prefix('=') {
                return Ok(Condition::ColumnTitleEquals {
                    selector,
                    expected: parse_expression(value.trim(), line)?,
                });
            }
        }
    }
    if let Some((left, operator, right)) = split_comparison(text) {
        return Ok(Condition::Compare {
            left: parse_expression(left.trim(), line)?,
            operator,
            right: parse_expression(right.trim(), line)?,
        });
    }
    Ok(Condition::Expression(parse_expression(text, line)?))
}

fn split_comparison(text: &str) -> Option<(&str, ComparisonOperator, &str)> {
    for (token, operator) in [
        ("!=", ComparisonOperator::NotEqual),
        ("<=", ComparisonOperator::LessOrEqual),
        (">=", ComparisonOperator::GreaterOrEqual),
        ("=", ComparisonOperator::Equal),
        ("<", ComparisonOperator::Less),
        (">", ComparisonOperator::Greater),
    ] {
        if let Some((left, right)) = split_top_level_token(text, token) {
            return Some((left, operator, right));
        }
    }
    None
}

fn parse_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if let Some(rest) = strip_keyword(&line.text, "var") {
        return parse_declaration(rest, DeclarationKind::Var, line.number);
    }
    if let Some(rest) = strip_keyword(&line.text, "const") {
        return parse_declaration(rest, DeclarationKind::Const, line.number);
    }
    if strip_keyword(&line.text, "let").is_some() || strip_keyword(&line.text, "dim").is_some() {
        return Err(parse_error(
            line.number,
            "LET / DIM are not supported; use VAR or CONST",
        ));
    }
    if eq_ci(&line.text, "return") || starts_with_ci(&line.text, "return ") {
        return parse_return_statement(line);
    }
    if starts_with_ci(&line.text, "this.worksheet.column(") {
        return parse_column_statement(line);
    }
    if starts_with_ci(&line.text, "this.worksheet.editor.cell(") {
        return parse_cell_statement(line);
    }
    if let Some((name, value)) = split_top_level_once(&line.text, '=') {
        let name = name.trim();
        if is_identifier(name) {
            validate_binding_name(name, line.number)?;
            return Ok(Statement::Assign {
                name: name.to_owned(),
                value: parse_binding_value(value, line.number)?,
            });
        }
    }
    if let Some(statement) = parse_field_assignment(line)? {
        return Ok(statement);
    }
    match parse_expression(&line.text, line.number)? {
        Expression::Call { name, arguments } => Ok(Statement::Call { name, arguments }),
        Expression::MethodCall {
            target,
            name,
            arguments,
        } => Ok(Statement::MethodCall {
            target,
            name,
            arguments,
        }),
        _ => Err(parse_error(line.number, "unsupported statement")),
    }
}

fn parse_declaration(
    rest: &str,
    kind: DeclarationKind,
    line: usize,
) -> Result<Statement, ParseError> {
    let (name, value) = split_top_level_once(rest, '=')
        .ok_or_else(|| parse_error(line, "expected `VAR name = value` or `CONST name = value`"))?;
    let name = name.trim();
    validate_binding_name(name, line)?;
    Ok(Statement::Declare {
        kind,
        name: name.to_owned(),
        value: parse_binding_value(value, line)?,
    })
}

fn parse_binding_value(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.starts_with('=') {
        return Err(parse_error(line, "expected a value after a single `=`"));
    }
    parse_expression(text, line)
}

fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_prefix_ci(text, keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

fn validate_binding_name(name: &str, line: usize) -> Result<(), ParseError> {
    validate_identifier(name, line)?;
    if [
        "self", "super", "true", "false", "var", "const", "let", "dim",
    ]
    .iter()
    .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        return Err(parse_error(line, format!("reserved binding name `{name}`")));
    }
    Ok(())
}

fn parse_return_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if eq_ci(&line.text, "return") {
        return Ok(Statement::Return { value: None });
    }
    let rest = strip_prefix_ci(&line.text, "return ").unwrap();
    Ok(Statement::Return {
        value: Some(parse_expression(rest.trim(), line.number)?),
    })
}

fn parse_field_assignment(line: &SourceLine) -> Result<Option<Statement>, ParseError> {
    let Some((left, right)) = split_top_level_once(&line.text, '=') else {
        return Ok(None);
    };
    let Some((target, field)) = split_member(left.trim()) else {
        return Ok(None);
    };
    validate_identifier(target, line.number)?;
    validate_identifier(field, line.number)?;
    Ok(Some(Statement::SetField {
        target: target.to_owned(),
        field: field.to_owned(),
        value: parse_expression(right.trim(), line.number)?,
    }))
}

fn parse_column_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let (selector, remainder) = parse_column_target(&line.text, line.number)?;
    if let Some(rest) = strip_prefix_ci(remainder, ".type") {
        let value = parse_assignment_scalar(rest, line.number)?;
        let column_type = parse_column_type(&value).ok_or_else(|| {
            parse_error(
                line.number,
                "column type must be String, Integer, Decimal, or Boolean",
            )
        })?;
        return Ok(Statement::ValidateColumnType {
            selector,
            column_type,
        });
    }
    if eq_ci(remainder, ".check.japanese") {
        return Ok(Statement::CheckJapanese { selector });
    }
    Err(parse_error(
        line.number,
        "supported column statements are `.Type = ...` and `.Check.Japanese`",
    ))
}

fn parse_cell_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let (argument, remainder) = parse_call(&line.text, "this.worksheet.editor.cell(", line.number)?;
    if !starts_with_ci(remainder, ".value.set") {
        return Err(parse_error(
            line.number,
            "expected `.Value.Set = <value>` after Cell(...) ",
        ));
    }
    let value = parse_assignment_expression(&remainder[".value.set".len()..], line.number)?;
    let range = parse_range_argument(argument, line.number)?
        .parse::<CellRange>()
        .map_err(|error| parse_error(line.number, error.to_string()))?;
    Ok(Statement::SetRangeValue { range, value })
}

fn parse_column_target(text: &str, line: usize) -> Result<(ColumnSelector, &str), ParseError> {
    let (argument, remainder) = parse_call(text, "this.worksheet.column(", line)?;
    Ok((parse_column_selector(argument, line)?, remainder))
}

fn parse_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    parse_additive_expression(text.trim(), line)
}

fn parse_additive_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    if let Some((left, operator, right)) = split_top_level_arithmetic(text, &['+', '-']) {
        let operator = match operator {
            '+' => ArithmeticOperator::Add,
            '-' => ArithmeticOperator::Subtract,
            _ => unreachable!(),
        };
        return Ok(Expression::Arithmetic {
            left: Box::new(parse_additive_expression(left.trim(), line)?),
            operator,
            right: Box::new(parse_multiplicative_expression(right.trim(), line)?),
        });
    }
    parse_multiplicative_expression(text, line)
}

fn parse_multiplicative_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    if let Some((left, operator, right)) = split_top_level_arithmetic(text, &['*', '/']) {
        let operator = match operator {
            '*' => ArithmeticOperator::Multiply,
            '/' => ArithmeticOperator::Divide,
            _ => unreachable!(),
        };
        return Ok(Expression::Arithmetic {
            left: Box::new(parse_multiplicative_expression(left.trim(), line)?),
            operator,
            right: Box::new(parse_unary_expression(right.trim(), line)?),
        });
    }
    parse_unary_expression(text, line)
}

fn parse_unary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('-') {
        if rest.trim().is_empty() {
            return Err(parse_error(line, "expected a value after unary '-'"));
        }
        return Ok(Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: Box::new(parse_unary_expression(rest.trim(), line)?),
        });
    }
    parse_primary_expression(text, line)
}

fn parse_primary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    let stripped = strip_outer_parentheses(text);
    if stripped.len() != text.len() {
        return parse_expression(stripped, line);
    }
    if text.starts_with('"') {
        return Ok(Expression::Literal(parse_scalar(text, line)?));
    }
    if let Some(rest) = strip_prefix_ci(text, "new ") {
        let (class_name, arguments) = parse_named_call(rest.trim(), line)?;
        return Ok(Expression::New {
            class_name,
            arguments: parse_argument_expressions(arguments, line)?,
        });
    }
    if let Some((target, member)) = split_member(text) {
        validate_identifier(target, line)?;
        if looks_like_named_call(member) {
            let (name, arguments) = parse_named_call(member, line)?;
            return Ok(Expression::MethodCall {
                target: target.to_owned(),
                name,
                arguments: parse_argument_expressions(arguments, line)?,
            });
        }
        validate_identifier(member, line)?;
        return Ok(Expression::Field {
            target: target.to_owned(),
            field: member.to_owned(),
        });
    }
    if looks_like_named_call(text) {
        let (name, arguments) = parse_named_call(text, line)?;
        return Ok(Expression::Call {
            name,
            arguments: parse_argument_expressions(arguments, line)?,
        });
    }
    if eq_ci(text, "true") || eq_ci(text, "false") {
        return Ok(Expression::Literal(text.to_ascii_lowercase()));
    }
    if is_identifier(text) {
        return Ok(Expression::Variable(text.to_owned()));
    }
    Ok(Expression::Literal(text.to_owned()))
}

fn split_top_level_arithmetic<'a>(
    text: &'a str,
    operators: &[char],
) -> Option<(&'a str, char, &'a str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut candidate = None;

    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 && operators.contains(&ch) => {
                if (ch == '+' || ch == '-') && is_unary_sign(text, index) {
                    continue;
                }
                candidate = Some((index, ch));
            }
            _ => {}
        }
    }

    candidate.map(|(index, operator)| {
        let right = index + operator.len_utf8();
        (&text[..index], operator, &text[right..])
    })
}

fn is_unary_sign(text: &str, index: usize) -> bool {
    let before = text[..index].trim_end();
    if before.is_empty() {
        return true;
    }
    matches!(
        before.chars().next_back(),
        Some('(' | ',' | '+' | '-' | '*' | '/' | '=' | '<' | '>')
    )
}

fn parse_argument_expressions(text: &str, line: usize) -> Result<Vec<Expression>, ParseError> {
    split_arguments(text, line)?
        .into_iter()
        .map(|argument| parse_expression(argument.trim(), line))
        .collect()
}

fn parse_call<'a>(
    text: &'a str,
    prefix: &str,
    line: usize,
) -> Result<(&'a str, &'a str), ParseError> {
    if !starts_with_ci(text, prefix) {
        return Err(parse_error(line, format!("expected `{prefix}...`")));
    }
    let start = prefix.len();
    let close = find_closing_parenthesis(text, start)
        .ok_or_else(|| parse_error(line, "missing closing `)`"))?;
    Ok((&text[start..close], text[close + 1..].trim()))
}

fn parse_named_call(text: &str, line: usize) -> Result<(String, &str), ParseError> {
    let open = text
        .find('(')
        .ok_or_else(|| parse_error(line, "expected call parentheses"))?;
    let name = text[..open].trim();
    validate_identifier(name, line)?;
    let close = find_closing_parenthesis(text, open + 1)
        .ok_or_else(|| parse_error(line, "missing closing `)`"))?;
    if !text[close + 1..].trim().is_empty() {
        return Err(parse_error(line, "unexpected text after call"));
    }
    Ok((name.to_owned(), &text[open + 1..close]))
}

fn find_closing_parenthesis(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(start + offset),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn parse_column_selector(argument: &str, line: usize) -> Result<ColumnSelector, ParseError> {
    let argument = argument.trim();
    if argument.starts_with('"') {
        return Ok(ColumnSelector::Header(parse_scalar(argument, line)?));
    }
    let one_based = argument.parse::<usize>().map_err(|_| {
        parse_error(
            line,
            "Column(...) expects a 1-based column number or quoted header",
        )
    })?;
    if one_based == 0 {
        return Err(parse_error(line, "column numbers start at 1"));
    }
    Ok(ColumnSelector::Index(one_based - 1))
}

fn parse_range_argument(argument: &str, line: usize) -> Result<String, ParseError> {
    let argument = argument.trim();
    if argument.starts_with('"') {
        return parse_scalar(argument, line);
    }
    if let Some((start, end)) = split_once_ci(argument, " to ") {
        return Ok(format!("{}:{}", start.trim(), end.trim()));
    }
    if argument.is_empty() {
        return Err(parse_error(line, "Cell(...) requires an A1 cell or range"));
    }
    Ok(argument.to_owned())
}

fn parse_assignment_scalar(text: &str, line: usize) -> Result<String, ParseError> {
    let value = text
        .trim()
        .strip_prefix('=')
        .ok_or_else(|| parse_error(line, "expected `=`"))?;
    parse_scalar(value.trim(), line)
}

fn parse_assignment_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let value = text
        .trim()
        .strip_prefix('=')
        .ok_or_else(|| parse_error(line, "expected `=`"))?;
    parse_expression(value.trim(), line)
}

fn parse_scalar(text: &str, line: usize) -> Result<String, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    if !text.starts_with('"') {
        return Ok(text.to_owned());
    }
    let mut value = String::new();
    let mut escaped = false;
    let mut closed = false;
    let mut trailing = String::new();
    for ch in text.chars().skip(1) {
        if closed {
            trailing.push(ch);
            continue;
        }
        if escaped {
            value.push(match ch {
                'n' => '\n',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            closed = true;
            continue;
        }
        value.push(ch);
    }
    if escaped || !closed || !trailing.trim().is_empty() {
        return Err(parse_error(line, "invalid quoted string"));
    }
    Ok(value)
}

fn split_arguments(text: &str, line: usize) -> Result<Vec<&str>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Err(parse_error(line, "unexpected `)` in argument list")),
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let part = text[start..index].trim();
                if part.is_empty() {
                    return Err(parse_error(line, "empty argument"));
                }
                parts.push(part);
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err(parse_error(line, "unterminated quoted string"));
    }
    if depth != 0 {
        return Err(parse_error(line, "unbalanced parentheses in argument list"));
    }
    let part = text[start..].trim();
    if part.is_empty() {
        return Err(parse_error(line, "empty argument"));
    }
    parts.push(part);
    Ok(parts)
}

fn split_member(text: &str) -> Option<(&str, &str)> {
    split_top_level_token(text, ".")
}

fn split_top_level_once(text: &str, target: char) -> Option<(&str, &str)> {
    let token = target.to_string();
    split_top_level_token(text, &token)
}

fn split_top_level_token<'a>(text: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + token.len() <= text.len() {
        let ch = text[index..].chars().next()?;
        if escaped {
            escaped = false;
            index += ch.len_utf8();
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            index += 1;
            continue;
        }
        if !quoted {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 && bytes[index..].starts_with(token.as_bytes()) {
                return Some((&text[..index], &text[index + token.len()..]));
            }
        }
        index += ch.len_utf8();
    }
    None
}

fn split_keyword_top_level<'a>(text: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let keyword_lower = keyword.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(relative) = lower[search_from..].find(&keyword_lower) {
        let index = search_from + relative;
        if is_top_level_index(text, index) {
            return Some((&text[..index], &text[index + keyword.len()..]));
        }
        search_from = index + 1;
    }
    None
}

fn is_top_level_index(text: &str, target: usize) -> bool {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if index >= target {
            break;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth == 0 && !quoted
}

fn strip_outer_parentheses(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim();
        if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }
        if find_closing_parenthesis(trimmed, 1) != Some(trimmed.len() - 1) {
            return trimmed;
        }
        text = &trimmed[1..trimmed.len() - 1];
    }
}

fn parse_column_type(value: &str) -> Option<ColumnType> {
    match value.to_ascii_lowercase().as_str() {
        "string" => Some(ColumnType::String),
        "integer" => Some(ColumnType::Integer),
        "decimal" => Some(ColumnType::Decimal),
        "boolean" => Some(ColumnType::Boolean),
        _ => None,
    }
}

fn looks_like_named_call(text: &str) -> bool {
    text.find('(')
        .is_some_and(|open| is_identifier(text[..open].trim()) && text.trim_end().ends_with(')'))
}

fn validate_identifier(identifier: &str, line: usize) -> Result<(), ParseError> {
    if is_identifier(identifier) {
        Ok(())
    } else {
        Err(parse_error(
            line,
            format!("invalid identifier `{identifier}`"),
        ))
    }
}

fn is_identifier(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn is_terminator(text: &str) -> bool {
    is_end_if(text) || is_end_def(text) || is_end_class(text)
}

fn is_end_if(text: &str) -> bool {
    eq_ci(text, "end if") || eq_ci(text, "endif")
}

fn is_end_def(text: &str) -> bool {
    eq_ci(text, "end def") || eq_ci(text, "enddef")
}

fn is_end_class(text: &str) -> bool {
    eq_ci(text, "end class") || eq_ci(text, "endclass")
}

fn is_rem_comment(text: &str) -> bool {
    eq_ci(text, "rem") || starts_with_ci(text, "rem ")
}

fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ci(text, prefix).then(|| &text[prefix.len()..])
}

fn eq_ci(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn split_once_ci<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let index = lower.find(&separator.to_ascii_lowercase())?;
    Some((&text[..index], &text[index + separator.len()..]))
}

fn parse_error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}
