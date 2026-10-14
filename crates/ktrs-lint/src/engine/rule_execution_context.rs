//! Port of ktlint-rule-engine `internal/RuleExecutionContext.kt`: the parsed file, its `.editorconfig`
//! and enabled rules, and the 2.0 traversal (every rule at a node, then the children, then every rule's
//! `after` at that node), preceded by a traversal of the internal suppression rule alone; in 1.8 mode
//! 1.8's rule after rule (`rule_major.rs`).

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::sync::atomic::{AtomicBool, Ordering};

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::FILE;

use crate::editorconfig::{
    CODE_STYLE_PROPERTY, EditorConfig, EditorConfigProperty, PropertyRef, RuleExecution,
    create_rule_execution_editor_config_property,
};
use crate::engine::code::KtLintRuleException;
use crate::engine::internal_rules::KTLINT_SUPPRESSION_RULE_ID;
use crate::engine::position_in_text_locator::PositionInTextLocator;
use crate::engine::rule_dispatch::RuleDispatch;
use crate::engine::rule_panic::catch_rule_panic;
use crate::engine::rule_setup::RuleSetup;
use crate::engine::suppression_locator::SuppressionLocator;
use crate::rule::{About, AutocorrectDecision, RuleId, RuleV2, TokenSet};
use crate::rule_provider::RuleV2Provider;

mod create;
mod rule_major;

pub(crate) use create::create_rule_execution_context;

static VERIFY_VISITED_TYPES: AtomicBool = AtomicBool::new(false);

/// Test hook: also run the `before_visit_child_nodes` calls that `visited_types` lets the engine skip,
/// panicking if one emits or edits (`GOLDEN_VERIFY_VISITED_TYPES=1` in `tests/golden`), and check the other
/// exact shortcuts against the full computation ([`verifying_shortcuts`]).
#[doc(hidden)]
pub fn set_verify_visited_types(enabled: bool) {
    VERIFY_VISITED_TYPES.store(enabled, Ordering::Relaxed);
}

/// Whether a shortcut must also compute the full answer and assert they agree (see [`set_verify_visited_types`]).
pub(crate) fn verifying_shortcuts() -> bool {
    VERIFY_VISITED_TYPES.load(Ordering::Relaxed)
}

/// `emitAndApprove(offset, ruleId, errorMessage, canBeAutoCorrected)`; `offset` in UTF-16 units.
pub type EmitAndApprove<'a> = dyn FnMut(usize, RuleId, &str, bool) -> AutocorrectDecision + 'a;

pub(crate) struct RuleExecutionContext<'a> {
    file_path_or_stdin: String,
    pub(crate) ast: Ast,
    /// The `.editorconfig` and the rules it enables.
    pub(crate) setup: Arc<RuleSetup>,
    /// Built from the original text and kept for every pass (so later positions are stale, as upstream).
    pub(crate) position_in_text_locator: Rc<PositionInTextLocator<'a>>,
    suppression_locator: SuppressionLocator,
}

/// A rule of one traversal. `stopped_at` is the node sequence number current when the rule stopped:
/// it still sees the `after` hooks of nodes entered up to then, and no node entered later.
struct RuleInstance {
    rule: Box<dyn RuleV2>,
    stopped_at: Option<u64>,
    /// The rule's constant answers, asked once instead of at every visit.
    rule_id: RuleId,
    ignores_suppressions: bool,
    can_stop: bool,
    visited_types: Option<TokenSet>,
    visited_after_types: Option<TokenSet>,
}

impl RuleInstance {
    fn new(rule: Box<dyn RuleV2>) -> RuleInstance {
        RuleInstance {
            rule_id: rule.rule_id(),
            ignores_suppressions: rule.ignores_ktlint_suppressions(),
            can_stop: rule.traversal_state().is_some(),
            visited_types: rule.visited_types(),
            visited_after_types: rule.visited_after_types(),
            rule,
            stopped_at: None,
        }
    }

    #[inline(always)]
    fn visits_after_kind(&self, kind: ktrs_syntax::SyntaxKind) -> bool {
        self.visited_after_types
            .is_none_or(|types| types.contains(kind))
    }

    fn visits(&self, entry_seq: u64) -> bool {
        self.stopped_at.is_none_or(|s| s >= entry_seq)
    }

    fn note_stop(&mut self, seq: u64) {
        if self.can_stop
            && self.stopped_at.is_none()
            && self.rule.traversal_state().is_some_and(|t| t.is_stopped())
        {
            self.stopped_at = Some(seq);
        }
    }
}

