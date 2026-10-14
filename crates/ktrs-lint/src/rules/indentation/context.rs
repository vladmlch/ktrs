//! `IndentationRule.kt` from `skipLeadingWhitespaceCommentsAndAnnotations` to `afterLastNode`: the indent context stack.

use std::sync::Arc;

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{ANNOTATION_ENTRY, BINARY_EXPRESSION, CONDITION, MODIFIER_LIST, SECONDARY_CONSTRUCTOR, VALUE_PARAMETER};

use super::{Args, IndentContext, IndentationRule, StartedIndentContext};
use crate::ast_node_extension::AstNodeExtension;

pub(super) fn skip_leading_whitespace_comments_and_annotations(ast: &Ast, this: NodeId) -> NodeId {
    assert!(
        matches!(ast.element_type(this), SECONDARY_CONSTRUCTOR | VALUE_PARAMETER),
        "IllegalArgumentException: Failed requirement."
    );
    ast.find_child_by_type(this, MODIFIER_LIST)
        .and_then(|modifier_list| {
            ast.children(modifier_list)
                .find(|&it| ast.is_code(it) && ast.element_type(it) != ANNOTATION_ENTRY)
                .or_else(|| ast.next_code_sibling(modifier_list))
        })
        .or_else(|| ast.children(this).find(|&it| ast.is_code(it)))
        .unwrap_or(this)
}

pub(super) fn get_preceding_leading_comments_and_whitespaces(ast: &Ast, this: NodeId) -> NodeId {
    let mut from_ast_node = this;
    while let Some(prev_leaf) = ast.prev_leaf(from_ast_node) {
        if !(ast.is_white_space(prev_leaf) || ast.is_part_of_comment(prev_leaf)) {
            break;
        }
        from_ast_node = prev_leaf;
    }
    from_ast_node
}

pub(super) fn is_part_of_binary_expression_wrapped_in_condition(ast: &Ast, node: NodeId) -> bool {
    ast.parents(node)
        .take_while(|&it| matches!(ast.element_type(it), BINARY_EXPRESSION | CONDITION))
        .last()
        .map(|it| ast.element_type(it))
        == Some(CONDITION)
}

impl IndentationRule {
    pub(super) fn current_indent_arc(&self) -> Arc<str> {
        self.indent_context_stack.last().expect("NullPointerException: peekLast().indent()").indent_arc()
    }

    pub(super) fn current_indent(&self) -> String {
        self.current_indent_arc().to_string()
    }

    /// The defaults: `to` = the last leaf of `from`, `node_indent` = [`Self::current_indent`], `child_indent` = one indent,
    /// `first_child_indent` and `last_child_indent` = `child_indent`.
    pub(super) fn start_indent_context(&mut self, ast: &Ast, from_ast_node: NodeId, args: Args) -> StartedIndentContext {
        let to_ast_node = args.to_ast_node.unwrap_or_else(|| ast.last_child_leaf_or_self(from_ast_node));
        let node_indent = args.node_indent.unwrap_or_else(|| self.current_indent_arc());
        let child_indent = args.child_indent.unwrap_or_else(|| self.cached_indent.clone());
        let first_child_indent = args.first_child_indent.unwrap_or_else(|| child_indent.clone());
        let last_child_indent = args.last_child_indent.unwrap_or_else(|| child_indent.clone());
        self.indent_context_stack.push(IndentContext {
            from_ast_node,
            to_ast_node,
            node_indent,
            first_child_indent,
            child_indent,
            last_child_indent,
            activated: args.activated,
            node_types: IndentContext::node_types(ast, from_ast_node, to_ast_node),
        });
        StartedIndentContext { from_ast_node }
    }

    pub(super) fn after_visit_child_nodes_impl(&mut self, ast: &Ast, node: NodeId) {
        while let Some(last) = self.indent_context_stack.last() {
            // End visit of the node matches with the toAstNode on top of the stack
            let ends_here = last.to_ast_node == node
                // When the ktlint indentation violations on child elements have been suppressed, those child elements are not visited,
                // and the stack is therefore not cleaned up properly. So remove all elements from top of the stack until the start of an
                // index context is found for the current node.
                || ast.is_ancestor(node, last.to_ast_node);
            if !ends_here {
                break;
            }
            self.indent_context_stack.pop();
        }
    }

    pub(super) fn after_last_node_impl(&mut self) {
        if !self.indent_context_stack.is_empty() {
            let contexts: Vec<String> = self.indent_context_stack.iter().map(ToString::to_string).collect();
            panic!("IllegalArgumentException: Stack should be empty:\n\t{}", contexts.join("\n\t"));
        }
    }
}
