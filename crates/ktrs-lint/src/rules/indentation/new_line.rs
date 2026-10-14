//! `IndentationRule.kt` from `visitNewLineIndentation` to `isPrecededByComment`: checking one newline whitespace.

use std::borrow::Cow;
use std::sync::Arc;

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::kt_tokens::COMMENTS;
use ktrs_syntax::SyntaxKind::{BLOCK_COMMENT, ELVIS, KDOC_END, KDOC_LEADING_ASTERISK, OPEN_QUOTE, OPERATION_REFERENCE, TYPE_CONSTRAINT};

use super::string_template_indenter::StringTemplateIndenter;
use super::{IndentContext, IndentationRule, KDOC_CONTINUATION_INDENT, TYPE_CONSTRAINT_CONTINUATION_INDENT};
use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{AutocorrectDecision, Emit, IndentStyle};

#[inline]
fn indent_matches(slice: &str, prefix: &str, suffix: &str) -> bool {
    slice.len() == prefix.len() + suffix.len()
        && slice.starts_with(prefix)
        && &slice[prefix.len()..] == suffix
}

impl IndentationRule {
    pub(super) fn visit_new_line_indentation(&mut self, ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>) {
        if ignore_indent(ast, node) {
            return;
        }

        let (expected_prefix, expected_suffix) = self.expected_indent_parts(ast, node);
        let expected_len = expected_prefix.len() + expected_suffix.len();
        let (auto_correct, before_last_newline) = {
            let normalized_node_indent = self.normalized_indent(ast, node, emit_and_approve);
            let text = ast.leaf_text(node);
            let node_indent = substring_after_last_newline(text);
            let is_normalized = node_indent == normalized_node_indent.as_ref();
            let matches_expected = indent_matches(normalized_node_indent.as_ref(), expected_prefix, expected_suffix);
            if !is_normalized || !matches_expected {
                let auto_correct = if !matches_expected {
                    let message =
                        format!("Unexpected indentation ({}) (should be {})", normalized_node_indent.len(), expected_len);
                    emit_and_approve(ast, ast.start_offset(node) + text.len() - node_indent.len(), &message, true)
                        == AutocorrectDecision::AllowAutocorrect
                } else {
                    // Indentation was at correct level but contained invalid indent characters. This violation has already
                    // been emitted.
                    true
                };
                if auto_correct {
                    (true, Some(text[..text.rfind('\n').unwrap_or(text.len())].to_owned()))
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            }
        };
        if let (true, Some(before)) = (auto_correct, before_last_newline) {
            ast.replace_text_with(node, &format!("{before}\n{expected_prefix}{expected_suffix}"));
        }
    }

    fn expected_indent_parts<'a>(&'a self, ast: &Ast, this: NodeId) -> (&'a str, &'a str) {
        let last_index_context = self.indent_context_stack.last().expect("NullPointerException: peekLast()");
        let next_leaf = ast.next_leaf(this);
        let from_first_leaf = ast.first_child_leaf_or_self(last_index_context.from_ast_node);
        let adjusted_child_indent = if this == from_first_leaf || next_leaf == Some(from_first_leaf) {
            &last_index_context.first_child_indent
        } else if this == last_index_context.to_ast_node || next_leaf == Some(last_index_context.to_ast_node) {
            &last_index_context.last_child_indent
        } else {
            &last_index_context.child_indent
        };
        (&last_index_context.node_indent, adjusted_child_indent)
    }

    fn normalized_indent<'a>(&self, ast: &'a Ast, this: NodeId, emit_and_approve: &mut Emit<'_>) -> Cow<'a, str> {
        let text = ast.leaf_text(this);
        let node_indent = substring_after_last_newline(text);
        let offset = ast.start_offset(this) + text.len() - node_indent.len();
        match self.indent_config.indent_style {
            IndentStyle::Space => {
                if node_indent.contains('\t') {
                    let normalized = emit_and_approve(ast, offset, "Unexpected tab character(s)", true)
                        // Ignore approval and fix invalid indent character always
                        .if_autocorrect_allowed(|| self.indent_config.to_normalized_indent(node_indent));
                    match normalized {
                        Some(s) => Cow::Owned(s),
                        None => Cow::Borrowed(node_indent),
                    }
                } else {
                    Cow::Borrowed(node_indent)
                }
            }
            IndentStyle::Tab => {
                let acceptable_trailing_spaces = acceptable_trailing_spaces(ast, this);
                let node_indent_without_acceptable_trailing_spaces =
                    node_indent.strip_suffix(acceptable_trailing_spaces).unwrap_or(node_indent);
                if node_indent_without_acceptable_trailing_spaces.contains(' ') {
                    let normalized = emit_and_approve(ast, offset, "Unexpected space character(s)", true)
                        .if_autocorrect_allowed(|| {
                            self.indent_config.to_normalized_indent(node_indent_without_acceptable_trailing_spaces) + acceptable_trailing_spaces
                        });
                    match normalized {
                        Some(s) => Cow::Owned(s),
                        None => Cow::Borrowed(node_indent),
                    }
                } else {
                    Cow::Borrowed(node_indent)
                }
            }
        }
    }

