use thiserror::Error;

use crate::process::{CellRange, ColumnType};

use super::ast::{
    ColumnSelector, Condition, Expression, FunctionDefinition, Program, Statement,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockEnd {
    If,
    Function,
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
                    return None;
                }
                Some(SourceLine {
                    number: index + 1,
                    text: text.to_owned(),
                })
            })
            .collect();

        Self { lines, position: 0 }
    }

    fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut functions = Vec::new();
        let mut statements = Vec::new();

        while let Some(line) = self.lines.get(self.position).cloned() {
            if starts_with_ci(&line.text, "def ") {
                let function = self.parse_function(&line)?;
                if functions
                    .iter()
                    .any(|existing: &FunctionDefinition| {
                        existing.name.eq_ignore_ascii_case(&function.name)
                    })
                {
                    return Err(parse_error(
                        line.number,
                        format!("duplicate function `{}`", function.name),
                    ));
                }
                functions.push(function);
                continue;
            }
            if is_end_if(&line.text) {
                return Err(parse_error(line.number, "unexpected `End If`"));
            }
            if is_end_def(&line.text) {
                return Err(parse_error(line.number, "unexpected `End Def`"));
            }
            statements.push(self.parse_statement_or_if()?);
        }

        Ok(Program {
            functions,
            statements,
        })
    }

    fn parse_function(&mut self, line: &SourceLine) -> Result<FunctionDefinition, ParseError> {
        let (name, parameters) = parse_function_signature(line)?;
        self.position += 1;
        let body = self.parse_block(BlockEnd::Function, line.number)?;
        Ok(FunctionDefinition {
            name,
            parameters,
            body,
        })
    }

    fn parse_statement_or_if(&mut self) -> Result<Statement, ParseError> {
        let line = self
            .lines
            .get(self.position)
            .cloned()
            .expect("parser position is within the source");

        if starts_with_ci(&line.text, "if ") {
            let condition = parse_if_condition(&line)?;
            self.position += 1;
            let body = self.parse_block(BlockEnd::If, line.number)?;
            return Ok(Statement::If { condition, body });
        }

        if starts_with_ci(&line.text, "def ") {
            return Err(parse_error(
                line.number,
                "function definitions are only allowed at the top level",
            ));
        }

        let statement = parse_statement(&line)?;
        self.position += 1;
        Ok(statement)
    }

    fn parse_block(
        &mut self,
        expected_end: BlockEnd,
        opened_at: usize,
    ) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();

        while let Some(line) = self.lines.get(self.position).cloned() {
            if is_end_if(&line.text) {
                if expected_end != BlockEnd::If {
                    return Err(parse_error(line.number, "unexpected `End If`"));
                }
                self.position += 1;
                return Ok(statements);
            }
            if is_end_def(&line.text) {
                if expected_end != BlockEnd::Function {
                    return Err(parse_error(line.number, "unexpected `End Def`"));
                }
                self.position += 1;
                return Ok(statements);
            }
            statements.push(self.parse_statement_or_if()?);
        }

        let expected = match expected_end {
            BlockEnd::If => "End If",
            BlockEnd::Function => "End Def",
        };
        Err(parse_error(opened_at, format!("missing `{expected}`")))
    }
}

fn parse_function_signature(line: &SourceLine) -> Result<(String, Vec<String>), ParseError> {
    let Some(signature) = strip_prefix_ci(&line.text, "def ") else {
        return Err(parse_error(line.number, "expected `Def name(...)`"));
    };
    let (name, arguments) = parse_named_call(signature.trim(), line.number)?;
    let parameters = split_arguments(arguments, line.number)?
        .into_iter()
        .map(|parameter| {
            let parameter = parameter.trim();
            validate_identifier(parameter, line.number)?;
            Ok(parameter.to_owned())
        })
        .collect::<Result<Vec<_>, ParseError>>()?;

    for (index, parameter) in parameters.iter().enumerate() {
        if parameters[..index]
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(parameter))
        {
            return Err(parse_error(
                line.number,
                format!("duplicate parameter `{parameter}`"),
            ));
        }
    }

    Ok((name, parameters))
}

