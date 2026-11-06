//! Safe, declarative project init scripts for persistent Viewer configuration.

use std::{fs, io, path::Path};

use thiserror::Error;

use crate::{
    process::aggregation_definition::AggregationDefinition,
    process::calculation::{CalculationEngine, CalculationTarget, CalculationTrigger},
    process::{CellRef, CsvDocument},
    process::{chart::ChartDefinition, display_settings::DisplaySettings},
    project::{ProjectError, RowlyProject, write_project_file_atomic},
};

#[path = "project_init/validation_rules.rs"]
mod validation_rules;
use validation_rules::{
    parse_project_validation_declaration, serialize_validation_expression,
    validation_statement_needs_continuation,
};

#[path = "project_init/paths.rs"]
mod paths;
use paths::{
    create_if_missing, create_parent, ensure_distinct_paths,
    ensure_generated_path_does_not_alias_project_data, project_script_path, read_optional_config,
    script_read_error, validate_regular_file_if_present,
};

const INIT_TEMPLATE: &str = "INCLUDE GENERATED\nINCLUDE USER\n";

#[derive(Debug, Clone)]
// {
//   責務: [StagedProjectInit: generated.rlyへ未保存のproject init設定と確定先pathを保持する]
//   フィールド: [config: 保存するgenerated設定, init_path/generated_path/user_path: 適用先project script]
// }
pub(crate) struct StagedProjectInit {
    config: ProjectInitConfig,
    init_path: std::path::PathBuf,
    generated_path: std::path::PathBuf,
    user_path: std::path::PathBuf,
}

#[derive(Debug)]
// {
//   責務: [ProjectInitSnapshot: generated設定確定前のscript状態を保持し、失敗時の復元に使う]
//   フィールド: [files: script pathごとの保存前bytes]
// }
pub(crate) struct ProjectInitSnapshot {
    files: Vec<ProjectInitFileSnapshot>,
}

#[derive(Debug)]
// {
//   責務: [ProjectInitFileSnapshot: ひとつのproject init scriptの保存前bytesを保持する]
//   フィールド: [path: script path, bytes: 保存前bytes。fileが未作成ならNone。]
// }
struct ProjectInitFileSnapshot {
    path: std::path::PathBuf,
    bytes: Option<Vec<u8>>,
}