struct RuleExecutionException {
    rule_id: RuleId,
    about: About,
    line: usize,
    col: usize,
    cause: String,
}

static MAX_LINE_LENGTH_RULE_ENABLED: LazyLock<EditorConfigProperty<RuleExecution>> =
    LazyLock::new(|| {
        create_rule_execution_editor_config_property(
            "standard:max-line-length",
            RuleExecution::Enabled,
        )
    });
static MAX_LINE_LENGTH_RULE_DISABLED: LazyLock<EditorConfigProperty<RuleExecution>> =
    LazyLock::new(|| {
        create_rule_execution_editor_config_property(
            "standard:max-line-length",
            RuleExecution::Disabled,
        )
    });

impl<'a> RuleExecutionContext<'a> {
    /// The suppression rule first, over the whole tree: its `ktlint-disable` migration climbs the tree to
    /// add annotations. Then all other rules together.
    pub(crate) fn execute_rules(
        &mut self,
        rules: Vec<Box<dyn RuleV2>>,
        lint_mode: bool,
        emit_and_approve: &mut EmitAndApprove<'_>,
    ) -> Result<(), KtLintRuleException> {
        if self.setup.ktlint_version.is_1_8() {
            for rule in rules {
                self.execute_rule_1_8(rule, lint_mode, emit_and_approve)?;
            }
            return Ok(());
        }
        let (suppression, others): (Vec<_>, Vec<_>) = rules
            .into_iter()
            .partition(|r| r.rule_id() == KTLINT_SUPPRESSION_RULE_ID);
        if !suppression.is_empty() && self.ast.allocated_leaf_text_contains("ktlint") {
            self.execute_rules_on_ast(suppression, true, lint_mode, emit_and_approve)?;
        }
        self.execute_rules_on_ast(others, false, lint_mode, emit_and_approve)
    }

    fn execute_rules_on_ast(
        &mut self,
        rules: Vec<Box<dyn RuleV2>>,
        is_suppression: bool,
        lint_mode: bool,
        emit_and_approve: &mut EmitAndApprove<'_>,
    ) -> Result<(), KtLintRuleException> {
        let setup = self.setup.clone();
        let dispatch = if is_suppression {
            &setup.suppression_dispatch
        } else {
            &setup.dispatch
        };
        let parts = TraversalParts {
            ast: &mut self.ast,
            suppression_locator: &mut self.suppression_locator,
            position_in_text_locator: &self.position_in_text_locator,
            lint_mode,
        };
        let rule_editor_config = |rule: &dyn RuleV2| setup.rule_editor_config(rule);
        traverse(parts, rules, dispatch, &rule_editor_config, emit_and_approve).map_err(|e| self.to_ktlint_rule_exception(e))
    }

    fn to_ktlint_rule_exception(&self, e: RuleExecutionException) -> KtLintRuleException {
        KtLintRuleException {
            line: e.line,
            col: e.col,
            rule_id: e.rule_id.value().to_owned(),
            message: format!(
                "Rule '{}' throws exception in file '{}' at position ({}:{})\n   Rule maintainer: {}\n   Issue tracker  : {}\n   Repository     : {}",
                e.rule_id.value(),
                self.file_path_or_stdin,
                e.line,
                e.col,
                e.about.maintainer,
                e.about.issue_tracker_url,
                e.about.repository_url
            ),
            cause: e.cause,
        }
    }
}

/// The rule's view of the `.editorconfig`: its declared properties, the code style (needed for the
/// defaults) and whether `standard:max-line-length` runs (whether `max_line_length` applies at all).
pub(crate) fn rule_editor_config(
    editor_config: &EditorConfig,
    rule_providers: &[RuleV2Provider],
    rule: &dyn RuleV2,
) -> EditorConfig {
    let max_line_length_rule_loaded = rule_providers
        .iter()
        .any(|p| p.rule_id().value() == "standard:max-line-length");
    let mut properties = rule.uses_editor_config_properties();
    properties.push(PropertyRef::from(&*CODE_STYLE_PROPERTY));
    properties.push(PropertyRef::from(if max_line_length_rule_loaded {
        &*MAX_LINE_LENGTH_RULE_ENABLED
    } else {
        &*MAX_LINE_LENGTH_RULE_DISABLED
    }));
    editor_config.filter_by(&properties)
}

struct TraversalParts<'a, 't> {
    ast: &'a mut Ast,
    suppression_locator: &'a mut SuppressionLocator,
    position_in_text_locator: &'a PositionInTextLocator<'t>,
    lint_mode: bool,
}

