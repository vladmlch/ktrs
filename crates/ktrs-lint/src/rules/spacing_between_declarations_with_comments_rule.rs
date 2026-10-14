//! Port of ktlint-ruleset-standard `SpacingBetweenDeclarationsWithCommentsRule.kt`.

use ktrs_ast::{Ast, NodeId};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::{AstNodeExtension, AstNodeQueries};
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;
use crate::token_sets::COMMENTS;

/// See <https://youtrack.jetbrains.com/issue/KT-35088>.
pub struct SpacingBetweenDeclarationsWithCommentsRule;

impl RuleV2 for SpacingBetweenDeclarationsWithCommentsRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:spacing-between-declarations-with-comments")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(COMMENTS)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if let Some(declaration) = Some(node)
            .filter(|&it| COMMENTS.contains(ast.element_type(it)))
            .filter(|&it| !is_tail_comment(ast, it))
            .and_then(|it| ast.parent(it))
            .filter(|&it| ast.is_declaration(it))
            .filter(|&it| ast.is_declaration(ast.prev_code_sibling(it)))
        {
            visit_commented_declaration(ast, declaration, emit);
        }
    }
}

fn is_tail_comment(ast: &Ast, node: NodeId) -> bool {
    ast.start_offset(node) > ast.start_offset(ast.parent(node).expect("NullPointerException: parent"))
}

fn visit_commented_declaration(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    if let Some(prev_sibling) = ast.prev_sibling(node).filter(|&it| !is_blank_line(ast, it)) {
        emit(ast, ast.start_offset(node), "Declarations and declarations with comments should have an empty space between.", true)
            .if_autocorrect_allowed(|| {
                let indent = ast.prev_leaf(node).map(|it| ast.leaf_text(it).trim_matches('\n')).unwrap_or("");
                ast.replace_text_with(prev_sibling, &format!("\n\n{indent}"));
            });
    }
}

fn is_blank_line(ast: &Ast, node: NodeId) -> bool {
    ast.is_white_space(node) && ast.leaf_text(node).starts_with("\n\n")
}
