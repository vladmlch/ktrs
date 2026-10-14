//! `StringTemplateIndenter` (private class in `IndentationRule.kt`): the closing quotes of a multiline raw string.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    CALL_EXPRESSION, CLOSING_QUOTE, DOT, EQ, FUN, LITERAL_STRING_TEMPLATE_ENTRY, REGULAR_STRING_PART, RETURN_KEYWORD,
    STRING_TEMPLATE,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::{AstNodeExtension, AstNodeLines};
use crate::editorconfig::CodeStyleValue;
use crate::engine::kotlin_text::{is_kotlin_blank, is_kotlin_whitespace};
use crate::indent_config::IndentConfig;
use crate::rule::Emit;

pub(super) struct StringTemplateIndenter {
    code_style: CodeStyleValue,
    indent_config: IndentConfig,
}

impl StringTemplateIndenter {
    pub(super) fn new(code_style: CodeStyleValue, indent_config: IndentConfig) -> StringTemplateIndenter {
        StringTemplateIndenter { code_style, indent_config }
    }

    pub(super) fn visit_closing_quotes(&self, expected_indent: &str, ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>) {
        assert!(ast.element_type(node) == STRING_TEMPLATE, "IllegalArgumentException: Failed requirement.");
        if !(is_followed_by_trim_indent(ast, node) || is_followed_by_trim_margin(ast, node)) || !is_multi_line(ast, node) {
            return;
        }
        if contains_mixed_indentation_characters(ast, node) {
            // It can not be determined with certainty how mixed indentation characters should be interpreted.
            // The trimIndent function handles tabs and spaces equally (one tabs equals one space) while the user
            // might expect that the tab size in the indentation is more than one space.
            emit_and_approve(ast, ast.start_offset(node), "Indentation of multiline string should not contain both tab(s) and space(s)", false);
            return;
        }

        let prev_leaf = ast.prev_leaf(node);
        let official = self.code_style == CodeStyleValue::KtlintOfficial;
        let corrected_expected_indent = if official && is_raw_string_literal_return_in_function_body_block(ast, node) {
            // Allow:
            //   fun foo(): String {
            //       return """
            //           some text
            //           """.trimIndent
            //   }
            ast.indent_without_newline_prefix(node) + &self.indent_config.indent
        } else if official && is_raw_string_literal_function_body_expression(ast, node) {
            // Allow:
            //   fun foo(
            //       bar: String
            //   ) = """
            //       $bar
            //       """.trimIndent
            ast.indent_without_newline_prefix(node) + &self.indent_config.indent
        } else if prev_leaf.is_some_and(|it| ast.text_matches(it, "\n")) {
            // In case the opening quotes are placed at the start of the line, then the closing quotes
            // should have no indent as well.
            String::new()
        } else {
            expected_indent.to_owned()
        };
        let children: Vec<NodeId> = ast.children(node).filter(|&it| is_indent_before_closing_quote(ast, it)).collect();
        for it in children {
            if ast.prev_leaf(it).is_some_and(|p| ast.text_matches(p, "\n")) {
                let text = ast.text(it);
                let (actual_indent, actual_content) = split_indent_at(&text, usize::MAX);
                if actual_indent != corrected_expected_indent {
                    let actual_content = actual_content.to_owned();
                    // It is a deliberate choice not to fix the indents inside the string literal except the line which only
                    // contains the closing quotes. See 'string-template-indent` rule for fixing the content of the string
                    // template itself
                    emit_and_approve(ast, ast.start_offset(it), "Unexpected indent of multiline string closing quotes", true)
                        .if_autocorrect_allowed(|| match ast.first_child_node(it) {
                            None => {
                                let leaf = ast.new_leaf(REGULAR_STRING_PART, &corrected_expected_indent);
                                ast.raw_insert_before_me(it, leaf);
                            }
                            Some(first_child_node) => {
                                ast.replace_text_with(first_child_node, &format!("{corrected_expected_indent}{actual_content}"));
                            }
                        });
                }
            }
        }
    }
}

