//! `StringTemplateIndentRule.kt` from `indentStringTemplate` to the end.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{CALL_EXPRESSION, CLOSING_QUOTE, DOT, LITERAL_STRING_TEMPLATE_ENTRY, OPEN_QUOTE, REGULAR_STRING_PART, STRING_TEMPLATE};

use super::{RAW_STRING_LITERAL_QUOTES, StringTemplateIndentRule};
use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::engine::kotlin_text::{is_kotlin_blank, is_kotlin_whitespace};
use crate::rule::Emit;

impl StringTemplateIndentRule {
    pub(super) fn indent_string_template(&self, ast: &mut Ast, node: NodeId, new_indent: &str, emit: &mut Emit<'_>) {
        // Get the max prefix length that all lines in the multiline string have in common. All whitespace characters are counted as
        // one single position. Note that the way of counting should be in sync with the way this is done by the trimIndent
        // function.
        let prefix_length = ast
            .text(node)
            .split('\n')
            // For a multiline raw string literal it is very unlikely that text after the opening quotes do contain indentation
            // characters (it really looks ugly). In such a case this text should be ignored when calculating the common prefix
            // length as otherwise it is probably set to 0.
            .filter(|it| !it.starts_with(RAW_STRING_LITERAL_QUOTES))
            // Indentation before the closing quotes however is relevant to take into account
            .map(|it| it.strip_suffix(RAW_STRING_LITERAL_QUOTES).unwrap_or(it))
            .filter(|it| !is_kotlin_blank(it))
            .map(indent_length)
            .min()
            .unwrap_or(0);

        check_and_fix_new_line_after_opening_quotes(ast, node, new_indent, emit);

        // Mutations below only insert before the current child, so the children can be taken up front.
        let children: Vec<NodeId> = ast
            .children(node)
            .filter(|&it| ast.element_type(it) != OPEN_QUOTE)
            // Blank lines inside the string template should not be indented
            .filter(|&it| !ast.text_matches(it, "\n"))
            .collect();
        for it in children {
            if ast.prev_leaf(it).is_some_and(|p| ast.text_matches(p, "\n")) {
                let text = ast.text(it);
                let (current_indent, current_content) =
                    if is_indent_before_closing_quote(ast, it) { (text.as_str(), "") } else { split_indent_at(&text, prefix_length) };
                let expected_indent = if is_indent_before_closing_quote(ast, it) || prefix_length > 0 { new_indent } else { "" };
                if current_indent.contains(self.wrong_indent_char()) {
                    self.check_and_fix_wrong_indentation_char(ast, it, current_indent, expected_indent, current_content, emit);
                } else if current_indent != expected_indent {
                    check_and_fix_indent(ast, it, current_indent.len(), expected_indent, current_content, emit);
                }
            }
        }

        check_and_fix_new_line_before_closing_quotes(ast, node, new_indent, emit);
    }

    fn check_and_fix_wrong_indentation_char(
        &self,
        ast: &mut Ast,
        node: NodeId,
        old_indent: &str,
        new_indent: &str,
        new_content: &str,
        emit: &mut Emit<'_>,
    ) {
        let offset = ast.start_offset(node) + old_indent.find(self.wrong_indent_char()).unwrap_or(0);
        let message = format!("Unexpected '{}' character(s) in margin of multiline string", self.wrong_indent_description());
        emit(ast, offset, &message, true).if_autocorrect_allowed(|| {
            let first_child_node = ast.first_child_node(node).expect("NullPointerException: firstChildNode");
            ast.replace_text_with(first_child_node, &format!("{new_indent}{new_content}"));
        });
    }
}

