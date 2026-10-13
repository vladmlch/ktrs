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

pub(crate) struct RuleExecutionContext {
    file_path_or_stdin: String,
    pub(crate) ast: Ast,
    /// The `.editorconfig` and the rules it enables.
    pub(crate) setup: Arc<RuleSetup>,
    /// Built from the original text and kept for every pass (so later positions are stale, as upstream).
    pub(crate) position_in_text_locator: Rc<PositionInTextLocator>,
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
    visits_after: bool,
}

impl RuleInstance {
    fn new(rule: Box<dyn RuleV2>) -> RuleInstance {
        RuleInstance {
            rule_id: rule.rule_id(),
            ignores_suppressions: rule.ignores_ktlint_suppressions(),
            can_stop: rule.traversal_state().is_some(),
            visited_types: rule.visited_types(),
            visits_after: rule.visits_after_child_nodes(),
            rule,
            stopped_at: None,
        }
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

impl RuleExecutionContext {
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
        self.execute_rules_on_ast(suppression, lint_mode, emit_and_approve)?;
        self.execute_rules_on_ast(others, lint_mode, emit_and_approve)
    }

    fn execute_rules_on_ast(
        &mut self,
        rules: Vec<Box<dyn RuleV2>>,
        lint_mode: bool,
        emit_and_approve: &mut EmitAndApprove<'_>,
    ) -> Result<(), KtLintRuleException> {
        let setup = &self.setup;
        let parts = TraversalParts {
            ast: &mut self.ast,
            suppression_locator: &mut self.suppression_locator,
            position_in_text_locator: &self.position_in_text_locator,
            lint_mode,
        };
        let rule_editor_config = |rule: &dyn RuleV2| setup.rule_editor_config(rule);
        traverse(parts, rules, &rule_editor_config, emit_and_approve).map_err(|e| self.to_ktlint_rule_exception(e))
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

struct TraversalParts<'a> {
    ast: &'a mut Ast,
    suppression_locator: &'a mut SuppressionLocator,
    position_in_text_locator: &'a PositionInTextLocator,
    lint_mode: bool,
}

/// One traversal: `beforeFirstNode` for all rules, the node walk, `afterLastNode` for all rules.
fn traverse<C: Deref<Target = EditorConfig>>(
    parts: TraversalParts<'_>,
    rules: Vec<Box<dyn RuleV2>>,
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
    let mut traversal = Traversal {
        ast: parts.ast,
        suppression_locator: parts.suppression_locator,
        position_in_text_locator: parts.position_in_text_locator,
        lint_mode: parts.lint_mode,
        emit_and_approve,
        children: Vec::new(),
        seq: 0,
        dispatch: RuleDispatch::new(rules.iter().map(|r| r.visits_after)),
    };
    let root = traversal.ast.root();
    traversal.execute_rules_on_node_recursively(root, &mut rules)?;
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
    traverse(
        parts,
        rules,
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

struct Traversal<'a, 'e> {
    ast: &'a mut Ast,
    suppression_locator: &'a mut SuppressionLocator,
    position_in_text_locator: &'a PositionInTextLocator,
    lint_mode: bool,
    emit_and_approve: &'a mut EmitAndApprove<'e>,
    /// One stack of `getChildren(null)` snapshots for the whole walk, so it allocates nothing per node.
    children: Vec<NodeId>,
    seq: u64,
    dispatch: RuleDispatch,
}

impl Traversal<'_, '_> {
    fn execute_rules_on_node_recursively(
        &mut self,
        node: NodeId,
        rules: &mut [RuleInstance],
    ) -> Result<(), RuleExecutionException> {
        self.seq += 1;
        let entry_seq = self.seq;
        let kind = self.ast.element_type(node);
        let verify = VERIFY_VISITED_TYPES.load(Ordering::Relaxed);
        // One catch for the whole node instead of one per visit: `current` names the rule whose hook
        // is running, so its panic is still attributed to it exactly as the per-visit catch did. A
        // panic anywhere else (the traversal machinery, `note_stop`, `verify_skipped_visit`'s
        // assertions) is not a rule exception and is resumed untouched, as it escaped before.
        const NO_RULE: usize = usize::MAX;
        let current = Cell::new(NO_RULE);
        let outcome = catch_rule_panic(|| -> Result<(), RuleExecutionException> {
            // Skipped hooks change nothing, so the replaced check only needs refreshing after a visit.
            let mut replaced = is_replaced(self.ast, node);
            // The rules from here on still ask upstream's replaced check, skipped ones included.
            let mut unchecked_from = 0;
            let (start, len) = self.dispatch.rules_for(kind, rules.iter().map(|r| r.visited_types), verify);
            for position in start..start + len {
                let index = self.dispatch.rule_at(position);
                let r = &mut rules[index];
                if !r.visits(entry_seq) {
                    continue;
                }
                // A node replaced by an earlier rule (e.g. `rawReplaceWithText`) is not processed further.
                if replaced {
                    return Ok(());
                }
                if r.visited_types.is_none_or(|types| types.contains(kind)) {
                    current.set(index);
                    self.visit_hook(node, r, true);
                    current.set(NO_RULE);
                    r.note_stop(self.seq);
                    replaced = is_replaced(self.ast, node);
                } else if verify {
                    self.verify_skipped_visit(node, r);
                }
                unchecked_from = index + 1;
            }
            if replaced && rules[unchecked_from..].iter().any(|r| r.visits(entry_seq)) {
                return Ok(());
            }
            let start = self.children.len();
            self.ast.get_children(node, &mut self.children);
            let end = self.children.len();
            for i in start..end {
                let child = self.children[i];
                self.execute_rules_on_node_recursively(child, rules)?;
            }
            self.children.truncate(start);
            // A no-op hook neither emits nor stops, and the next rule repeats the replaced check.
            for position in 0..self.dispatch.after.len() {
                let index = usize::from(self.dispatch.after[position]);
                let r = &mut rules[index];
                if !r.visits(entry_seq) {
                    continue;
                }
                if is_replaced(self.ast, node) {
                    return Ok(());
                }
                current.set(index);
                self.visit_hook(node, r, false);
                current.set(NO_RULE);
                r.note_stop(self.seq);
            }
            Ok(())
        });
        match outcome {
            Ok(result) => result,
            Err(payload) => {
                let index = current.get();
                if index == NO_RULE {
                    std::panic::resume_unwind(payload);
                }
                let r = &mut rules[index];
                // The per-visit catch ran `note_stop` after a caught panic too (between the catch
                // and the exception), so keep running it before attributing.
                r.note_stop(self.seq);
                // In format mode the node may not be in the original text, so no position is given.
                let (line, col) = if self.lint_mode {
                    self.position_in_text_locator
                        .locate(self.ast.utf16_offset(node, self.ast.start_offset(node)))
                } else {
                    (0, 0)
                };
                Err(RuleExecutionException {
                    rule_id: r.rule_id,
                    about: r.rule.about(),
                    line,
                    col,
                    cause: panic_message(payload),
                })
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
        let tree = self.ast.tree_root(node);
        if !self.suppression_locator.suppress(self.ast, root, node, rule_id, r.ignores_suppressions) {
            let emit_and_approve = &mut *self.emit_and_approve;
            let mut emit =
                |ast: &Ast, offset: usize, message: &str, can_be_auto_corrected: bool| {
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