fn parse_if_condition(line: &SourceLine) -> Result<Condition, ParseError> {
    let lower = line.text.to_ascii_lowercase();
    if !lower.starts_with("if ") || !lower.ends_with(" then") {
        return Err(parse_error(line.number, "expected `If <condition> Then`"));
    }

    let condition = line.text[3..line.text.len() - 5].trim();

    if starts_with_ci(condition, "this.worksheet.column(") {
        let (selector, remainder) = parse_column_target(condition, line.number)?;

        if eq_ci(remainder, ".exists") {
            return Ok(Condition::ColumnExists { selector });
        }

        if let Some(rest) = strip_prefix_ci(remainder, ".title") {
            let expected = parse_assignment_expression(rest, line.number)?;
            return Ok(Condition::ColumnTitleEquals { selector, expected });
        }
    }

    let Some((left, right)) = split_top_level_once(condition, '=') else {
        return Err(parse_error(
            line.number,
            "supported conditions are column `.Exists`, column `.Title = ...`, and value equality",
        ));
    };

    Ok(Condition::ValueEquals {
        left: parse_expression(left.trim(), line.number)?,
        right: parse_expression(right.trim(), line.number)?,
    })
}

fn parse_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if starts_with_ci(&line.text, "let ") {
        return parse_let_statement(line);
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
    if looks_like_named_call(&line.text) {
        let Expression::Call { name, arguments } = parse_expression(&line.text, line.number)? else {
            return Err(parse_error(line.number, "expected a function call"));
        };
        return Ok(Statement::Call { name, arguments });
    }

    Err(parse_error(line.number, "unsupported statement"))
}

fn parse_let_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let rest = strip_prefix_ci(&line.text, "let ")
        .ok_or_else(|| parse_error(line.number, "expected `Let name = value`"))?;
    let Some((name, value)) = split_top_level_once(rest, '=') else {
        return Err(parse_error(line.number, "expected `Let name = value`"));
    };
    let name = name.trim();
    validate_identifier(name, line.number)?;
    Ok(Statement::Let {
        name: name.to_owned(),
        value: parse_expression(value.trim(), line.number)?,
    })
}

fn parse_return_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if eq_ci(&line.text, "return") {
        return Ok(Statement::Return { value: None });
    }
    let rest = strip_prefix_ci(&line.text, "return ")
        .ok_or_else(|| parse_error(line.number, "expected `Return` or `Return value`"))?;
    Ok(Statement::Return {
        value: Some(parse_expression(rest.trim(), line.number)?),
    })
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
    let prefix = "this.worksheet.editor.cell(";
    let (argument, remainder) = parse_call(&line.text, prefix, line.number)?;
    if !starts_with_ci(remainder, ".value.set") {
        return Err(parse_error(
            line.number,
            "expected `.Value.Set = <value>` after Cell(...) ",
        ));
    }

    let rest = &remainder[".value.set".len()..];
    let value = parse_assignment_expression(rest, line.number)?;
    let range_text = parse_range_argument(argument, line.number)?;
    let range = range_text
        .parse::<CellRange>()
        .map_err(|error| parse_error(line.number, error.to_string()))?;

    Ok(Statement::SetRangeValue { range, value })
}

fn parse_column_target(text: &str, line: usize) -> Result<(ColumnSelector, &str), ParseError> {
    let prefix = "this.worksheet.column(";
    let (argument, remainder) = parse_call(text, prefix, line)?;
    let selector = parse_column_selector(argument, line)?;
    Ok((selector, remainder))
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
    let Some(open) = text.find('(') else {
        return Err(parse_error(line, "expected function call parentheses"));
    };
    let name = text[..open].trim();
    validate_identifier(name, line)?;
    let close = find_closing_parenthesis(text, open + 1)
        .ok_or_else(|| parse_error(line, "missing closing `)`"))?;
    if !text[close + 1..].trim().is_empty() {
        return Err(parse_error(line, "unexpected text after function call"));
    }
    Ok((name.to_owned(), &text[open + 1..close]))
}