// {
//   責務: [ProjectInitUpdate: ひとつのgenerated設定カテゴリでsession configを更新する]
//   選択肢: [ColumnTypes: 列型, ValidationRules: validation, CalculationBindings: Calculation Binding, AggregationViews: Group By / Pivot定義]
// }
#[derive(Clone, Copy)]
enum ProjectInitUpdate<'a> {
    ColumnTypes(&'a CsvDocument),
    ValidationRules(&'a CsvDocument),
    CalculationBindings(&'a CalculationEngine),
    AggregationViews(&'a [AggregationDefinition]),
    ChartViews(&'a [ChartDefinition]),
    DisplaySettings(&'a DisplaySettings),
}

impl ProjectInitSnapshot {
    pub(crate) fn restore(&self) -> Result<(), ProjectInitError> {
        for file in &self.files {
            match &file.bytes {
                Some(bytes) => {
                    create_parent(&file.path)?;
                    write_project_file_atomic(&file.path, bytes)?;
                }
                None => match fs::remove_file(&file.path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(ProjectInitError::ScriptWrite {
                            path: file.path.display().to_string(),
                            message: error.to_string(),
                        });
                    }
                },
            }
        }
        Ok(())
    }
}

impl StagedProjectInit {
    pub(crate) fn validate_for_project(
        &self,
        project: &RowlyProject,
        manifest_path: &Path,
    ) -> Result<(), ProjectInitError> {
        let init_path = project_script_path(manifest_path, &project.scripts.init)?;
        let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
        let user_path = project_script_path(manifest_path, &project.scripts.user)?;
        ensure_distinct_paths(&init_path, &generated_path, &user_path)?;
        ensure_generated_path_does_not_alias_project_data(
            project,
            manifest_path,
            &init_path,
            &generated_path,
            &user_path,
        )?;
        if self.init_path != init_path
            || self.generated_path != generated_path
            || self.user_path != user_path
        {
            return Err(ProjectInitError::StagedPathChanged);
        }
        for path in [&init_path, &generated_path, &user_path] {
            validate_regular_file_if_present(path)?;
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<ProjectInitSnapshot, ProjectInitError> {
        let mut files = Vec::new();
        for path in [&self.init_path, &self.user_path, &self.generated_path] {
            let bytes = match fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(script_read_error(path, error)),
            };
            files.push(ProjectInitFileSnapshot {
                path: path.clone(),
                bytes,
            });
        }
        Ok(ProjectInitSnapshot { files })
    }

    pub(crate) fn commit(&self) -> Result<ProjectInitSnapshot, ProjectInitError> {
        let snapshot = self.snapshot()?;
        let result = (|| {
            create_if_missing(&self.user_path, "")?;
            create_if_missing(&self.init_path, INIT_TEMPLATE)?;
            create_parent(&self.generated_path)?;
            write_project_file_atomic(&self.generated_path, self.config.serialize().as_bytes())?;
            Ok::<(), ProjectInitError>(())
        })();
        if let Err(error) = result {
            if let Err(rollback) = snapshot.restore() {
                return Err(ProjectInitError::CommitRollback {
                    primary: error.to_string(),
                    rollback: rollback.to_string(),
                });
            }
            return Err(error);
        }
        Ok(snapshot)
    }

    pub(crate) fn apply_source(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        config.apply_source(source_id, document)
    }

    pub(crate) fn chart_views(
        &self,
        source_id: &str,
    ) -> Result<Vec<ChartDefinition>, ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        Ok(config.source_chart_views(source_id))
    }

    pub(crate) fn display_settings(
        &self,
        source_id: &str,
    ) -> Result<DisplaySettings, ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        Ok(config.display.source_settings(source_id))
    }

    pub(crate) fn aggregation_views(
        &self,
        source_id: &str,
    ) -> Result<Vec<AggregationDefinition>, ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        Ok(config.source_aggregation_views(source_id))
    }

    pub(crate) fn apply_calculation_bindings(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
        engine: &mut CalculationEngine,
    ) -> Result<(), ProjectInitError> {
        let mut config = self.config.clone();
        config.apply_user_overrides(read_optional_config(&self.user_path)?);
        apply_config_calculation_bindings(&config, source_id, document, engine)
    }
}

#[path = "project_init/config.rs"]
mod config;
use config::ProjectInitConfig;

#[path = "project_init/script_includes.rs"]
mod script_includes;
use script_includes::{
    InitInclude, is_rem_comment, parse_column_type_declaration, parse_init_includes, parse_quoted,
    quote, read_safe_init_config, strip_prefix_ci,
};

#[path = "project_init/aggregation_views.rs"]
mod aggregation_views;
use aggregation_views::{
    AggregationViews, ChartViews, is_aggregation_declaration, parse_aggregation_declaration,
};

#[path = "project_init/chart_views.rs"]
mod chart_views;
use chart_views::{is_chart_declaration, parse_chart_declaration};

#[path = "project_init/display_views.rs"]
mod display_views;
use display_views::{DisplayStore, is_display_declaration, parse_display_declaration};

#[path = "project_init/calculation_bindings.rs"]
mod calculation_bindings;
use calculation_bindings::{
    ProjectCalculationBinding, calculation_target_contains, calculation_targets_overlap,
    expand_project_calculation_target, is_project_calculation_declaration,
    parse_project_calculation_declaration, serialize_calculation_target,
};

pub(crate) fn user_defines_calculation_binding(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    target: CalculationTarget,
) -> Result<bool, ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;
    for path in [&init_path, &generated_path, &user_path] {
        validate_regular_file_if_present(path)?;
    }
    let init_source = match fs::read_to_string(&init_path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(script_read_error(&init_path, error)),
    };
    let includes = parse_init_includes(&init_source, &init_path)?;
    if !includes
        .iter()
        .any(|include| matches!(include, InitInclude::User))
    {
        return Ok(false);
    }
    let user_config = read_optional_config(&user_path)?;
    Ok(user_config
        .calculation_bindings
        .keys()
        .any(|(entry_source, user_target)| {
            entry_source == source_id && calculation_targets_overlap(target, *user_target)
        }))
}

pub(crate) fn stage_generated_column_types(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &CsvDocument,
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::ColumnTypes(document),
    )
}

pub(crate) fn stage_generated_validation_rules(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &CsvDocument,
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::ValidationRules(document),
    )
}

pub(crate) fn stage_generated_calculation_bindings(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    _document: &CsvDocument,
    engine: &CalculationEngine,
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::CalculationBindings(engine),
    )
}

// {
//   責務: [stage_generated_aggregation_views: sourceのGroup By / Pivot定義をgenerated設定へstagingする]
//   処理: [渡された定義の集合でsourceのgenerated定義を置き換える。user.rlyは読むだけで書かない。列が欠けていても定義は保存し、休眠の判断は解決時に行う]
//   引数: [project/manifest_path: 対象project, source_id: 対象source, definitions: 保存する全定義, staged: 既存のstaging]
//   戻り値: [Result<StagedProjectInit, ProjectInitError>: 新しいstaging、または名前の不備・script検証の失敗]
// }
pub(crate) fn stage_generated_aggregation_views(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    definitions: &[AggregationDefinition],
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::AggregationViews(definitions),
    )
}

