//! Port of ktlint-rule-engine `api/KtLintRuleEngine.kt`.

use std::path::Path;
use std::sync::Arc;

use ktrs_ast::Ast;

use crate::editorconfig::EditorConfig;
use crate::engine::rule_setup::{RuleSetup, RuleSetupCache};
use crate::engine::code::{Code, KtLintException, LintError};
use crate::engine::code_formatter::{AutocorrectHandler, format};
use crate::engine::editor_config_cache::THREAD_SAFE_EDITOR_CONFIG_CACHE;
use crate::engine::editor_config_defaults::{EditorConfigDefaults, EditorConfigOverride};
use crate::engine::editor_config_loader::{EditorConfigLoader, EditorConfigLoaderEc4j};
use crate::engine::rule_execution_context::create_rule_execution_context;
use crate::rule::AutocorrectDecision;
use crate::rule_provider::{RuleV2Provider, property_types};

pub const MAX_FORMAT_RUNS_PER_FILE: usize = 3;
pub const UTF8_BOM: char = '\u{FEFF}';

/// Receives the engine's own warnings as (logger name, message): upstream logs them with SLF4J.
pub type EngineWarnings = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct KtLintRuleEngine {
    /// The rules to run; each provider creates a fresh rule per traversal.
    pub(crate) rule_providers: Vec<RuleV2Provider>,
    /// Values for properties that no `.editorconfig` on the file's path sets (above ktlint's defaults).
    editor_config_defaults: EditorConfigDefaults,
    /// Values that win over every `.editorconfig`.
    editor_config_override: EditorConfigOverride,
    editor_config_loader: EditorConfigLoader,
    rule_setup_cache: RuleSetupCache,
    engine_warnings: Option<EngineWarnings>,
}

impl KtLintRuleEngine {
    pub fn new(rule_providers: Vec<RuleV2Provider>) -> KtLintRuleEngine {
        KtLintRuleEngine::with_editor_config(
            rule_providers,
            EditorConfigDefaults::empty(),
            EditorConfigOverride::empty(),
        )
    }

    pub fn with_editor_config(
        rule_providers: Vec<RuleV2Provider>,
        editor_config_defaults: EditorConfigDefaults,
        editor_config_override: EditorConfigOverride,
    ) -> KtLintRuleEngine {
        assert!(
            !rule_providers.is_empty(),
            "IllegalArgumentException: A non-empty set of 'ruleProviders' need to be provided"
        );
        let editor_config_loader = EditorConfigLoader::new(
            EditorConfigLoaderEc4j::new(&property_types(&rule_providers)),
            editor_config_defaults.clone(),
            editor_config_override.clone(),
        );
        KtLintRuleEngine {
            rule_providers,
            editor_config_defaults,
            editor_config_override,
            editor_config_loader,
            rule_setup_cache: RuleSetupCache::default(),
            engine_warnings: None,
        }
    }

    /// Routes the engine's warnings (logger name, message) to `engine_warnings`.
    pub fn with_engine_warnings(self, engine_warnings: EngineWarnings) -> KtLintRuleEngine {
        KtLintRuleEngine { engine_warnings: Some(engine_warnings), ..self }
    }

    pub(crate) fn warn(&self, logger: &str, message: impl FnOnce() -> String) {
        if let Some(engine_warnings) = &self.engine_warnings {
            engine_warnings(logger, &message());
        }
    }

    pub(crate) fn editor_config_loader(&self) -> &EditorConfigLoader {
        &self.editor_config_loader
    }

    /// The enabled rules and their `.editorconfig` views for a file's loaded config.
    pub(crate) fn rule_setup(&self, editor_config: EditorConfig) -> Arc<RuleSetup> {
        self.rule_setup_cache.get(editor_config, &self.rule_providers)
    }

    pub fn rule_providers(&self) -> &[RuleV2Provider] {
        &self.rule_providers
    }

    pub fn editor_config_defaults(&self) -> &EditorConfigDefaults {
        &self.editor_config_defaults
    }

    pub fn editor_config_override(&self) -> &EditorConfigOverride {
        &self.editor_config_override
    }

