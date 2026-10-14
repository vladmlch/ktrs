//! Port of ktlint-ruleset-standard `SpacingAroundDotRule.kt` (id `dot-spacing`).

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::DOT;

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[DOT]);

pub struct SpacingAroundDotRule;

impl RuleV2 for SpacingAroundDotRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:dot-spacing")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if !(ast.element_type(node) == DOT && ast.is_leaf_element(node) && ast.is_code(node)) {
            return;
        }
        if let Some(prev_leaf) = ast.prev_leaf(node).filter(|&it| ast.is_white_space_without_newline(it)) {
            let message = format!("Unexpected spacing before \"{}\"", ast.text(node));
            emit(ast, ast.start_offset(prev_leaf), &message, true).if_autocorrect_allowed(|| ast.remove(prev_leaf));
        }
        if let Some(next_leaf) = ast.next_leaf(node).filter(|&it| ast.is_white_space(it)) {
            let message = format!("Unexpected spacing after \"{}\"", ast.text(node));
            emit(ast, ast.start_offset(next_leaf), &message, true).if_autocorrect_allowed(|| ast.remove(next_leaf));
        }
    }
}
