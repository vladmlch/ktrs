//! `WrappingRule.kt` from `rearrangeClosingQuote` to `requireNewlineAfterLeaf`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{FUNCTION_LITERAL, LITERAL_STRING_TEMPLATE_ENTRY, RBRACE, STRING_TEMPLATE, WHEN_ENTRY};

use super::helpers::{is_followed_by_trim_indent, is_followed_by_trim_margin, is_multi_line};
use super::{LTOKEN_SET, WrappingRule, matching_rtoken};
use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::Emit;

impl WrappingRule {
    pub(super) fn rearrange_closing_quote(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let applies = ast
            .parent(node)
            .filter(|&it| ast.element_type(it) == STRING_TEMPLATE)
            .filter(|&it| is_multi_line(ast, it))
            .filter(|&it| is_followed_by_trim_indent(ast, it) || is_followed_by_trim_margin(ast, it))
            .filter(|_| !ast.prev_sibling(node).is_none_or(|it| if ast.is_leaf_element(it) { ast.leaf_text(it).chars().all(char::is_whitespace) } else { ast.text_chunks(it).all(|c| c.chars().all(char::is_whitespace)) }))
            .is_some();
        if applies {
            // rewriting `"""\n    text\n_""".trimIndent()` to `"""\n    text\n_\n""".trimIndent()`
            emit(ast, ast.start_offset(node), "Missing newline before \"\"\"", true).if_autocorrect_allowed(|| {
                let entry = ast.new_leaf(LITERAL_STRING_TEMPLATE_ENTRY, "\n");
                ast.raw_insert_before_me(node, entry);
            });
        }
    }

    pub(super) fn rearrange_arrow(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let parent = ast.parent(node).expect("NullPointerException: parent!!");
        let parent_type = ast.element_type(parent);
        if
        // check `{ p -> ... }` and `when { m -> ... }` only
        (parent_type != FUNCTION_LITERAL && parent_type != WHEN_ENTRY)
            // ... and only if expression after -> spans multiple lines
            || !ast.text_contains(parent, '\n')
            // permit `when {\n m -> 0 + d({\n })\n}`
            || (parent_type == WHEN_ENTRY && must_be_followed_by_newline(ast, node))
            // permit `when (this) {\n in 0x1F600..0x1F64F, // Emoticons\n 0x200D // Zero-width Joiner\n -> true\n}`
            || (parent_type == WHEN_ENTRY && ast.is_white_space_with_newline(ast.prev_leaf(node)))
        {
            return;
        }
        if !ast.is_white_space_with_newline(ast.next_code_leaf(node).and_then(|it| ast.prev_leaf(it))) {
            self.require_newline_after_leaf(ast, node, emit, None);
        }
        let Some(r) = ast.next_sibling_matching(node, |it| ast.element_type(it) == RBRACE) else { return };
        if !ast.is_white_space_with_newline(ast.prev_leaf(r)) {
            let indent = ast.indent(node);
            self.require_newline_before_leaf(ast, r, emit, &indent);
        }
    }

    pub(super) fn require_newline_before_leaf(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>, indent: &str) {
        let text = if ast.is_leaf_element(node) { std::borrow::Cow::Borrowed(ast.leaf_text(node)) } else { std::borrow::Cow::Owned(ast.text(node)) };
        let message = format!("Missing newline before \"{text}\"");
        emit(ast, ast.start_offset(node) - 1, &message, true).if_autocorrect_allowed(|| ast.upsert_whitespace_before_me(node, indent));
    }

    /// `nodeToFix` is always `nodeAfterWhichNewlineIsRequired` at the call sites.
    pub(super) fn require_newline_after_leaf(
        &self,
        ast: &mut Ast,
        node_after_which_newline_is_required: NodeId,
        emit: &mut Emit<'_>,
        indent: Option<String>,
    ) {
        let node_to_fix = node_after_which_newline_is_required;
        let text = if ast.is_leaf_element(node_to_fix) { std::borrow::Cow::Borrowed(ast.leaf_text(node_to_fix)) } else { std::borrow::Cow::Owned(ast.text(node_to_fix)) };
        let message = format!("Missing newline after \"{text}\"");
        emit(ast, ast.start_offset(node_after_which_newline_is_required) + 1, &message, true).if_autocorrect_allowed(|| {
            let temp_indent = indent.unwrap_or_else(|| self.indent_config.child_indent_of(ast, node_to_fix));
            ast.upsert_whitespace_after_me(node_to_fix, &temp_indent);
        });
    }
}

/// Finds the EOL token (last token before a newline); when it opens a bracket, true if the matching closing one is
/// its sibling.
fn must_be_followed_by_newline(ast: &Ast, node: NodeId) -> bool {
    let next_code_sibling = ast.next_code_sibling(node); // e.g. BINARY_EXPRESSION
    let mut l_token = next_code_sibling
        .and_then(|it| ast.next_leaf_matching(it, |it| ast.is_white_space_with_newline(it)))
        .and_then(|it| ast.prev_code_leaf(it));
    if let Some(l) = l_token
        && !LTOKEN_SET.contains(ast.element_type(l))
    {
        // special cases: `x = y.f({ z ->\n})` and `x = y.f(0, 1,\n2, 3)`
        l_token = ast.prev_leaf_matching(l, |it| LTOKEN_SET.contains(ast.element_type(it)) || it == node);
    }
    if let Some(l) = l_token
        && LTOKEN_SET.contains(ast.element_type(l))
    {
        let r_element_type = matching_rtoken(ast.element_type(l));
        let r_token = ast.next_sibling_matching(l, |it| Some(ast.element_type(it)) == r_element_type);
        return r_token.and_then(|it| ast.parent(it)) == ast.parent(l);
    }
    next_code_sibling.is_some_and(|it| !ast.text_contains(it, '\n'))
}
