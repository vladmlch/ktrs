//! Port of ktlint-ruleset-standard `NoTrailingSpacesRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::kt_tokens::COMMENTS;
use ktrs_syntax::SyntaxKind::{EOL_COMMENT, KDOC_END, KDOC_LEADING_ASTERISK, KDOC_TEXT, WHITE_SPACE};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::element_type::KDOC;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

// Non-code outside a KDoc is whitespace or a (leaf) comment token.
const VISITED_TYPES: TokenSet = TokenSet::or_set(&[COMMENTS, TokenSet::create(&[WHITE_SPACE, KDOC_TEXT])]);

pub struct NoTrailingSpacesRule;

impl RuleV2 for NoTrailingSpacesRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-trailing-spaces")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if is_part_of_kdoc(ast, node) {
            if ast.is_white_space(node) && has_trailing_spaces_before_newline(ast, node) {
                let text = ast.text(node);
                let offset_of_space_before_newline_in_text = text.find(" \n").expect("indexOf(\" \\n\")");
                let offset_of_first_space_before_newline_in_text =
                    text[..offset_of_space_before_newline_in_text].trim_end_matches(' ').len();
                emit(ast, ast.start_offset(node) + offset_of_first_space_before_newline_in_text, "Trailing space(s)", true)
                    .if_autocorrect_allowed(|| remove_trailing_spaces_before_newline(ast, node));
            }

            if ast.element_type(node) == KDOC_TEXT
                && ast.leaf_text(node).ends_with(' ')
                && ast
                    .next_leaf(node)
                    .filter(|&it| {
                        ast.next_leaf(it).map(|n| ast.element_type(n)) == Some(KDOC_LEADING_ASTERISK)
                            || ast.next_leaf(it).map(|n| ast.element_type(n)) == Some(KDOC_END)
                    })
                    .is_some_and(|it| ast.leaf_text(it).starts_with('\n'))
            {
                let trimmed_text = ast.leaf_text(node).trim_end().to_owned();
                emit(ast, ast.start_offset(node) + trimmed_text.len(), "Trailing space(s)", true)
                    .if_autocorrect_allowed(|| ast.replace_text_with(node, &trimmed_text));
            }
        } else if !ast.is_code(node) {
            let text = ast.text(node);
            let lines: Vec<&str> = text.split('\n').collect();
            let mut autocorrect = false;
            let mut violation_offset = ast.start_offset(node);

            let mut modified_lines = Vec::with_capacity(lines.len());
            for (index, &line) in lines.iter().enumerate() {
                let modified_line = if ast.element_type(node) != EOL_COMMENT && index == lines.len() - 1 && ast.next_leaf(node).is_some() {
                    // Do not change the last line as it contains the indentation of the next element except
                    // when it is an EOL comment which may also not contain trailing spaces
                    line
                } else if has_trailing_space(line) {
                    let modified_line = line.trim_end();
                    let first_trailing_space_offset = violation_offset + modified_line.len();
                    if emit(ast, first_trailing_space_offset, "Trailing space(s)", true).if_autocorrect_allowed(|| ()).is_some() {
                        autocorrect = true;
                    }
                    modified_line
                } else {
                    line
                };
                violation_offset += line.len() + 1;
                modified_lines.push(modified_line);
            }
            if autocorrect {
                ast.replace_text_with(node, &modified_lines.join("\n"));
            }
        }
    }
}

fn is_part_of_kdoc(ast: &Ast, node: NodeId) -> bool {
    ast.is_part_of(node, KDOC)
}

/// `SPACE_OR_TAB_BEFORE_NEWLINE_REGEX` is ` +\n`.
fn has_trailing_spaces_before_newline(ast: &Ast, node: NodeId) -> bool {
    ast.text(node).contains(" \n")
}

fn remove_trailing_spaces_before_newline(ast: &mut Ast, node: NodeId) {
    let text = ast.text(node);
    // ` +\n` -> `\n`: trim the spaces of every line but the last
    let mut out = String::with_capacity(text.len());
    let mut parts = text.split('\n').peekable();
    while let Some(line) = parts.next() {
        if parts.peek().is_some() {
            out.push_str(line.trim_end_matches(' '));
            out.push('\n');
        } else {
            out.push_str(line);
        }
    }
    ast.replace_text_with(node, &out);
}

fn has_trailing_space(line: &str) -> bool {
    line.ends_with(' ')
}