fn check_and_fix_new_line_after_opening_quotes(ast: &mut Ast, node: NodeId, indent: &str, emit: &mut Emit<'_>) {
    // The string template can start with an INTERPOLATION_PREFIX (multi dollar string interpolation, see
    // https://kotlinlang.org/docs/strings.html#multi-dollar-string-interpolation), which is to be skipped.
    let Some(first_node_after_opening_quotes) = ast.find_child_by_type(node, OPEN_QUOTE).and_then(|it| ast.next_leaf(it)) else {
        return;
    };
    let text = ast.leaf_text(first_node_after_opening_quotes);
    if !is_kotlin_blank(text) {
        let offset = ast.start_offset(first_node_after_opening_quotes) + text.len();
        emit(ast, offset, "Missing newline after the opening quotes of the raw string literal", true).if_autocorrect_allowed(|| {
            let leaf = ast.new_leaf(REGULAR_STRING_PART, &format!("\n{indent}"));
            ast.raw_insert_before_me(first_node_after_opening_quotes, leaf);
        });
    }
}

fn check_and_fix_indent(ast: &mut Ast, node: NodeId, old_indent_length: usize, new_indent: &str, new_content: &str, emit: &mut Emit<'_>) {
    emit(ast, ast.start_offset(node) + old_indent_length, "Unexpected indent of raw string literal", true).if_autocorrect_allowed(|| {
        if ast.element_type(node) == CLOSING_QUOTE {
            let leaf = ast.new_leaf(REGULAR_STRING_PART, new_indent);
            ast.raw_insert_before_me(node, leaf);
        } else {
            let first_child_leaf = ast.first_child_leaf_or_self(node);
            ast.replace_text_with(first_child_leaf, &format!("{new_indent}{new_content}"));
        }
    });
}

fn check_and_fix_new_line_before_closing_quotes(ast: &mut Ast, node: NodeId, indent: &str, emit: &mut Emit<'_>) {
    let last_child_node = ast.last_child_node(node).expect("NullPointerException: lastChildNode.prevLeaf");
    let Some(last_node_before_closing_quotes) = ast.prev_leaf(last_child_node) else {
        return;
    };
    let text = ast.leaf_text(last_node_before_closing_quotes);
    if !is_kotlin_blank(text) {
        let offset = ast.start_offset(last_node_before_closing_quotes) + text.len();
        emit(ast, offset, "Missing newline before the closing quotes of the raw string literal", true).if_autocorrect_allowed(|| {
            let leaf = ast.new_leaf(REGULAR_STRING_PART, &format!("\n{indent}"));
            ast.raw_insert_after_me(last_node_before_closing_quotes, leaf);
        });
    }
}

pub(super) fn contains_literal_string_template_entry_with_newline(ast: &Ast, this: NodeId) -> bool {
    assert!(ast.element_type(this) == STRING_TEMPLATE, "IllegalArgumentException: Failed requirement.");
    ast.children(this).any(|it| ast.element_type(it) == LITERAL_STRING_TEMPLATE_ENTRY && ast.text_matches(it, "\n"))
}

pub(super) fn is_followed_by_trim_indent(ast: &Ast, this: NodeId) -> bool {
    assert!(ast.element_type(this) == STRING_TEMPLATE, "IllegalArgumentException: Failed requirement.");
    ast.next_sibling_matching(this, |it| ast.element_type(it) != DOT)
        .is_some_and(|it| ast.element_type(it) == CALL_EXPRESSION && ast.text_matches(it, "trimIndent()"))
}

/// In chars: the indent is whitespace, so chars and UTF-16 units coincide for it.
pub(super) fn indent_length(this: &str) -> usize {
    this.chars().position(|c| !is_kotlin_whitespace(c)).unwrap_or_else(|| this.encode_utf16().count())
}

/// Splits the string at the given index (in chars) or at the first non-white space character before that index. The
/// second part still can start with whitespace characters when the string starts with more of them than `index`.
pub(super) fn split_indent_at(this: &str, index: usize) -> (&str, &str) {
    let first_non_whitespace_index = this.chars().position(|c| !is_kotlin_whitespace(c)).unwrap_or(usize::MAX);
    let safe_index = first_non_whitespace_index.min(index);
    let byte_index = this.char_indices().nth(safe_index).map_or(this.len(), |(i, _)| i);
    this.split_at(byte_index)
}

fn is_indent_before_closing_quote(ast: &Ast, this: NodeId) -> bool {
    is_kotlin_blank(&ast.text(this)) && ast.next_code_sibling(this).map(|it| ast.element_type(it)) == Some(CLOSING_QUOTE)
}