fn find_closing_parenthesis(text: &str, start: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (offset, character) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }

        match character {
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
    let text = text.trim();
    let Some(value) = text.strip_prefix('=') else {
        return Err(parse_error(line, "expected `=`"));
    };
    parse_scalar(value.trim(), line)
}

fn parse_assignment_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    let Some(value) = text.strip_prefix('=') else {
        return Err(parse_error(line, "expected `=`"));
    };
    parse_expression(value.trim(), line)
}

fn parse_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    if text.starts_with('"') {
        return Ok(Expression::Literal(parse_scalar(text, line)?));
    }
    if looks_like_named_call(text) {
        let (name, arguments) = parse_named_call(text, line)?;
        let arguments = split_arguments(arguments, line)?
            .into_iter()
            .map(|argument| parse_expression(argument.trim(), line))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Expression::Call { name, arguments });
    }
    if eq_ci(text, "true") || eq_ci(text, "false") {
        return Ok(Expression::Literal(text.to_ascii_lowercase()));
    }
    if is_identifier(text) {
        return Ok(Expression::Variable(text.to_owned()));
    }
    Ok(Expression::Literal(text.to_owned()))
}

fn parse_scalar(text: &str, line: usize) -> Result<String, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    if !text.starts_with('"') {
        return Ok(text.to_owned());
    }

    let mut characters = text.chars();
    let _ = characters.next();
    let mut value = String::new();
    let mut escaped = false;
    let mut closed = false;
    let mut trailing = String::new();

    for character in characters {
        if closed {
            trailing.push(character);
            continue;
        }
        if escaped {
            value.push(match character {
                'n' => '\n',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            closed = true;
            continue;
        }
        value.push(character);
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

    let mut arguments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }

        match character {
            '(' => depth += 1,
            ')' if depth == 0 => {
                return Err(parse_error(line, "unexpected `)` in argument list"));
            }
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let argument = text[start..index].trim();
                if argument.is_empty() {
                    return Err(parse_error(line, "empty function argument"));
                }
                arguments.push(argument);
                start = index + character.len_utf8();
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

    let argument = text[start..].trim();
    if argument.is_empty() {
        return Err(parse_error(line, "empty function argument"));
    }
    arguments.push(argument);
    Ok(arguments)
}

fn split_top_level_once(text: &str, target: char) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }

        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if character == target && depth == 0 => {
                return Some((&text[..index], &text[index + character.len_utf8()..]));
            }
            _ => {}
        }
    }

    None
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
    let Some(open) = text.find('(') else {
        return false;
    };
    is_identifier(text[..open].trim()) && text.trim_end().ends_with(')')
}

fn validate_identifier(identifier: &str, line: usize) -> Result<(), ParseError> {
    if is_identifier(identifier) {
        return Ok(());
    }
    Err(parse_error(
        line,
        format!("invalid identifier `{identifier}`"),
    ))
}

fn is_identifier(identifier: &str) -> bool {
    let mut characters = identifier.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if first != '_' && !first.is_alphabetic() {
        return false;
    }
    characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn is_end_if(text: &str) -> bool {
    eq_ci(text, "end if") || eq_ci(text, "endif")
}

fn is_end_def(text: &str) -> bool {
    eq_ci(text, "end def") || eq_ci(text, "enddef")
}

fn is_rem_comment(text: &str) -> bool {
    eq_ci(text, "rem") || starts_with_ci(text, "rem ")
}

fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ci(text, prefix).then(|| &text[prefix.len()..])
}

fn eq_ci(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn split_once_ci<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let separator = separator.to_ascii_lowercase();
    let index = lower.find(&separator)?;
    Some((&text[..index], &text[index + separator.len()..]))
}

fn parse_error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}
