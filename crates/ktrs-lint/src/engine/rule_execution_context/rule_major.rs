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

impl RuleExecutionContext {
    /// 1.8 `executeRule(rule, autocorrectHandler, emitAndApprove)`.
    pub(super) fn execute_rule_1_8(
        &mut self,
        mut rule: Box<dyn RuleV2>,
        lint_mode: bool,
        emit_and_approve: &mut EmitAndApprove<'_>,
    ) -> Result<(), KtLintRuleException> {
        let editor_config = self.setup.rule_editor_config(&*rule);
        let mut traversal = RuleTraversal {
            ast: &mut self.ast,
            suppression_locator: &mut self.suppression_locator,
            emit_and_approve,
            rule_id: rule.rule_id(),
            ignores_suppressions: rule.ignores_ktlint_suppressions(),
            visited_types: rule.visited_types(),
            visits_after: rule.visits_after_child_nodes(),
            children: Vec::new(),
        };
        let result = execute(&mut *rule, |rule| rule.before_first_node(&editor_config))
            .and_then(|()| {
                let root = traversal.ast.root();
                traversal.execute_rule_on_node_recursively(root, &mut *rule).map_err(|cause| {
                    // The exception is rethrown at every level up to the root, which gives the position.
                    let (line, col) = if lint_mode { self.position_in_text_locator.locate(0) } else { (0, 0) };
                    RuleExecutionException { rule_id: rule.rule_id(), about: rule.about(), line, col, cause }
                })
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
    visits_after: bool,
    /// One stack of `getChildren(null)` snapshots for the whole walk.
    children: Vec<NodeId>,
}

fn should_continue_traversal_of_ast(rule: &dyn RuleV2) -> bool {
    rule.traversal_state().is_none_or(|t| !t.is_stopped())
}

impl RuleTraversal<'_, '_> {
    /// `Err` is the panic message.
    fn execute_rule_on_node_recursively(&mut self, node: NodeId, rule: &mut dyn RuleV2) -> Result<(), String> {
        if !should_continue_traversal_of_ast(rule) {
            return Ok(());
        }
        let visits_before = self.visited_types.is_none_or(|types| types.contains(self.ast.element_type(node)));
        // Hooks that do nothing need no answer, but a visited one gets the answer from before any edit.
        let suppress = (visits_before || self.visits_after)
            && self.suppression_locator.suppress(self.ast, self.ast.root(), node, self.rule_id, self.ignores_suppressions);
        // One catch for the whole node instead of one per visit: `in_hook` marks the region whose
        // panics become `Err` (the hooks'). A panic anywhere else (the traversal machinery,
        // `traversal_state`) is not a rule exception and is resumed untouched, as it escaped before.
        let in_hook = Cell::new(false);
        let outcome = catch_rule_panic(|| -> Result<(), String> {
            if !suppress && visits_before {
                in_hook.set(true);
                self.visit_hook(node, rule, true);
                in_hook.set(false);
            }
            if should_continue_traversal_of_ast(rule) {
                let start = self.children.len();
                self.ast.get_children(node, &mut self.children);
                let end = self.children.len();
                for i in start..end {
                    let child = self.children[i];
                    self.execute_rule_on_node_recursively(child, rule)?;
                }
                self.children.truncate(start);
            }
            if !suppress && self.visits_after {
                in_hook.set(true);
                self.visit_hook(node, rule, false);
                in_hook.set(false);
            }
            Ok(())
        });
        match outcome {
            Ok(result) => result,
            Err(payload) => {
                if !in_hook.get() {
                    std::panic::resume_unwind(payload);
                }
                Err(panic_message(payload))
            }
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
