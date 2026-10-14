//! Port of ktlint-rule-engine-core `Rule.kt` (`RuleId`, `RuleSetId`, `RuleV2`, `About`, the marker
//! interfaces), `IgnoreKtlintSuppressions.kt`, `internal/IdNamingPolicy.kt` and `AutocorrectDecision.kt`.

use ktrs_ast::{Ast, NodeId};
pub use ktrs_parser::token_set::TokenSet;

pub use crate::editorconfig::EditorConfig;
use crate::editorconfig::PropertyRef;
// TODO: drop once indent_config.rs (1B) uses `IndentStyleValue` like upstream `IndentConfig`.
pub use crate::editorconfig::IndentStyleValue as IndentStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(pub &'static str);

impl RuleId {
    /// `RuleId(value)`, which enforces the naming policy.
    pub fn new(value: &'static str) -> RuleId {
        enforce_rule_id_naming(value);
        RuleId(value)
    }

    pub fn value(self) -> &'static str {
        self.0
    }

    /// `ruleSetId`: the part before `:` ("" without one).
    pub fn rule_set_id(self) -> RuleSetId {
        RuleSetId(self.0.split_once(':').map_or("", |(set, _)| set))
    }

    pub fn is_valid(value: &str) -> bool {
        is_valid_rule_id(value)
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleSetId(pub &'static str);

impl RuleSetId {
    /// Reserved for the rules of the ktlint project.
    pub const STANDARD: RuleSetId = RuleSetId("standard");

    pub fn value(self) -> &'static str {
        self.0
    }

    pub fn is_valid(value: &str) -> bool {
        is_valid_rule_set_id(value)
    }
}

/// `SIMPLE_ID_REGEX` = `[a-z]+(-[a-z]+)*`.
fn is_simple_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase()))
}

fn enforce_rule_id_naming(rule_id: &str) {
    assert!(
        is_valid_rule_id(rule_id),
        "IllegalArgumentException: Rule with id '{rule_id}' must match regexp '[a-z]+(-[a-z]+)*:[a-z]+(-[a-z]+)*'"
    );
}

fn is_valid_rule_id(rule_id: &str) -> bool {
    rule_id
        .split_once(':')
        .is_some_and(|(set, id)| is_simple_id(set) && is_simple_id(id))
}

fn is_valid_rule_set_id(rule_set_id: &str) -> bool {
    is_simple_id(rule_set_id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutocorrectDecision {
    AllowAutocorrect,
    NoAutocorrect,
}

impl AutocorrectDecision {
    /// `ifAutocorrectAllowed { }`.
    pub fn if_autocorrect_allowed<T>(self, function: impl FnOnce() -> T) -> Option<T> {
        (self == AutocorrectDecision::AllowAutocorrect).then(function)
    }
}

/// `RuleV2.About`: shown when the rule throws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct About {
    pub maintainer: &'static str,
    pub repository_url: &'static str,
    pub issue_tracker_url: &'static str,
}

impl Default for About {
    fn default() -> About {
        About {
            maintainer: "Not specified (and not maintained by the Ktlint project)",
            repository_url: "Not specified",
            issue_tracker_url: "Not specified",
        }
    }
}

/// The `emit(offset, errorMessage, canBeAutoCorrected)` lambda. It also takes the tree, because the
/// offset (UTF-8, in the tree the visited node was in when its visit began: a node an earlier rule
/// replaced is visited in its detached tree, as in ktlint) is converted to UTF-16 at emit time.
pub type Emit<'a> = dyn FnMut(&Ast, usize, &str, bool) -> AutocorrectDecision + 'a;

/// What `stopTraversalOfAST()` sets. A rule that stops keeps one and returns it from
/// [`RuleV2::traversal_state`]; the NOT_STARTED/CONTINUE half lives in the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TraversalState {
    stopped: bool,
}

impl TraversalState {
    /// `stopTraversalOfAST()`. In `before_first_node` no node is visited; in a visit hook the children
    /// are skipped, `after_visit_child_nodes` still runs for the node and its parents; `after_last_node`
    /// always runs.
    pub fn stop_traversal_of_ast(&mut self) {
        self.stopped = true;
    }

    pub fn is_stopped(self) -> bool {
        self.stopped
    }
}

