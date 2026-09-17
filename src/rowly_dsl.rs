use thiserror::Error;

use crate::process::{
    CellRange, ColumnError, ColumnType, ColumnTypeReport, CsvDocument, DocumentError,
    JapaneseCheckReport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    statements: Vec<Statement>,
}

impl Program {
    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    If {
        condition: Condition,
        body: Vec<Statement>,
    },
    SetRangeValue {
        range: CellRange,
        value: String,
    },
    ValidateColumnType {
        selector: ColumnSelector,
        column_type: ColumnType,
    },
    CheckJapanese {
        selector: ColumnSelector,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    ColumnExists {
        selector: ColumnSelector,
    },
    ColumnTitleEquals {
        selector: ColumnSelector,
        expected: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnSelector {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    events: Vec<ExecutionEvent>,
}

impl ExecutionReport {
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionEvent {
    ConditionEvaluated {
        condition: Condition,
        result: bool,
    },
    RangeValueSet {
        range: CellRange,
        value: String,
    },
    ColumnTypeChecked {
        selector: ColumnSelector,
        report: ColumnTypeReport,
    },
    JapaneseChecked {
        selector: ColumnSelector,
        report: JapaneseCheckReport,
    },
}

pub fn parse(source: &str) -> Result<Program, ParseError> {
    Parser::new(source).parse_program()
}

pub fn execute(
    program: &Program,
    document: &mut CsvDocument,
) -> Result<ExecutionReport, ExecutionError> {
    let mut events = Vec::new();
    execute_statements(&program.statements, document, &mut events)?;
    Ok(ExecutionReport { events })
}

pub fn run(source: &str, document: &mut CsvDocument) -> Result<ExecutionReport, DslError> {
    let program = parse(source)?;
    Ok(execute(&program, document)?)
}

fn execute_statements(
    statements: &[Statement],
    document: &mut CsvDocument,
    events: &mut Vec<ExecutionEvent>,
) -> Result<(), ExecutionError> {
    for statement in statements {
        match statement {
            Statement::If { condition, body } => {
                let result = evaluate_condition(condition, document)?;
                events.push(ExecutionEvent::ConditionEvaluated {
                    condition: condition.clone(),
                    result,
                });
                if result {
                    execute_statements(body, document, events)?;
                }
            }
            Statement::SetRangeValue { range, value } => {
                document.set_range_value(*range, value.clone())?;
                events.push(ExecutionEvent::RangeValueSet {
                    range: *range,
                    value: value.clone(),
                });
            }
            Statement::ValidateColumnType {
                selector,
                column_type,
            } => {
                let column = resolve_column(selector, document)?;
                let report = document.validate_column_type(column, *column_type)?;
                events.push(ExecutionEvent::ColumnTypeChecked {
                    selector: selector.clone(),
                    report,
                });
            }
            Statement::CheckJapanese { selector } => {
                let column = resolve_column(selector, document)?;
                let report = document.check_column_japanese(column)?;
                events.push(ExecutionEvent::JapaneseChecked {
                    selector: selector.clone(),
                    report,
                });
            }
        }
    }

    Ok(())
}

fn evaluate_condition(
    condition: &Condition,
    document: &CsvDocument,
) -> Result<bool, ExecutionError> {
    match condition {
        Condition::ColumnExists { selector } => Ok(match selector {
            ColumnSelector::Index(column) => *column < document.column_count(),
            ColumnSelector::Header(header) => !document.column_indices_by_header(header).is_empty(),
        }),
        Condition::ColumnTitleEquals { selector, expected } => {
            let column = resolve_column(selector, document)?;
            Ok(document.cell(0, column) == Some(expected.as_str()))
        }
    }
}

fn resolve_column(selector: &ColumnSelector, document: &CsvDocument) -> Result<usize, ColumnError> {
    match selector {
        ColumnSelector::Index(column) => {
            let column_count = document.column_count();
            if *column >= column_count {
                return Err(ColumnError::ColumnOutOfBounds {
                    column: *column,
                    column_count,
                });
            }
            Ok(*column)
        }
        ColumnSelector::Header(header) => document.column_index_by_header(header),
    }
}

#[derive(Debug, Error)]
pub enum DslError {
    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Execute(#[from] ExecutionError),
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Column(#[from] ColumnError),

    #[error(transparent)]
    Document(#[from] DocumentError),
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
        let statements = self.parse_block(None)?;
        Ok(Program { statements })
    }

    fn parse_block(&mut self, opened_at: Option<usize>) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();

        while let Some(line) = self.lines.get(self.position).cloned() {
            if is_end_if(&line.text) {
                if opened_at.is_none() {
                    return Err(parse_error(line.number, "unexpected `End If`"));
                }
                self.position += 1;
                return Ok(statements);
            }

            if starts_with_ci(&line.text, "if ") {
                let condition = parse_if_condition(&line)?;
                self.position += 1;
                let body = self.parse_block(Some(line.number))?;
                statements.push(Statement::If { condition, body });
                continue;
            }

            statements.push(parse_statement(&line)?);
            self.position += 1;
        }

        if let Some(line) = opened_at {
            return Err(parse_error(line, "missing `End If`"));
        }

        Ok(statements)
    }
}

fn parse_if_condition(line: &SourceLine) -> Result<Condition, ParseError> {
    let lower = line.text.to_ascii_lowercase();
    if !lower.starts_with("if ") || !lower.ends_with(" then") {
        return Err(parse_error(line.number, "expected `If <condition> Then`"));
    }

    let condition = line.text[3..line.text.len() - 5].trim();
    let (selector, remainder) = parse_column_target(condition, line.number)?;

    if eq_ci(remainder, ".exists") {
        return Ok(Condition::ColumnExists { selector });
    }

    if let Some(rest) = strip_prefix_ci(remainder, ".title") {
        let expected = parse_assignment_value(rest, line.number)?;
        return Ok(Condition::ColumnTitleEquals { selector, expected });
    }

    Err(parse_error(
        line.number,
        "supported conditions are `.Exists` and `.Title = <value>`",
    ))
}

fn parse_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if starts_with_ci(&line.text, "this.worksheet.column(") {
        return parse_column_statement(line);
    }
    if starts_with_ci(&line.text, "this.worksheet.editor.cell(") {
        return parse_cell_statement(line);
    }

    Err(parse_error(line.number, "unsupported statement"))
}

fn parse_column_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let (selector, remainder) = parse_column_target(&line.text, line.number)?;

    if let Some(rest) = strip_prefix_ci(remainder, ".type") {
        let value = parse_assignment_value(rest, line.number)?;
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
    let value = parse_assignment_value(rest, line.number)?;
    let range_text = parse_range_argument(argument, line.number)?;
    let range = range_text
        .parse::<CellRange>()
        .map_err(|error| parse_error(line.number, error.to_string()))?;

    Ok(Statement::SetRangeValue { range, value })
}

fn parse_column_target(
    text: &str,
    line: usize,
) -> Result<(ColumnSelector, &str), ParseError> {
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

fn find_closing_parenthesis(text: &str, start: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;

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
        if character == ')' && !quoted {
            return Some(start + offset);
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

fn parse_assignment_value(text: &str, line: usize) -> Result<String, ParseError> {
    let text = text.trim();
    let Some(value) = text.strip_prefix('=') else {
        return Err(parse_error(line, "expected `=`"));
    };
    parse_scalar(value.trim(), line)
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

fn parse_column_type(value: &str) -> Option<ColumnType> {
    match value.to_ascii_lowercase().as_str() {
        "string" => Some(ColumnType::String),
        "integer" => Some(ColumnType::Integer),
        "decimal" => Some(ColumnType::Decimal),
        "boolean" => Some(ColumnType::Boolean),
        _ => None,
    }
}

fn is_end_if(text: &str) -> bool {
    eq_ci(text, "end if") || eq_ci(text, "endif")
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn open(source: &str) -> (tempfile::TempDir, CsvDocument) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, source).unwrap();
        let document = CsvDocument::open(path).unwrap();
        (directory, document)
    }

    #[test]
    fn parses_basic_style_if_column_checks_and_range_set() {
        let program = parse(
            r#"
                If This.Worksheet.Column(1).Title = "名前" Then
                    This.Worksheet.Column(1).Type = String
                    This.Worksheet.Column(1).Check.Japanese
                End If

                This.Worksheet.Editor.Cell(A2 To A3).Value.Set = 8
            "#,
        )
        .unwrap();

        assert_eq!(program.statements().len(), 2);
        let Statement::If { condition, body } = &program.statements()[0] else {
            panic!("expected if statement");
        };
        assert_eq!(
            condition,
            &Condition::ColumnTitleEquals {
                selector: ColumnSelector::Index(0),
                expected: "名前".into(),
            }
        );
        assert_eq!(body.len(), 2);
        assert_eq!(
            program.statements()[1],
            Statement::SetRangeValue {
                range: "A2:A3".parse().unwrap(),
                value: "8".into(),
            }
        );
    }

    #[test]
    fn executes_column_type_and_japanese_checks() {
        let (_directory, mut document) = open("名前,年齢\n田中太郎,20\nAlice,21\n");
        let execution = run(
            r#"
                If This.Worksheet.Column(1).Title = "名前" Then
                    This.Worksheet.Column(1).Type = String
                    This.Worksheet.Column(1).Check.Japanese
                End If
            "#,
            &mut document,
        )
        .unwrap();

        assert_eq!(execution.events().len(), 3);
        assert!(matches!(
            &execution.events()[0],
            ExecutionEvent::ConditionEvaluated { result: true, .. }
        ));
        let ExecutionEvent::ColumnTypeChecked {
            report: type_report,
            ..
        } = &execution.events()[1]
        else {
            panic!("expected type report");
        };
        assert!(type_report.is_valid());
        assert_eq!(type_report.checked_cells(), 2);

        let ExecutionEvent::JapaneseChecked {
            report: japanese_report,
            ..
        } = &execution.events()[2]
        else {
            panic!("expected Japanese report");
        };
        assert_eq!(japanese_report.matches().len(), 1);
        assert_eq!(japanese_report.mismatches().len(), 1);
        assert_eq!(
            japanese_report.mismatches()[0].reference().to_string(),
            "A3"
        );
    }

    #[test]
    fn false_if_condition_skips_body() {
        let (_directory, mut document) = open("氏名\n田中\n");
        let report = run(
            r#"
                If This.Worksheet.Column(1).Title = "名前" Then
                    This.Worksheet.Column(1).Check.Japanese
                End If
            "#,
            &mut document,
        )
        .unwrap();

        assert_eq!(report.events().len(), 1);
        assert!(matches!(
            &report.events()[0],
            ExecutionEvent::ConditionEvaluated { result: false, .. }
        ));
    }

    #[test]
    fn header_selector_and_range_set_use_process_boundary() {
        let (_directory, mut document) = open("名前,年齢\n田中,20\n山田,21\n");
        let report = run(
            r#"
                This.Worksheet.Column("年齢").Type = Integer
                This.Worksheet.Editor.Cell("B2:B3").Value.Set = 30
            "#,
            &mut document,
        )
        .unwrap();

        assert_eq!(report.events().len(), 2);
        assert_eq!(document.cell_a1("B2").unwrap(), Some("30"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("30"));
        assert!(document.can_undo());
        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("20"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("21"));
    }

    #[test]
    fn reports_missing_end_if_at_opening_line() {
        let error = parse(
            r#"
                If This.Worksheet.Column("名前").Exists Then
                    This.Worksheet.Column("名前").Type = String
            "#,
        )
        .unwrap_err();

        assert_eq!(error.line(), 2);
        assert!(error.message().contains("End If"));
    }
}