fn is_raw_string_literal_function_body_expression(ast: &Ast, this: NodeId) -> bool {
    let prev_leaf = ast.prev_leaf(this);
    (!ast.is_white_space(prev_leaf) || prev_leaf.is_some_and(|it| ast.text_matches(it, " ")))
        && ast
            .prev_code_leaf(this)
            .filter(|&it| ast.element_type(it) == EQ)
            .and_then(|it| ast.parent(it))
            .map(|it| ast.element_type(it))
            == Some(FUN)
}

fn is_raw_string_literal_return_in_function_body_block(ast: &Ast, this: NodeId) -> bool {
    ast.prev_code_leaf(this).map(|it| ast.element_type(it)) == Some(RETURN_KEYWORD)
}

fn is_followed_by_trim_indent(ast: &Ast, this: NodeId) -> bool {
    is_followed_by(ast, this, "trimIndent()")
}

fn is_followed_by_trim_margin(ast: &Ast, this: NodeId) -> bool {
    is_followed_by(ast, this, "trimMargin()")
}

fn is_followed_by(ast: &Ast, this: NodeId, call_expression_name: &str) -> bool {
    assert!(ast.element_type(this) == STRING_TEMPLATE, "IllegalArgumentException: Failed requirement.");
    ast.next_sibling_matching(this, |it| ast.element_type(it) != DOT)
        .is_some_and(|it| ast.element_type(it) == CALL_EXPRESSION && ast.text_matches(it, call_expression_name))
}

fn is_multi_line(ast: &Ast, this: NodeId) -> bool {
    ast.children(this).any(|it| ast.element_type(it) == LITERAL_STRING_TEMPLATE_ENTRY && ast.text_matches(it, "\n"))
}

fn contains_mixed_indentation_characters(ast: &Ast, this: NodeId) -> bool {
    let text = ast.text(this);
    let non_blank_lines: Vec<&str> = text
        .split('\n')
        .filter(|it| !it.starts_with("\"\"\""))
        .filter(|it| !it.ends_with("\"\"\""))
        .filter(|it| !is_kotlin_blank(it))
        .collect();
    let prefix_length = non_blank_lines.iter().map(|it| indent_length(it)).min().unwrap_or(0);
    let mut distinct_indent_characters: Vec<char> = Vec::new();
    for line in &non_blank_lines {
        for c in split_indent_at(line, prefix_length).0.chars() {
            if !distinct_indent_characters.contains(&c) {
                distinct_indent_characters.push(c);
            }
        }
    }
    distinct_indent_characters.len() > 1
}

fn is_indent_before_closing_quote(ast: &Ast, this: NodeId) -> bool {
    ast.element_type(this) == CLOSING_QUOTE
        || (is_kotlin_blank(&ast.text(this)) && ast.next_code_sibling(this).map(|it| ast.element_type(it)) == Some(CLOSING_QUOTE))
}

/// In chars: the indent is whitespace, so chars and UTF-16 units coincide for it.
fn indent_length(this: &str) -> usize {
    this.chars().position(|c| !is_kotlin_whitespace(c)).unwrap_or_else(|| this.encode_utf16().count())
}

/// Splits the string at the given index (in chars) or at the first non-white space character before that index. The
/// second part still can start with whitespace characters when the string starts with more of them than `index`.
fn split_indent_at(this: &str, index: usize) -> (&str, &str) {
    if this == "\n" {
        return ("", "");
    }
    let first_non_whitespace_index = this.chars().position(|c| !is_kotlin_whitespace(c)).unwrap_or(usize::MAX);
    let safe_index = first_non_whitespace_index.min(index);
    let byte_index = this.char_indices().nth(safe_index).map_or(this.len(), |(i, _)| i);
    this.split_at(byte_index)
}