// {
//   責務: [stage_generated_chart_views: sourceのChart定義をgenerated設定へstagingする]
//   処理: [渡された定義の集合でsourceのgenerated Chart定義を置き換える。user.rlyは読むだけで書かない。data sourceや列が欠けていても定義は保存し、解決の可否はchart::resolveが判断する]
//   戻り値: [Result<StagedProjectInit, ProjectInitError>: 新しいstaging、または名前・data sourceの不備・script検証の失敗]
// }
pub(crate) fn stage_generated_chart_views(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    definitions: &[ChartDefinition],
    staged: Option<&StagedProjectInit>,
) -> Result<StagedProjectInit, ProjectInitError> {
    stage_generated_configuration(
        project,
        manifest_path,
        source_id,
        staged,
        ProjectInitUpdate::ChartViews(definitions),
    )
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

// {
//   責務: [load_chart_views: 保存済みのChart定義をgenerated + user.rlyから読む。user.rlyの同名定義を優先して名前順に返す]
// }
pub(crate) fn load_chart_views(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
) -> Result<Vec<ChartDefinition>, ProjectInitError> {
    Ok(read_safe_init_config(project, manifest_path, source_id)?.source_chart_views(source_id))
}

// {
//   責務: [load_aggregation_views: 保存済みのGroup By / Pivot定義をgenerated + user.rlyから読む]
//   処理: [safe initのincludeに従い、user.rlyの同名定義をgeneratedより優先して名前順に返す]
//   戻り値: [Result<Vec<AggregationDefinition>, ProjectInitError>: sourceの定義。未保存なら空]
// }
pub(crate) fn load_aggregation_views(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
) -> Result<Vec<AggregationDefinition>, ProjectInitError> {
    Ok(read_safe_init_config(project, manifest_path, source_id)?
        .source_aggregation_views(source_id))
}

fn stage_generated_configuration(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    staged: Option<&StagedProjectInit>,
    update: ProjectInitUpdate<'_>,
) -> Result<StagedProjectInit, ProjectInitError> {
    if source_id.trim().is_empty() {
        return Err(ProjectInitError::EmptySourceId);
    }
    let init_path = project_script_path(manifest_path, &project.scripts.init)?;
    let generated_path = project_script_path(manifest_path, &project.scripts.generated)?;
    let user_path = project_script_path(manifest_path, &project.scripts.user)?;
    ensure_distinct_paths(&init_path, &generated_path, &user_path)?;
    ensure_generated_path_does_not_alias_project_data(
        project,
        manifest_path,
        &init_path,
        &generated_path,
        &user_path,
    )?;
    for path in [&init_path, &generated_path, &user_path] {
        validate_regular_file_if_present(path)?;
    }

    let mut config = match staged {
        Some(staged)
            if staged.init_path == init_path
                && staged.generated_path == generated_path
                && staged.user_path == user_path =>
        {
            staged.config.clone()
        }
        Some(_) => return Err(ProjectInitError::StagedPathChanged),
        None => read_optional_config(&generated_path)?,
    };
    let user_config = read_optional_config(&user_path)?;
    match update {
        ProjectInitUpdate::ColumnTypes(document) => {
            config.update_column_types(source_id, document, &user_config);
        }
        ProjectInitUpdate::ValidationRules(document) => {
            config.update_validation_rules(source_id, document, &user_config)?;
        }
        ProjectInitUpdate::AggregationViews(definitions) => {
            config.update_aggregation_views(source_id, definitions, &user_config)?;
        }
        ProjectInitUpdate::ChartViews(definitions) => {
            config.update_chart_views(source_id, definitions, &user_config)?;
        }
        ProjectInitUpdate::DisplaySettings(settings) => {
            config
                .display
                .update(source_id, settings, &user_config.display)?;
        }
        ProjectInitUpdate::CalculationBindings(engine) => {
            config.update_calculation_bindings(source_id, engine, &user_config)?;
        }
    }
    match fs::read_to_string(&init_path) {
        Ok(init_source) => {
            parse_init_includes(&init_source, &init_path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(script_read_error(&init_path, error)),
    }
    Ok(StagedProjectInit {
        config,
        init_path,
        generated_path,
        user_path,
    })
}

/// Apply only the config-only statements referenced by `init.rly`.
/// Arbitrary DSL, macro execution, and filesystem operations are rejected.
pub(crate) fn apply_safe_init(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &mut CsvDocument,
) -> Result<(), ProjectInitError> {
    let config = read_safe_init_config(project, manifest_path, source_id)?;
    config.apply_source(source_id, document)
}

pub(crate) fn apply_safe_init_calculation_bindings(
    project: &RowlyProject,
    manifest_path: &Path,
    source_id: &str,
    document: &mut CsvDocument,
    engine: &mut CalculationEngine,
) -> Result<(), ProjectInitError> {
    let config = read_safe_init_config(project, manifest_path, source_id)?;
    apply_config_calculation_bindings(&config, source_id, document, engine)
}

fn apply_config_calculation_bindings(
    config: &ProjectInitConfig,
    source_id: &str,
    document: &mut CsvDocument,
    engine: &mut CalculationEngine,
) -> Result<(), ProjectInitError> {
    let mut dsl = String::new();
    for ((entry_source, target), binding) in &config.calculation_bindings {
        if entry_source != source_id {
            continue;
        }
        for target_cell in expand_project_calculation_target(*target) {
            if document.cell_ref(target_cell).is_none() {
                return Err(ProjectInitError::MissingCalculationTarget(target_cell));
            }
        }
        let (target_kind, target_text) = serialize_calculation_target(*target);
        let expression = binding
            .expression
            .as_deref()
            .ok_or_else(|| ProjectInitError::MissingCalculationExpression(target_text.clone()))?;
        dsl.push_str("SET This.Worksheet.Editor.");
        dsl.push_str(target_kind);
        dsl.push('(');
        dsl.push_str(&quote(&target_text));
        dsl.push_str(").Calculation.Expression = ");
        dsl.push_str(expression);
        dsl.push('\n');
        dsl.push_str("SET This.Worksheet.Editor.");
        dsl.push_str(target_kind);
        dsl.push('(');
        dsl.push_str(&quote(&target_text));
        dsl.push_str(").Calculation.Trigger = ");
        dsl.push_str(
            match binding
                .trigger
                .unwrap_or(CalculationTrigger::DependencyChange)
            {
                CalculationTrigger::DependencyChange => "DependencyChange",
                CalculationTrigger::Manual => "Manual",
            },
        );
        dsl.push('\n');
    }
    if !dsl.is_empty() {
        crate::rowly_dsl::run_with_calculation_engine(&dsl, document, engine)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProjectInitError {
    #[error("project source id must not be empty")]
    EmptySourceId,
    #[error("project init script path `{0}` must be project-relative and must not traverse links")]
    UnsafeScriptPath(String),
    #[error("project init script paths collide")]
    ScriptPathCollision,
    #[error("generated script path `{0}` aliases project data or another project file")]
    GeneratedPathAliasesProjectData(String),
    #[error("project init script target `{0}` must be a regular file")]
    NotRegularScriptFile(String),
    #[error("project column type cannot target duplicate header `{0}`")]
    AmbiguousHeader(String),
    #[error("project validation target `{0}` resolves to more than one column")]
    DuplicateValidationTarget(String),
    #[error("column index {0} has no stable header identity for project persistence")]
    UnstableValidationTarget(usize),
    #[error("project persistence only supports single-cell calculation targets")]
    UnsupportedCalculationTarget,
    #[error("calculation target cell `{0}` does not exist in the project source")]
    MissingCalculationTarget(CellRef),
    #[error("calculation target `{0}` has a trigger but no expression")]
    MissingCalculationExpression(String),
    #[error("invalid aggregation definition: {0}")]
    InvalidAggregationDefinition(String),
    #[error("invalid chart definition: {0}")]
    InvalidChartDefinition(String),
    #[error("invalid display settings: {0}")]
    InvalidDisplaySettings(String),
    #[error(transparent)]
    Dsl(#[from] crate::rowly_dsl::DslError),
    #[error(transparent)]
    Document(#[from] crate::process::DocumentError),
    #[error("failed to read project init script `{path}`: {message}")]
    ScriptRead { path: String, message: String },
    #[error("failed to write project init script `{path}`: {message}")]
    ScriptWrite { path: String, message: String },
    #[error(
        "failed to restore project init after commit error: {primary}; rollback failed: {rollback}"
    )]
    CommitRollback { primary: String, rollback: String },
    #[error("project script paths changed while generated configuration was staged")]
    StagedPathChanged,
    #[error("invalid project init script `{path}` at line {line}: {message}")]
    InvalidInit {
        path: String,
        line: usize,
        message: String,
    },
    #[error("invalid project config script `{path}` at line {line}: {message}")]
    InvalidConfig {
        path: String,
        line: usize,
        message: String,
    },
    #[error(transparent)]
    Project(#[from] ProjectError),
}