    pub(super) fn visit_white_space_before_closing_quote(&mut self, ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>) {
        if self.string_template_indenter.is_none() {
            self.string_template_indenter = Some(StringTemplateIndenter::new(self.code_style, self.indent_config.clone()));
        }
        let current_indent = self.current_indent();
        let parent = ast.parent(node).expect("NullPointerException: parent!!");
        self.string_template_indenter.as_ref().unwrap().visit_closing_quotes(&current_indent, ast, parent, emit_and_approve);
    }
}

fn substring_after_last_newline(text: &str) -> &str {
    text.rfind('\n').map_or(text, |i| &text[i + 1..])
}

fn ignore_indent(ast: &Ast, this: NodeId) -> bool {
    let next_leaf = ast.next_leaf(this);
    let text = ast.leaf_text(this);
    if text.ends_with('\n') && is_start_of_raw_string_literal(ast, next_leaf) {
        return true; // raw strings (""") are allowed at column 0
    }

    if let Some(comment) = next_leaf.filter(|&it| ast.is_part_of_set(it, COMMENTS)) {
        if text.ends_with('\n') {
            return true; // comments are allowed at column 0
        }
        if ast.text_contains(comment, '\n') && ast.element_type(comment) == BLOCK_COMMENT {
            // FIXME: while we cannot assume any kind of layout inside a block comment,
            // `/*` and `*/` can still be indented
            return true;
        }
    }
    false
}

fn is_start_of_raw_string_literal(ast: &Ast, this: Option<NodeId>) -> bool {
    this.is_some_and(|it| ast.element_type(it) == OPEN_QUOTE && ast.text_matches(it, "\"\"\""))
}

pub(super) fn is_elvis_operator(ast: &Ast, this: Option<NodeId>) -> bool {
    this.is_some_and(|it| {
        ast.element_type(it) == OPERATION_REFERENCE && ast.first_child_node(it).map(|c| ast.element_type(c)) == Some(ELVIS)
    })
}

fn acceptable_trailing_spaces(ast: &Ast, this: NodeId) -> &'static str {
    assert!(ast.is_white_space(this), "IllegalArgumentException: Failed requirement.");
    let acceptable_trailing_spaces = match ast.next_leaf(this).map(|it| ast.element_type(it)) {
        // The indentation of a KDoc comment contains a space as the last character regardless of the indentation
        // style (tabs or spaces) except for the starting line of the KDoc comment
        Some(KDOC_LEADING_ASTERISK | KDOC_END) => KDOC_CONTINUATION_INDENT,
        // 6 spaces (length of "where" keyword plus a separator space) to indent type constraints as below:
        //    where A1 : RecyclerView.Adapter<V1>,
        //          A1 : ComposableAdapter.ViewTypeProvider,
        Some(TYPE_CONSTRAINT) => TYPE_CONSTRAINT_CONTINUATION_INDENT,
        _ => "",
    };
    let node_indent = substring_after_last_newline(ast.leaf_text(this));
    if node_indent.ends_with(acceptable_trailing_spaces) { acceptable_trailing_spaces } else { "" }
}

pub(super) fn start_no_indent_zone(ast: &Ast, node: NodeId) -> IndentContext {
    let to_ast_node = ast.last_child_leaf_or_self(node);
    IndentContext {
        from_ast_node: node,
        to_ast_node,
        node_indent: Arc::from(""),
        first_child_indent: Arc::from(""),
        child_indent: Arc::from(""),
        last_child_indent: Arc::from(""),
        activated: true,
        node_types: IndentContext::node_types(ast, node, to_ast_node),
    }
}

pub(super) fn is_preceded_by_comment(ast: &Ast, this: NodeId) -> bool {
    ast.prev_sibling_matching(this, |it| !ast.is_white_space(it)).is_some_and(|it| ast.is_part_of_comment(it))
}
