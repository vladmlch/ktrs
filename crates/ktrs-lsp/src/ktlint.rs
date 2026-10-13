//! ktlint engines per configuration, and the engine calls the server makes: lint, fix all (`ktlint --format`),
//! fix one error, insert a suppression.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use ktrs_lint::editorconfig::{
    CODE_STYLE_PROPERTY, EXPERIMENTAL_RULES_EXECUTION_PROPERTY, KtlintVersion as EngineVersion, PropertyRef, with_ktlint_version,
};
use ktrs_lint::engine::ktlint_rule_engine::EngineWarnings;
use ktrs_lint::engine::{EditorConfigPropertyRegistry, KtlintSuppression};
use ktrs_lint::rules::standard_rule_providers;
use ktrs_lint::{AutocorrectDecision, Code, EditorConfigDefaults, EditorConfigOverride, KtLintException, KtLintRuleEngine, LintError};
use ktrs_project::{KtlintConfig, KtlintVersion};

/// Why the engine produced no result.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Not valid Kotlin: no rule ran.
    Parse,
    Other(String),
}

pub(crate) struct KtlintEngines {
    engines: Vec<(KtlintConfig, KtLintRuleEngine)>,
    engine_warnings: EngineWarnings,
    warned: HashSet<String>,
}

impl KtlintEngines {
    pub(crate) fn new(engine_warnings: EngineWarnings) -> KtlintEngines {
        KtlintEngines { engines: Vec::new(), engine_warnings, warned: HashSet::new() }
    }

    /// The engine for `config`, built on first use. Building reports what it leaves out (rule set JARs ktrs
    /// can't run, unknown `.editorconfig` properties) to `warn`, once per message over the server's life.
    pub(crate) fn get_or_build(&mut self, config: &KtlintConfig, warn: &mut dyn FnMut(String)) -> &KtLintRuleEngine {
        let index = match self.engines.iter().position(|(c, _)| c == config) {
            Some(index) => index,
            None => {
                let (engine, warnings) = build(config, self.engine_warnings.clone());
                for warning in warnings {
                    if self.warned.insert(warning.clone()) {
                        warn(warning);
                    }
                }
                self.engines.push((config.clone(), engine));
                self.engines.len() - 1
            }
        };
        &self.engines[index].1
    }