    /// `lint(code, callback)`: one pass, no autocorrect; errors sorted by (line, col), duplicates dropped.
    pub fn lint(
        &self,
        code: &Code,
        callback: &mut dyn FnMut(&LintError),
    ) -> Result<(), KtLintException> {
        format(
            self,
            code,
            AutocorrectHandler::None,
            &mut |e, _| callback(e),
            1,
            &mut |_| {},
        )
        .map(drop)
    }

    /// `format(code, callback)`: up to [`MAX_FORMAT_RUNS_PER_FILE`] passes; `callback` is asked for every
    /// error in emit order (duplicates across passes included) and decides whether it is autocorrected.
    pub fn format(
        &self,
        code: &Code,
        callback: &mut dyn FnMut(&LintError) -> AutocorrectDecision,
    ) -> Result<String, KtLintException> {
        self.format_with(code, true, callback)
    }

    /// `codeFormatter.format(code, autocorrectHandler, callback)`: `autocorrect` decides per error as in
    /// [`Self::format`]; once done, `callback` gets the distinct errors sorted by (line, col), with whether each
    /// was corrected. With `|_| AllowAutocorrect` it is 1.x's deprecated `format(code, callback: (LintError,
    /// Boolean) -> Unit)` (`AllAutocorrectHandler`), which Spotless calls.
    pub fn format_reporting(
        &self,
        code: &Code,
        autocorrect: &mut dyn FnMut(&LintError) -> AutocorrectDecision,
        callback: &mut dyn FnMut(&LintError, bool),
    ) -> Result<String, KtLintException> {
        let handler = AutocorrectHandler::LintErrorAutocorrectHandler(autocorrect);
        format(self, code, handler, callback, MAX_FORMAT_RUNS_PER_FILE, &mut |_| {})
            .map(|formatted| formatted.into_owned())
    }

    /// `format(code, rerunAfterAutocorrect, callback)`: without rerun, a single pass (for consumers that
    /// let a user decide per error).
    pub fn format_with(
        &self,
        code: &Code,
        rerun_after_autocorrect: bool,
        callback: &mut dyn FnMut(&LintError) -> AutocorrectDecision,
    ) -> Result<String, KtLintException> {
        let max_format_runs_per_file = if rerun_after_autocorrect {
            MAX_FORMAT_RUNS_PER_FILE
        } else {
            1
        };
        let handler = AutocorrectHandler::LintErrorAutocorrectHandler(callback);
        format(
            self,
            code,
            handler,
            &mut |_, _| {},
            max_format_runs_per_file,
            &mut |_| {},
        )
        .map(|formatted| formatted.into_owned())
    }

    /// [`Self::format`], calling `after_pass` with the tree at the end of every pass (what a last-sorted
    /// probe rule sees in `afterLastNode`, lint-after-format included).
    pub fn format_observed(
        &self,
        code: &Code,
        callback: &mut dyn FnMut(&LintError) -> AutocorrectDecision,
        after_pass: &mut dyn FnMut(&Ast),
    ) -> Result<String, KtLintException> {
        let handler = AutocorrectHandler::LintErrorAutocorrectHandler(callback);
        format(
            self,
            code,
            handler,
            &mut |_, _| {},
            MAX_FORMAT_RUNS_PER_FILE,
            after_pass,
        )
        .map(|formatted| formatted.into_owned())
    }

    /// `transformToAst(code)`: the parsed tree (after the parse-error check and `.editorconfig` load).
    pub fn transform_to_ast(&self, code: &Code) -> Result<Ast, KtLintException> {
        create_rule_execution_context(self, code).map(|context| context.ast)
    }

    /// `trimMemory()`: drops the cached `.editorconfig` files.
    pub fn trim_memory(&self) {
        THREAD_SAFE_EDITOR_CONFIG_CACHE.clear();
        self.editor_config_loader.clear_dir_chains();
    }

    /// `reloadEditorConfigFile(path)`: re-reads a cached `.editorconfig` file.
    pub fn reload_editor_config_file(
        &self,
        path: &Path,
    ) -> Result<(), ktrs_editorconfig::ParseException> {
        self.editor_config_loader.clear_dir_chains();
        THREAD_SAFE_EDITOR_CONFIG_CACHE.reload_if_exists(path)
    }

    /// Drops the memoized directory chains for this engine.
    pub fn clear_dir_chains(&self) {
        self.editor_config_loader.clear_dir_chains();
    }

    pub fn dir_chains_len(&self) -> usize {
        self.editor_config_loader.dir_chains_len()
    }
}
