//! ```text
//! 責務: [display_views: 列の表示書式・View style・行高・条件付き書式をgenerated init DSLの1行宣言として保存・復元する]
//! 処理: [1: 設定を `SET This.Project.Source(..).Worksheet.Display.<Format|Style|RowHeight|Conditional>(key) = ..` に直列化する 2: 同じ文法を読み戻し、style・条件付き書式・行高は各modelの検証を通す 3: 列は見出し名で持つので、列が欠けても設定は捨てず、解決時（display_settings::resolve）に休眠させる]
//! 補足: [bare CSVにはproject DSLが無いので保存しない（session内だけ）。user.rlyは読むだけで書き換えない。保存するのは設定だけで、CSVにも側のfileにも表示結果は保存しない]
//! ```
use std::collections::{BTreeMap, HashSet};

use crate::process::{
    conditional_format::{ConditionalFormat, ConditionalRule},
    display_format::DisplayFormat,
    display_settings::{ColumnFormat, DisplaySettings},
    view_style::{StyleRule, ViewStyle},
};

use super::aggregation_views::{Value, parse_value, single_text};
use super::{
    ProjectInitError, ProjectInitUpdate, StagedProjectInit, declaration_continues_with,
    parse_quoted, quote, read_optional_config, read_safe_init_config,
    stage_generated_configuration, strip_prefix_ci,
};
use crate::project::RowlyProject;
use std::path::Path;

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
    //   責務: [insert: 読み戻した1行をsourceの設定へ入れる]
    //   処理: [同じfile内で同じキー（同じ見出し・行位置・通し番号）が2回出たら、どちらかを黙って捨てずにerrorにする]
    //   戻り値: [Result<(), String>: 成功、またはキー重複のerror]
    // }
    pub(super) fn insert(&mut self, source_id: String, item: DisplayItem) -> Result<(), String> {
        let duplicated = match item {
            DisplayItem::Format { column, format } => self
                .formats
                .insert((source_id, column.clone()), format)
                .is_some()
                .then(|| format!("column `{column}` has more than one display format")),
            DisplayItem::Height { row, millimeters } => self
                .heights
                .insert((source_id, row), millimeters)
                .is_some()
                .then(|| format!("row {row} has more than one row height")),
            DisplayItem::Style { index, rule } => self
                .styles
                .insert((source_id, index), rule)
                .is_some()
                .then(|| format!("style rule number {index} appears more than once")),
            DisplayItem::Conditional { index, rule } => self
                .conditionals
                .insert((source_id, index), rule)
                .is_some()
                .then(|| format!("conditional rule number {index} appears more than once")),
        };
        duplicated.map_or(Ok(()), Err)
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
    declaration_continues_with(line, PREFIX, MIDDLE)
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

use conditional_syntax::{
    SOURCES, interpret_conditional_rule, interpret_style_rule, serialize_predicate,
};
use style_syntax::{
    interpret_format, name_of, parse_index, serialize_format, serialize_style, serialize_target,
};

#[path = "display_views/conditional_syntax.rs"]
mod conditional_syntax;
impl StagedProjectInit {
    // {
    //   責務: [display_settings: staging中のgenerated設定にuser.rlyを重ねて、sourceの表示設定を返す]
    //   戻り値: [Result<DisplaySettings, ProjectInitError>: sourceの設定。user.rlyの読取に失敗したらerror]
    // }
    pub(crate) fn display_settings(
        &self,
        source_id: &str,
    ) -> Result<DisplaySettings, ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        Ok(config.display.source_settings(source_id))
    }
}

// {
//   責務: [stage_generated_display_settings: sourceの列の表示書式・View style・行高・条件付き書式をgenerated設定へstagingする]
//   処理: [渡された設定一式でsourceのgenerated設定を置き換える。user.rlyは読むだけで書かない。列が欠けていても設定は保存し、休眠の判断は解決時（DisplaySettings::resolve）に行う]
//   戻り値: [Result<StagedProjectInit, ProjectInitError>: 新しいstaging、または設定の不備・script検証の失敗]
// }
pub(crate) fn stage_generated_display_settings(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    settings: &DisplaySettings,
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::DisplaySettings(settings),
    )
}

// {
//   責務: [load_display_settings: 保存済みの表示設定をgenerated + user.rlyから読む]
//   処理: [safe initのincludeに従い、user.rlyの項目をgeneratedより優先して返す]
//   戻り値: [Result<DisplaySettings, ProjectInitError>: sourceの設定。未保存なら空]
// }
pub(crate) fn load_display_settings(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
) -> Result<DisplaySettings, ProjectInitError> {
    Ok(read_safe_init_config(project, manifest_path, source_id)?
        .display
        .source_settings(source_id))
}
#[path = "display_views/style_syntax.rs"]
mod style_syntax;
