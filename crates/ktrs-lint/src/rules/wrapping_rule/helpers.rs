//! `WrappingRule.kt` from `isMultiLine` to `getEndOfBlock` (`afterVisitChildNodes` is `after_visit_block`).

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    self, BLOCK, CALL_EXPRESSION, DOT, FOR, FUNCTION_LITERAL, LBRACE, LITERAL_STRING_TEMPLATE_ENTRY, LPAR, RBRACE, RPAR,
};

use super::WrappingRule;
use crate::ast_node_extension::{AstNodeExtension, AstNodeLines};
use crate::rule::Emit;

pub(super) fn is_multi_line(ast: &Ast, node: NodeId) -> bool {
    ast.children(node).any(|child| ast.element_type(child) == LITERAL_STRING_TEMPLATE_ENTRY && ast.text_matches(child, "\n"))
}

pub(super) fn has_line_break(ast: &Ast, node: NodeId, ignore_element_types: &[SyntaxKind]) -> bool {
    if ast.is_white_space_with_newline(node) {
        return true;
    }
    if ignore_element_types.is_empty() {
        ast.text_contains(node, '\n')
    } else {
        !ignore_element_types.contains(&ast.element_type(node))
            && ast.children(node).any(|c| ast.text_contains(c, '\n') && !ignore_element_types.contains(&ast.element_type(c)))
    }
}

pub(super) fn is_followed_by_trim_indent(ast: &Ast, node: NodeId) -> bool {
    is_followed_by(ast, node, "trimIndent()")
}

pub(super) fn is_followed_by_trim_margin(ast: &Ast, node: NodeId) -> bool {
    is_followed_by(ast, node, "trimMargin()")
}

fn is_followed_by(ast: &Ast, node: NodeId, call_expression_name: &str) -> bool {
    ast.next_sibling_matching(node, |it| ast.element_type(it) != DOT)
        .is_some_and(|it| ast.element_type(it) == CALL_EXPRESSION && ast.text_matches(it, call_expression_name))
}

/// Allows a for-statement in which only the expression contains a newline (`for (foo in listOf(\n"foo-1"\n)) {`),
/// but rejects `for (\nfoo in listOf(...)\n) {`.
pub(super) fn is_part_of_for_loop_condition_with_multiline_expression(ast: &Ast, n: NodeId) -> bool {
    let Some(parent) = ast.parent(n).filter(|&it| ast.element_type(it) == FOR) else { return false };
    if ast.element_type(n) != LPAR {
        let lpar = ast.find_child_by_type(parent, LPAR).expect("NullPointerException: findChildByType(LPAR)!!");
        return is_part_of_for_loop_condition_with_multiline_expression(ast, lpar);
    }

    let mut node = Some(n);
    while let Some(x) = node.filter(|&x| ast.element_type(x) != RPAR) {
        if ast.is_white_space_with_newline(x) {
            return false;
        }
        node = ast.next_sibling(x);
    }
    true
}

impl WrappingRule {
    pub(super) fn after_visit_block(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.element_type(node) != BLOCK {
            return;
        }
        let Some(lbrace) = get_start_of_block(ast, node).filter(|&it| ast.element_type(it) == LBRACE) else { return };
        if let Some(rbrace) = get_end_of_block(ast, node)
            .filter(|&it| ast.element_type(it) == RBRACE)
            .filter(|&it| !is_preceded_by_newline(ast, it))
            && ast.has_new_line_in_closed_range(lbrace, rbrace)
        {
            let indent = self.indent_config.parent_indent_of(ast, node);
            self.require_newline_before_leaf(ast, rbrace, emit, &indent);
        }
    }
}

pub(super) fn followed_by_newline(ast: &Ast, node: NodeId) -> bool {
    ast.is_white_space_with_newline(ast.next_leaf(node))
}

fn is_preceded_by_newline(ast: &Ast, node: NodeId) -> bool {
    ast.is_white_space_with_newline(ast.prev_leaf(node))
}

pub(super) fn get_start_of_block(ast: &Ast, node: NodeId) -> Option<NodeId> {
    match ast.parent(node).filter(|&it| ast.element_type(it) == FUNCTION_LITERAL) {
        Some(parent) => ast.find_child_by_type(parent, LBRACE),
        None => {
            let first = ast.first_child_leaf_or_self(node);
            if ast.element_type(first) == LBRACE {
                // WHEN-entry block have LBRACE and RBRACE as first and last elements
                Some(first)
            } else {
                // Other blocks have LBRACE and RBRACE as siblings of the block
                ast.prev_code_sibling(first)
            }
        }
    }
}

pub(super) fn get_end_of_block(ast: &Ast, node: NodeId) -> Option<NodeId> {
    match ast.parent(node).filter(|&it| ast.element_type(it) == FUNCTION_LITERAL) {
        Some(parent) => ast.find_child_by_type(parent, RBRACE),
        None => {
            let last = ast.last_child_leaf_or_self(node);
            if ast.element_type(last) == RBRACE {
                // WHEN-entry block have LBRACE and RBRACE as first and last elements
                Some(last)
            } else {
                // Other blocks have LBRACE and RBRACE as siblings of the block
                ast.next_code_sibling(last)
            }
        }
    }
}
