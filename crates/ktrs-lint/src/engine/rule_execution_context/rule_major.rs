//! ktlint 1.8.0's traversal (`RuleExecutionContext.executeRule`, replaced in 2.0 by #3252): each rule visits
//! the whole tree, then the next rule starts. Unlike 2.0 it has no bail-out for replaced nodes, and asks the
//! suppression locator once per node, before `beforeVisitChildNodes`, for both hooks.

use std::cell::Cell;

use ktrs_ast::{Ast, NodeId};

use super::{EmitAndApprove, RuleExecutionContext, RuleExecutionException, execute, panic_message};
use crate::engine::code::KtLintRuleException;
use crate::engine::rule_panic::catch_rule_panic;
use crate::engine::suppression_locator::SuppressionLocator;
use crate::rule::{RuleId, RuleV2, TokenSet};

impl<'a> RuleExecutionContext<'a> {
    /// 1.8 `executeRule(rule, autocorrectHandler, emitAndApprove)`.
    pub(super) fn execute_rule_1_8(
        &mut self,
        mut rule: Box<dyn RuleV2>,
        lint_mode: bool,
        emit_and_approve: &mut EmitAndApprove<'_>,
    ) -> Result<(), KtLintRuleException> {
        let editor_config = self.setup.rule_editor_config(&*rule);
        let root = self.ast.root();
        let mut traversal = RuleTraversal {
            ast: &mut self.ast,
            suppression_locator: &mut self.suppression_locator,
            emit_and_approve,
            rule_id: rule.rule_id(),
            ignores_suppressions: rule.ignores_ktlint_suppressions(),
            visited_types: rule.visited_types(),
            visited_after_types: rule.visited_after_types(),
            visits_after: rule.visits_after_child_nodes(),
            children: Vec::new(),
            in_hook: Cell::new(false),
            current_node: Cell::new(root),
        };
        let result = execute(&mut *rule, |rule| rule.before_first_node(&editor_config))
            .and_then(|()| {
                let outcome = catch_rule_panic(std::panic::AssertUnwindSafe(|| {
                    traversal.execute_rule_on_node_recursively(root, &mut *rule);
                }));
                match outcome {
                    Ok(()) => Ok(()),
                    Err(payload) => {
                        if !traversal.in_hook.get() {
                            std::panic::resume_unwind(payload);
                        }
                        let node = traversal.current_node.get();
                        let (line, col) = if lint_mode {
                            self.position_in_text_locator
                                .locate(traversal.ast.utf16_offset(node, traversal.ast.start_offset(node)))
                        } else {
                            (0, 0)
                        };
                        Err(RuleExecutionException {
                            rule_id: rule.rule_id(),
                            about: rule.about(),
                            line,
                            col,
                            cause: panic_message(payload),
                        })
                    }
                }
            })
            .and_then(|()| execute(&mut *rule, |rule| rule.after_last_node()));
        result.map_err(|e| self.to_ktlint_rule_exception(e))
    }
}

struct RuleTraversal<'a, 'e> {
    ast: &'a mut Ast,
    suppression_locator: &'a mut SuppressionLocator,
    emit_and_approve: &'a mut EmitAndApprove<'e>,
    rule_id: RuleId,
    ignores_suppressions: bool,
    visited_types: Option<TokenSet>,
    visited_after_types: Option<TokenSet>,
    visits_after: bool,
    /// One stack of `getChildren(null)` snapshots for the whole walk.
    children: Vec<NodeId>,
    in_hook: Cell<bool>,
    current_node: Cell<NodeId>,
}

fn should_continue_traversal_of_ast(rule: &dyn RuleV2) -> bool {
    rule.traversal_state().is_none_or(|t| !t.is_stopped())
}

impl RuleTraversal<'_, '_> {
    fn execute_rule_on_node_recursively(&mut self, node: NodeId, rule: &mut dyn RuleV2) {
        if !should_continue_traversal_of_ast(rule) {
            return;
        }
        let kind = self.ast.element_type(node);
        let visits_before = self.visited_types.is_none_or(|types| types.contains(kind));
        let visits_after = self.visits_after && self.visited_after_types.is_none_or(|types| types.contains(kind));
        // Hooks that do nothing need no answer, but a visited one gets the answer from before any edit.
        let suppress = (visits_before || visits_after)
            && self.suppression_locator.suppress(self.ast, self.ast.root(), node, self.rule_id, self.ignores_suppressions);
        if !suppress && visits_before {
            self.current_node.set(node);
            self.in_hook.set(true);
            self.visit_hook(node, rule, true);
            self.in_hook.set(false);
        }
        if should_continue_traversal_of_ast(rule) {
            let start = self.children.len();
            self.ast.get_children(node, &mut self.children);
            let end = self.children.len();
            for i in start..end {
                let child = self.children[i];
                self.execute_rule_on_node_recursively(child, rule);
            }
            self.children.truncate(start);
        }
        if !suppress && visits_after {
            self.current_node.set(node);
            self.in_hook.set(true);
            self.visit_hook(node, rule, false);
            self.in_hook.set(false);
        }
    }

    /// The part of a visit the catch covers: the hook itself (the suppression answer is per node).
    fn visit_hook(&mut self, node: NodeId, rule: &mut dyn RuleV2, before: bool) {
        let rule_id = self.rule_id;
        let emit_and_approve = &mut *self.emit_and_approve;
        let ast = &mut *self.ast;
        let tree = ast.tree_root(node);
        let mut emit = |ast: &Ast, offset: usize, message: &str, can_be_auto_corrected: bool| {
            emit_and_approve(ast.utf16_offset(tree, offset), rule_id, message, can_be_auto_corrected)
        };
        if before {
            rule.before_visit_child_nodes(ast, node, &mut emit);
        } else {
            rule.after_visit_child_nodes(ast, node, &mut emit);
        }
    }
}
