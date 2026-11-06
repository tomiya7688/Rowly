use crate::{
    process::{
        CsvDocument,
        aggregation_definition::AggregationDefinition,
        calculation::{CalculationEngine, CalculationTarget},
        chart::ChartDefinition,
        display_settings::DisplaySettings,
    },
    project::{SourceKind, resolve_project_reference},
    project_init,
};

use super::{ProjectSession, ProjectSessionError};

impl ProjectSession {
    /// ```text
    /// 責務: [user_defines_calculation_binding: GUI編集targetと重なるuser.rlyの宣言を確認する]
    /// 処理: [project initの安全なinclude設定を読み、user側のcell/range宣言との重なりを調べる]
    /// 引数: [source_id: 対象project source, target: GUIが編集するcellまたはrange]
    /// 戻り値: [Result<bool, ProjectSessionError>: user.rlyに重なる宣言があればtrue]
    /// エラー: [ProjectSessionError: sourceまたはscript path・内容の検証失敗]
    /// ```
    pub fn user_defines_calculation_binding(
        &self,
        source_id: &str,
        target: CalculationTarget,
    ) -> Result<bool, ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        Ok(project_init::user_defines_calculation_binding(
            &self.project,
            self.manifest_path(),
            source_id,
            target,
        )?)
    }

    /// ```text
    /// 責務: [save_generated_column_types: source documentのcolumn type設定をgenerated init scriptへstagingする]
    /// 処理: [source idを確認し、generated設定をsession内へ保持する]
    /// 引数: [source_id: project source id, document: type宣言を抽出するCSV document]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated設定をsession内へ保持しstructure / config dirtyを立てる]
    /// エラー: [ProjectSessionError: source id、設定検証、既存script読込またはscript path検証の失敗]
    /// ```
    pub fn save_generated_column_types(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        let staged = project_init::stage_generated_column_types(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [save_generated_validation_rules: sourceのvalidation ruleをgenerated init DSLへstagingする]
    /// 処理: [source idを確認し、generated設定をsession内へ保持する]
    /// 引数: [source_id: project source id, document: validation ruleを抽出するCSV document]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated設定をsession内へ保持しstructure / config dirtyを立てる]
    /// エラー: [ProjectSessionError: source id、設定検証、既存script読込またはscript path検証の失敗]
    /// ```
    pub fn save_generated_validation_rules(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        let staged = project_init::stage_generated_validation_rules(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [save_generated_calculation_bindings: sourceのCalculation Bindingをgenerated init DSLへstagingする]
    /// 処理: [source idを確認し、engineのexpressionとtriggerをgenerated configへ保持する]
    /// 引数: [source_id: project source id, document: target検証に使うcanonical CSV, engine: 保存するbinding state]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated configをsession内へ保持しproject config dirtyを立てる。user.rlyは書き換えない]
    /// エラー: [ProjectSessionError: source、target、bindingまたはscript保存の失敗]
    /// ```
    pub fn save_generated_calculation_bindings(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
        engine: &CalculationEngine,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        let staged = project_init::stage_generated_calculation_bindings(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
            engine,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [save_generated_aggregation_views: sourceのGroup By / Pivot定義をgenerated init DSLへstagingする]
    /// 処理: [source idを確認し、渡された定義の全集合でsourceのgenerated定義を置き換えてsession内へ保持する。bare CSVにはprojectが無いのでこのAPIは使えず、定義はsession内だけに留まる]
    /// 引数: [source_id: project source id, definitions: 保存する全定義]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated設定をsession内へ保持しproject config dirtyを立てる。user.rlyは書き換えない]
    /// エラー: [ProjectSessionError: source、定義の名前、script検証の失敗]
    /// ```
    pub fn save_generated_aggregation_views(
        &mut self,
        source_id: &str,
        definitions: &[AggregationDefinition],
    ) -> Result<(), ProjectSessionError> {
        self.require_source(source_id)?;
        let staged = project_init::stage_generated_aggregation_views(
            &self.project,
            self.manifest_path(),
            source_id,
            definitions,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [aggregation_views: sourceのGroup By / Pivot定義を読む]
    /// 処理: [staged configがあればuser overrideを重ね、なければproject DSLから読む。列の有無は見ない（休眠の判断はresolveで行う）]
    /// 引数: [source_id: project source id]
    /// 戻り値: [Result<Vec<AggregationDefinition>, ProjectSessionError>: 名前順の定義。未保存なら空]
    /// ```
    pub fn aggregation_views(
        &self,
        source_id: &str,
    ) -> Result<Vec<AggregationDefinition>, ProjectSessionError> {
        self.require_source(source_id)?;
        if let Some(staged_init) = &self.staged_init {
            Ok(staged_init.aggregation_views(source_id)?)
        } else {
            Ok(project_init::load_aggregation_views(
                &self.project,
                self.manifest_path(),
                source_id,
            )?)
        }
    }

    /// ```text
    /// 責務: [save_generated_chart_views: sourceのChart定義をgenerated init DSLへstagingする]
    /// 処理: [source idを確認し、渡された定義の全集合でsourceのgenerated Chart定義を置き換えてsession内へ保持する。data sourceや列が欠けていても定義は捨てない。bare CSVにはprojectが無いのでこのAPIは使えず、定義はsession内だけに留まる]
    /// 引数: [source_id: Chartの元になるproject source id, definitions: 保存する全定義]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated設定をsession内へ保持しproject config dirtyを立てる。user.rlyは書き換えない]
    /// エラー: [ProjectSessionError: source、定義の名前・data source、script検証の失敗]
    /// ```
    pub fn save_generated_chart_views(
        &mut self,
        source_id: &str,
        definitions: &[ChartDefinition],
    ) -> Result<(), ProjectSessionError> {
        self.require_source(source_id)?;
        let staged = project_init::stage_generated_chart_views(
            &self.project,
            self.manifest_path(),
            source_id,
            definitions,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [chart_views: sourceのChart定義を読む]
    /// 処理: [staged configがあればuser overrideを重ね、なければproject DSLから読む。data sourceや列の有無は見ない（解決はchart::resolveで行う）]
    /// 引数: [source_id: project source id]
    /// 戻り値: [Result<Vec<ChartDefinition>, ProjectSessionError>: 名前順の定義。未保存なら空]
    /// ```
    pub fn chart_views(
        &self,
        source_id: &str,
    ) -> Result<Vec<ChartDefinition>, ProjectSessionError> {
        self.require_source(source_id)?;
        if let Some(staged_init) = &self.staged_init {
            Ok(staged_init.chart_views(source_id)?)
        } else {
            Ok(project_init::load_chart_views(
                &self.project,
                self.manifest_path(),
                source_id,
            )?)
        }
    }

    /// ```text
    /// 責務: [save_generated_display_settings: sourceの列の表示書式・View style・行高・条件付き書式をgenerated init DSLへstagingする]
    /// 処理: [source idを確認し、渡された設定一式でsourceのgenerated設定を置き換えてsession内へ保持する。列が欠けていても設定は捨てない。bare CSVにはprojectが無いのでこのAPIは使えず、設定はsession内だけに留まる]
    /// 引数: [source_id: project source id, settings: 保存する設定の全集合]
    /// 戻り値: [Result<(), ProjectSessionError>: staging成功または設定error]
    /// 副作用: [generated設定をsession内へ保持しproject config dirtyを立てる。user.rlyは書き換えない]
    /// エラー: [ProjectSessionError: source、設定の不備（重複・空の見出し、検証に通らないstyle・条件付き書式・行高）、script検証の失敗]
    /// ```
    pub fn save_generated_display_settings(
        &mut self,
        source_id: &str,
        settings: &DisplaySettings,
    ) -> Result<(), ProjectSessionError> {
        self.require_source(source_id)?;
        let staged = project_init::stage_generated_display_settings(
            &self.project,
            self.manifest_path(),
            source_id,
            settings,
            self.staged_init.as_ref(),
        )?;
        self.staged_init = Some(staged);
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// ```text
    /// 責務: [display_settings: sourceの表示設定を読む]
    /// 処理: [staged configがあればuser overrideを重ね、なければproject DSLから読む。列の有無は見ない（解決はDisplaySettings::resolveで行う）]
    /// 引数: [source_id: project source id]
    /// 戻り値: [Result<DisplaySettings, ProjectSessionError>: 列の表示書式は見出し順、style・条件付き書式は保存順。未保存なら空]
    /// ```
    pub fn display_settings(
        &self,
        source_id: &str,
    ) -> Result<DisplaySettings, ProjectSessionError> {
        self.require_source(source_id)?;
        if let Some(staged_init) = &self.staged_init {
            Ok(staged_init.display_settings(source_id)?)
        } else {
            Ok(project_init::load_display_settings(
                &self.project,
                self.manifest_path(),
                source_id,
            )?)
        }
    }

    fn require_source(&self, source_id: &str) -> Result<(), ProjectSessionError> {
        if self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            Ok(())
        } else {
            Err(ProjectSessionError::SourceNotFound(source_id.to_owned()))
        }
    }

    /// ```text
    /// 責務: [open_source_document: 宣言済みfile sourceを開きsafe init configを適用する]
    /// 処理: [source idと種別を確認し、sidecar metadataを読まずresolved CSVへstagedまたはdisk上のconfigを適用する]
    /// 引数: [source_id: 開くfile sourceのstable id]
    /// 戻り値: [Result<CsvDocument, ProjectSessionError>: safe init適用後のCSV document]
    /// エラー: [ProjectSessionError: source不明、directory指定、CSV open、またはsafe initの失敗]
    /// ```
    pub fn open_source_document(
        &self,
        source_id: &str,
    ) -> Result<CsvDocument, ProjectSessionError> {
        let source = self
            .project
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .ok_or_else(|| ProjectSessionError::SourceNotFound(source_id.to_owned()))?;
        if source.kind != SourceKind::File {
            return Err(ProjectSessionError::DirectorySource(source_id.to_owned()));
        }
        let path = resolve_project_reference(self.manifest_path(), &source.path);
        let mut document = CsvDocument::open_without_metadata(path)?;
        self.apply_safe_init(source_id, &mut document)?;
        Ok(document)
    }

    /// ```text
    /// 責務: [open_source_with_calculations: source CSV、safe init metadata、Calculation Bindingを復元する]
    /// 処理: [source documentを開き、stagedまたはproject DSLのbindingをengineへ登録して全件再計算する]
    /// 引数: [source_id: 開くfile sourceのstable id]
    /// 戻り値: [Result<(CsvDocument, CalculationEngine), ProjectSessionError>: CSVと復元済みcalculation state]
    /// 副作用: [session内の一時CalculationEngineへbindingとderived resultを設定する。CSV値は変更しない]
    /// エラー: [ProjectSessionError: source、safe init、DSL bindingまたは再計算の失敗]
    /// ```
    pub fn open_source_with_calculations(
        &self,
        source_id: &str,
    ) -> Result<(CsvDocument, CalculationEngine), ProjectSessionError> {
        let mut document = self.open_source_document(source_id)?;
        let mut engine = CalculationEngine::default();
        if let Some(staged_init) = &self.staged_init {
            staged_init.apply_calculation_bindings(source_id, &mut document, &mut engine)?;
        } else {
            project_init::apply_safe_init_calculation_bindings(
                &self.project,
                self.manifest_path(),
                source_id,
                &mut document,
                &mut engine,
            )?;
        }
        engine.recalculate_all(&document)?;
        Ok((document, engine))
    }

    /// ```text
    /// 責務: [apply_safe_init: 対象documentへsupported project configを適用する]
    /// 処理: [staged configがあればuser overrideを重ね、なければconfig-only initを読む]
    /// 引数: [source_id: 設定を選ぶsource id, document: 設定を適用するmutable CSV document]
    /// 戻り値: [Result<(), ProjectSessionError>: config適用成功またはerror]
    /// 副作用: [supported configに基づきdocument metadata等を更新する]
    /// エラー: [ProjectSessionError: sourceまたはinit configの検証・適用失敗]
    /// ```
    pub fn apply_safe_init(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if let Some(staged_init) = &self.staged_init {
            staged_init.apply_source(source_id, document)?;
        } else {
            project_init::apply_safe_init(
                &self.project,
                self.manifest_path(),
                source_id,
                document,
            )?;
        }
        Ok(())
    }
}