/// ktlint 1.8 `Rule.VisitorModifier` (removed in 2.0, #3252): only the 1.8 rule order reads it
/// (`engine/rule_provider_sorter_1_8.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisitorModifier {
    RunAfterRule(RuleId, RunAfterRuleMode),
    RunAsLateAsPossible,
}

impl VisitorModifier {
    /// `RunAfterRule(ruleId, REGARDLESS_WHETHER_RUN_AFTER_RULE_IS_LOADED_OR_DISABLED)`.
    pub const fn run_after(rule_id: &'static str) -> VisitorModifier {
        VisitorModifier::RunAfterRule(RuleId(rule_id), RunAfterRuleMode::RegardlessWhetherRunAfterRuleIsLoadedOrDisabled)
    }

    /// `RunAfterRule(ruleId, ONLY_WHEN_RUN_AFTER_RULE_IS_LOADED_AND_ENABLED)`.
    pub const fn run_after_loaded_and_enabled(rule_id: &'static str) -> VisitorModifier {
        VisitorModifier::RunAfterRule(RuleId(rule_id), RunAfterRuleMode::OnlyWhenRunAfterRuleIsLoadedAndEnabled)
    }
}

/// `Rule.VisitorModifier.RunAfterRule.Mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAfterRuleMode {
    /// The rule is not run at all (1.8 throws) when the other rule is not loaded or disabled.
    OnlyWhenRunAfterRuleIsLoadedAndEnabled,
    RegardlessWhetherRunAfterRuleIsLoadedOrDisabled,
}

/// `RuleV2`. A new instance runs every pass (`RuleV2Provider.createNewRuleInstance`), so it may keep state.
/// The Kotlin marker interfaces are the `is_*` methods.
pub trait RuleV2 {
    fn rule_id(&self) -> RuleId;

    fn about(&self) -> About {
        About::default()
    }

    /// The `.editorconfig` properties the rule reads; only these (and the code style) are in the
    /// [`EditorConfig`] it gets.
    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        Vec::new()
    }

    /// `RuleV2.Experimental`: runs only when experimental rules (or the rule itself) are enabled.
    fn is_experimental(&self) -> bool {
        false
    }

    /// `RuleV2.OfficialCodeStyle`: runs only with `ktlint_code_style = ktlint_official` (or when enabled).
    fn is_official_code_style(&self) -> bool {
        false
    }

    /// `RuleV2.OnlyWhenEnabledInEditorconfig`.
    fn is_only_when_enabled_in_editorconfig(&self) -> bool {
        false
    }

    /// The rule's `visitorModifiers` in ktlint 1.8.0.
    fn visitor_modifiers(&self) -> &'static [VisitorModifier] {
        &[]
    }

    /// `IgnoreKtlintSuppressions`: not subject to `@Suppress` or formatter tags.
    fn ignores_ktlint_suppressions(&self) -> bool {
        false
    }

    /// `None` for a rule that never stops; one that can returns `Some` from the start (asked once).
    fn traversal_state(&self) -> Option<TraversalState> {
        None
    }

    fn before_first_node(&mut self, _editor_config: &EditorConfig) {}

    /// The element types [`Self::before_visit_child_nodes`] acts on (`None`: every node). For a node of
    /// any other type the hook must do nothing at all (no emit, edit, state change or stop), so the
    /// engine skips the call. Asked once per traversal.
    fn visited_types(&self) -> Option<TokenSet> {
        None
    }

    fn before_visit_child_nodes(&mut self, _ast: &mut Ast, _node: NodeId, _emit: &mut Emit<'_>) {}

    fn after_visit_child_nodes(&mut self, _ast: &mut Ast, _node: NodeId, _emit: &mut Emit<'_>) {}

    /// Whether [`Self::after_visit_child_nodes`] is overridden; the engine skips the no-op hook of the
    /// others (half of all visits). `tests/rule_hooks.rs` keeps the two in sync.
    fn visits_after_child_nodes(&self) -> bool {
        false
    }

    /// The element types [`Self::after_visit_child_nodes`] acts on (`None`: every node). For a node of
    /// any other type the hook must do nothing at all (no emit, edit, state change or stop), so the
    /// engine skips the call. Asked once per traversal.
    fn visited_after_types(&self) -> Option<TokenSet> {
        None
    }

    fn after_last_node(&mut self) {}
}