/// One traversal: `beforeFirstNode` for all rules, the node walk, `afterLastNode` for all rules.
fn traverse<'a, 't, C: Deref<Target = EditorConfig>>(
    parts: TraversalParts<'a, 't>,
    rules: Vec<Box<dyn RuleV2>>,
    dispatch: &'a RuleDispatch,
    rule_editor_config: &dyn Fn(&dyn RuleV2) -> C,
    emit_and_approve: &mut EmitAndApprove<'_>,
) -> Result<(), RuleExecutionException> {
    let mut rules: Vec<RuleInstance> = rules
        .into_iter()
        .map(RuleInstance::new)
        .collect();
    for r in &mut rules {
        execute(&mut *r.rule, |rule| {
            let editor_config = rule_editor_config(&*rule);
            rule.before_first_node(&editor_config)
        })?;
        r.note_stop(0);
    }
    parts.suppression_locator.refresh_may_have_hints(parts.ast);
    let root = parts.ast.root();
    let mut traversal = Traversal {
        ast: parts.ast,
        suppression_locator: parts.suppression_locator,
        position_in_text_locator: parts.position_in_text_locator,
        lint_mode: parts.lint_mode,
        emit_and_approve,
        children: Vec::new(),
        seq: 0,
        dispatch,
        current_rule: Cell::new(NO_RULE),
        current_node: Cell::new(root),
    };
    let outcome = catch_rule_panic(std::panic::AssertUnwindSafe(|| {
        traversal.execute_rules_on_node_recursively(root, &mut rules);
    }));
    match outcome {
        Ok(()) => {}
        Err(payload) => {
            let index = traversal.current_rule.get();
            if index == NO_RULE {
                std::panic::resume_unwind(payload);
            }
            let node = traversal.current_node.get();
            let r = &mut rules[index];
            r.note_stop(traversal.seq);
            let (line, col) = if traversal.lint_mode {
                traversal
                    .position_in_text_locator
                    .locate(traversal.ast.utf16_offset(node, traversal.ast.start_offset(node)))
            } else {
                (0, 0)
            };
            return Err(RuleExecutionException {
                rule_id: r.rule_id,
                about: r.rule.about(),
                line,
                col,
                cause: panic_message(payload),
            });
        }
    }
    for r in &mut rules {
        execute(&mut *r.rule, |rule| rule.after_last_node())?;
    }
    Ok(())
}

/// One traversal of `rules` over `ast` with a fixed `editor_config` for every rule, in format mode;
/// public for measurements (the engine's traversal minus the per-rule `.editorconfig` view).
pub fn execute_rules(
    ast: &mut Ast,
    rules: Vec<Box<dyn RuleV2>>,
    editor_config: &EditorConfig,
    suppression_locator: &mut SuppressionLocator,
    emit_and_approve: &mut EmitAndApprove<'_>,
) -> Result<(), String> {
    let position_in_text_locator = PositionInTextLocator::new("");
    let parts = TraversalParts {
        ast,
        suppression_locator,
        position_in_text_locator: &position_in_text_locator,
        lint_mode: false,
    };
    let dispatch = RuleDispatch::build(
        rules.iter().map(|r| (r.visited_types(), r.visits_after_child_nodes())),
    );
    traverse(
        parts,
        rules,
        &dispatch,
        &|_| editor_config,
        emit_and_approve,
    )
    .map_err(|e| e.cause)
}

/// `rule.execute { }`: an exception outside a node visit has no position.
fn execute(
    rule: &mut dyn RuleV2,
    action: impl FnOnce(&mut dyn RuleV2),
) -> Result<(), RuleExecutionException> {
    let (rule_id, about) = (rule.rule_id(), rule.about());
    catch_rule_panic(|| action(rule)).map_err(|payload| {
        RuleExecutionException {
            rule_id,
            about,
            line: 0,
            col: 0,
            cause: panic_message(payload),
        }
    })
}

pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

const NO_RULE: usize = usize::MAX;

struct Traversal<'a, 't, 'e> {
    ast: &'a mut Ast,
    suppression_locator: &'a mut SuppressionLocator,
    position_in_text_locator: &'a PositionInTextLocator<'t>,
    lint_mode: bool,
    emit_and_approve: &'a mut EmitAndApprove<'e>,
    /// One stack of `getChildren(null)` snapshots for the whole walk, so it allocates nothing per node.
    children: Vec<NodeId>,
    seq: u64,
    dispatch: &'a RuleDispatch,
    current_rule: Cell<usize>,
    current_node: Cell<NodeId>,
}

