//! Port of ktlint-ruleset-standard `NoConsecutiveBlankLinesRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{CLASS, IDENTIFIER, PRIMARY_CONSTRUCTOR, WHITE_SPACE};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[WHITE_SPACE]);

pub struct NoConsecutiveBlankLinesRule;

impl RuleV2 for NoConsecutiveBlankLinesRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-consecutive-blank-lines")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.is_white_space(node) && ast.prev_sibling(node).is_some() {
            let new_line_count = ast.newline_count(node) as usize;
            if new_line_count < 2 {
                return;
            }
            let eof = ast.next_leaf(node).is_none();
            let between_class_and_primary_constructor = is_between_class_and_primary_constructor(ast, node);
            if new_line_count > 2 || eof || between_class_and_primary_constructor {
                let (first_line_len, second_line_len) = {
                    let text = ast.leaf_text(node);
                    let first_nl = text.find('\n').unwrap_or(0);
                    let rest = &text[(first_nl + 1).min(text.len())..];
                    let second_nl = rest.find('\n').unwrap_or(0);
                    (first_nl, second_nl)
                };
                let offset = ast.start_offset(node) + first_line_len + second_line_len + if between_class_and_primary_constructor { 1 } else { 2 };
                emit(ast, offset, "Needless blank line(s)", true).if_autocorrect_allowed(|| {
                    let new_text = {
                        let text = ast.leaf_text(node);
                        let first_nl = text.find('\n').unwrap_or(0);
                        let last_nl = text.rfind('\n').unwrap_or(0);
                        let first_line = &text[..first_nl];
                        let last_line = &text[(last_nl + 1).min(text.len())..];
                        let mut s = String::with_capacity(first_line.len() + 2 + last_line.len());
                        s.push_str(first_line);
                        s.push('\n');
                        if !eof && !between_class_and_primary_constructor {
                            s.push('\n');
                        }
                        s.push_str(last_line);
                        s
                    };
                    ast.replace_text_with(node, &new_text);
                });
            }
        }
    }
}

fn is_between_class_and_primary_constructor(ast: &Ast, node: NodeId) -> bool {
    ast.prev_code_leaf(node).is_some_and(|prev_node| {
        ast.element_type(prev_node) == IDENTIFIER
            && ast.parent(prev_node).map(|it| ast.element_type(it)) == Some(CLASS)
            && ast.next_sibling(node).map(|it| ast.element_type(it)) == Some(PRIMARY_CONSTRUCTOR)
    })
}