    /// Re-reads a changed `.editorconfig` in ktlint's (process-wide) cache, or drops the cache when `path` was
    /// created or deleted (the cache may hold its absence). Every engine keeps its own memoized directory
    /// chains, so all engines are dropped, not just the first.
    pub(crate) fn editor_config_changed(&self, path: &Path, created_or_deleted: bool) -> Result<(), String> {
        for (_, engine) in &self.engines {
            if created_or_deleted {
                engine.trim_memory();
            } else {
                engine.reload_editor_config_file(path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

fn build(config: &KtlintConfig, engine_warnings: EngineWarnings) -> (KtLintRuleEngine, Vec<String>) {
    let mut warnings = Vec::new();
    let mut providers = standard_rule_providers();
    let mut compose = false;
    for jar in &config.rule_sets {
        if ktrs_compose::jar::native_compose_rules_release(Path::new(jar)).is_some() {
            compose = true;
        } else {
            warnings.push(format!(
                "ktrs runs no custom rule set but compose-rules {}: linting without '{jar}'",
                ktrs_compose::COMPOSE_RULES_VERSION
            ));
        }
    }
    if compose {
        providers.extend(ktrs_compose::compose_rule_providers());
    }
    let mut overrides: Vec<(PropertyRef, Option<String>)> = Vec::new();
    if config.android == Some(true) {
        overrides.push(((&*CODE_STYLE_PROPERTY).into(), Some("android_studio".to_owned())));
    }
    if let Some(experimental) = config.experimental {
        let value = if experimental { "enabled" } else { "disabled" };
        overrides.push(((&*EXPERIMENTAL_RULES_EXECUTION_PROPERTY).into(), Some(value.to_owned())));
    }
    let registry = EditorConfigPropertyRegistry::new(&providers);
    for (name, value) in &config.editorconfig_overrides {
        match registry.find(name) {
            Ok(property) => overrides.push((property, Some(value.clone()))),
            Err(_) => warnings.push(format!("ignoring the .editorconfig override '{name}': no ktlint rule uses it")),
        }
    }
    let editor_config_override = if overrides.is_empty() { EditorConfigOverride::empty() } else { EditorConfigOverride::from(overrides) };
    let version = match config.version {
        KtlintVersion::V1_8 => EngineVersion::V1_8,
        KtlintVersion::V2_0 => EngineVersion::V2_0,
    };
    let editor_config_override = with_ktlint_version(editor_config_override, version);
    let engine = KtLintRuleEngine::with_editor_config(providers, EditorConfigDefaults::empty(), editor_config_override)
        .with_engine_warnings(engine_warnings);
    (engine, warnings)
}

/// The engine's input: a file (its `.editorconfig` files and name apply) or, without a path, a snippet.
pub(crate) fn code(path: Option<&Path>, text: &str) -> Code {
    match path {
        Some(path) => Code::from_file_content(path, text.to_owned()),
        None => Code::from_snippet(text, false),
    }
}

/// The lint errors, sorted by position.
pub(crate) fn lint(engine: &KtLintRuleEngine, code: &Code) -> Result<Vec<LintError>, Failure> {
    let mut errors = Vec::new();
    guarded(code, || engine.lint(code, &mut |e| errors.push(e.clone())))?;
    Ok(errors)
}

/// `ktlint --format`: every autocorrectable error fixed.
pub(crate) fn fix_all(engine: &KtLintRuleEngine, code: &Code) -> Result<String, Failure> {
    guarded(code, || {
        engine.format(code, &mut |e| if e.can_be_auto_corrected { AutocorrectDecision::AllowAutocorrect } else { AutocorrectDecision::NoAutocorrect })
    })
}

/// Only `error` fixed, in a single pass (as the IntelliJ ktlint plugin's single-violation autocorrect).
pub(crate) fn fix_one(engine: &KtLintRuleEngine, code: &Code, error: &LintError) -> Result<String, Failure> {
    guarded(code, || {
        engine.format_with(code, false, &mut |e| {
            if e.line == error.line && e.col == error.col && e.rule_id == error.rule_id {
                AutocorrectDecision::AllowAutocorrect
            } else {
                AutocorrectDecision::NoAutocorrect
            }
        })
    })
}

/// The code with a ktlint suppression for `error`'s rule at its position, or for the whole file.
pub(crate) fn suppress(engine: &KtLintRuleEngine, code: &Code, error: &LintError, whole_file: bool) -> Result<String, Failure> {
    let suppression = if whole_file {
        KtlintSuppression::ForFile { rule_id: error.rule_id }
    } else {
        KtlintSuppression::AtOffset { line: error.line, col: error.col, rule_id: error.rule_id }
    };
    match catch_unwind(AssertUnwindSafe(|| engine.insert_suppression(code, &suppression))) {
        Ok(Ok(code)) => Ok(code),
        Ok(Err(e)) => Err(Failure::Other(format!("cannot suppress {}: {e:?}", error.rule_id.value()))),
        Err(_) => Err(internal_error(code)),
    }
}

fn guarded<T>(code: &Code, run: impl FnOnce() -> Result<T, KtLintException>) -> Result<T, Failure> {
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(KtLintException::Parse(_))) => Err(Failure::Parse),
        Ok(Err(KtLintException::Rule(e))) => Err(Failure::Other(format!("{}: {} (caused by: {})", code.file_path_or_stdin(), e.message, e.cause))),
        Ok(Err(e)) => Err(Failure::Other(format!("{}: {e}", code.file_path_or_stdin()))),
        Err(_) => Err(internal_error(code)),
    }
}

fn internal_error(code: &Code) -> Failure {
    Failure::Other(format!("internal error in ktrs (please report it with this file): {}", code.file_path_or_stdin()))
}
