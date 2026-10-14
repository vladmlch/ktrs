//! The per-`.editorconfig` part of a file's setup, shared by every file whose config is equal: the enabled
//! rule providers (sorted for execution) and each rule's view of the config. Both are pure functions of the
//! config and the engine's providers, and rebuilding them per file and per pass was most of the fixed cost.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::editorconfig::{EditorConfig, KtlintVersion};
use crate::engine::internal_rules::KTLINT_SUPPRESSION_RULE_ID;
use crate::engine::rule_dispatch::RuleDispatch;
use crate::engine::rule_execution_context::rule_editor_config;
use crate::engine::rule_filter::{InternalRuleProvidersFilter, RuleExecutionRuleFilter, apply_rule_filters};
use crate::engine::rule_provider_sorter_1_8::run_after_rule_filter;
use crate::engine::visitor_provider::VisitorProvider;
use crate::rule::{RuleId, RuleV2};
use crate::rule_provider::{RuleV2Provider, rule_providers_in};

/// Distinct configs kept; a run usually has one per `.editorconfig` directory.
const CAPACITY: usize = 16;

pub(crate) struct RuleSetup {
    pub(crate) editor_config: EditorConfig,
    pub(crate) ktlint_version: KtlintVersion,
    pub(crate) rule_providers: Vec<RuleV2Provider>,
    pub(crate) visitor_provider: VisitorProvider,
    pub(crate) dispatch: Arc<RuleDispatch>,
    pub(crate) suppression_dispatch: Arc<RuleDispatch>,
    /// 1.8's `RunAfterRuleFilter` failed: the `IllegalStateException` message every file then throws.
    pub(crate) rule_filter_error: Option<String>,
    /// `RuleExecutionRuleFilter`'s WARNs, which upstream logs for every file.
    pub(crate) rule_filter_warnings: Vec<String>,
    rule_editor_configs: Mutex<HashMap<RuleId, Arc<EditorConfig>>>,
}

impl RuleSetup {
    /// [`rule_editor_config`], built on first request so a panic in it still surfaces inside the rule's
    /// `execute`, as upstream.
    pub(crate) fn rule_editor_config(&self, rule: &dyn RuleV2) -> Arc<EditorConfig> {
        if let Some(cached) = self.rule_editor_configs.lock().unwrap().get(&rule.rule_id()) {
            return cached.clone();
        }
        let built = Arc::new(rule_editor_config(&self.editor_config, &self.rule_providers, rule));
        self.rule_editor_configs.lock().unwrap().insert(rule.rule_id(), built.clone());
        built
    }
}

#[derive(Default)]
pub(crate) struct RuleSetupCache {
    entries: Mutex<Vec<Arc<RuleSetup>>>,
}

impl RuleSetupCache {
    pub(crate) fn get(&self, editor_config: EditorConfig, engine_rule_providers: &[RuleV2Provider]) -> Arc<RuleSetup> {
        if let Some(hit) = self.entries.lock().unwrap().iter().find(|s| s.editor_config == editor_config) {
            return hit.clone();
        }
        let ktlint_version = KtlintVersion::of(&editor_config);
        // The version's rule set stands in for the engine's providers, so suppressions of rules it lacks
        // are "unknown or not loaded" as in that release.
        let engine_rule_providers = &rule_providers_in(engine_rule_providers, ktlint_version);
        let rule_execution_rule_filter = RuleExecutionRuleFilter::new(&editor_config);
        let mut rule_providers = apply_rule_filters(
            engine_rule_providers,
            &[&InternalRuleProvidersFilter::new(engine_rule_providers), &rule_execution_rule_filter],
        );
        let rule_filter_warnings = rule_execution_rule_filter.into_warnings();
        let mut rule_filter_error = None;
        if ktlint_version.is_1_8() {
            match run_after_rule_filter(rule_providers) {
                Ok(filtered) => rule_providers = filtered,
                Err(message) => {
                    rule_filter_error = Some(message);
                    rule_providers = Vec::new();
                }
            }
        }
        let visitor_provider = VisitorProvider::for_version(&rule_providers, ktlint_version);
        let rules = visitor_provider.rules();
        let (suppression, others): (Vec<_>, Vec<_>) = rules
            .into_iter()
            .partition(|r| r.rule_id() == KTLINT_SUPPRESSION_RULE_ID);
        let dispatch = Arc::new(RuleDispatch::build(
            others.iter().map(|r| (r.visited_types(), r.visits_after_child_nodes())),
        ));
        let suppression_dispatch = Arc::new(RuleDispatch::build(
            suppression.iter().map(|r| (r.visited_types(), r.visits_after_child_nodes())),
        ));
        let setup = Arc::new(RuleSetup {
            visitor_provider,
            editor_config,
            ktlint_version,
            rule_providers,
            dispatch,
            suppression_dispatch,
            rule_filter_error,
            rule_filter_warnings,
            rule_editor_configs: Mutex::default(),
        });
        let mut entries = self.entries.lock().unwrap();
        if entries.len() == CAPACITY {
            entries.remove(0);
        }
        entries.push(setup.clone());
        setup
    }
}