impl Traversal<'_, '_, '_> {
    fn execute_rules_on_node_recursively(
        &mut self,
        node: NodeId,
        rules: &mut [RuleInstance],
    ) {
        self.seq += 1;
        let entry_seq = self.seq;
        let kind = self.ast.element_type(node);
        let verify = VERIFY_VISITED_TYPES.load(Ordering::Relaxed);
        // Skipped hooks change nothing, so the replaced check only needs refreshing after a visit.
        let mut replaced = if self.lint_mode { false } else { is_replaced(self.ast, node) };
        // The rules from here on still ask upstream's replaced check, skipped ones included.
        let mut unchecked_from = 0;
        for &index in self.dispatch.rules_for(kind) {
            let index = usize::from(index);
            let r = &mut rules[index];
            if !r.visits(entry_seq) {
                continue;
            }
            // A node replaced by an earlier rule (e.g. `rawReplaceWithText`) is not processed further.
            if !self.lint_mode && replaced {
                return;
            }
            let mod_count_before = if self.lint_mode { 0 } else { self.ast.modification_count() };
            self.current_rule.set(index);
            self.current_node.set(node);
            self.visit_hook(node, r, true);
            self.current_rule.set(NO_RULE);
            r.note_stop(self.seq);
            if !self.lint_mode && self.ast.modification_count() != mod_count_before {
                replaced = is_replaced(self.ast, node);
            }
            if !self.lint_mode {
                unchecked_from = index + 1;
            }
        }
        if !self.lint_mode && replaced && rules[unchecked_from..].iter().any(|r| r.visits(entry_seq)) {
            return;
        }
        if verify {
            for r in rules.iter_mut() {
                if !r.visited_types.is_none_or(|types| types.contains(kind)) {
                    self.verify_skipped_visit(node, r);
                }
            }
        }
        let start = self.children.len();
        self.ast.get_children(node, &mut self.children);
        let end = self.children.len();
        for i in start..end {
            let child = self.children[i];
            self.execute_rules_on_node_recursively(child, rules);
        }
        self.children.truncate(start);
        // A no-op hook neither emits nor stops, and the next rule repeats the replaced check.
        let mut after_replaced = if self.lint_mode { false } else { is_replaced(self.ast, node) };
        for &index in &self.dispatch.after {
            let index = usize::from(index);
            let r = &mut rules[index];
            if !r.visits(entry_seq) || !r.visits_after_kind(kind) {
                continue;
            }
            if !self.lint_mode && after_replaced {
                return;
            }
            let mod_count_before = if self.lint_mode { 0 } else { self.ast.modification_count() };
            self.current_rule.set(index);
            self.current_node.set(node);
            self.visit_hook(node, r, false);
            self.current_rule.set(NO_RULE);
            r.note_stop(self.seq);
            if !self.lint_mode && self.ast.modification_count() != mod_count_before {
                after_replaced = is_replaced(self.ast, node);
            }
        }
    }

    /// Runs a hook that `visited_types` excludes and panics if it emits or edits the tree.
    fn verify_skipped_visit(&mut self, node: NodeId, r: &mut RuleInstance) {
        let (rule_id, kind) = (r.rule_id, self.ast.element_type(node));
        let modification_count = self.ast.modification_count();
        r.rule.before_visit_child_nodes(self.ast, node, &mut |_, _, message, _| {
            panic!("{rule_id}: visited_types misses {kind:?} (emitted {message:?})")
        });
        assert_eq!(
            self.ast.modification_count(),
            modification_count,
            "{rule_id}: visited_types misses {kind:?} (edited the tree)"
        );
    }

    /// The part of a visit the catch covers: the suppression check, then the hook unless suppressed.
    fn visit_hook(&mut self, node: NodeId, r: &mut RuleInstance, before: bool) {
        let root = self.ast.root();
        let rule_id = r.rule_id;
        if !self.suppression_locator.suppress(self.ast, root, node, rule_id, r.ignores_suppressions) {
            let emit_and_approve = &mut *self.emit_and_approve;
            let mut emit =
                |ast: &Ast, offset: usize, message: &str, can_be_auto_corrected: bool| {
                    let tree = ast.tree_root(node);
                    emit_and_approve(
                        ast.utf16_offset(tree, offset),
                        rule_id,
                        message,
                        can_be_auto_corrected,
                    )
                };
            if before {
                r.rule.before_visit_child_nodes(self.ast, node, &mut emit);
            } else {
                r.rule.after_visit_child_nodes(self.ast, node, &mut emit);
            }
        }
    }
}

/// The 2.0 bail-out: a node without parent that is not the file was replaced.
fn is_replaced(ast: &Ast, node: NodeId) -> bool {
    ast.tree_parent(node).is_none() && ast.element_type(node) != FILE
}
