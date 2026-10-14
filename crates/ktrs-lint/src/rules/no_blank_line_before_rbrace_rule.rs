//! Port of ktlint-ruleset-standard `NoBlankLineBeforeRbraceRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{RBRACE, WHITE_SPACE};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[WHITE_SPACE]);

pub struct NoBlankLineBeforeRbraceRule;

impl RuleV2 for NoBlankLineBeforeRbraceRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-blank-line-before-rbrace")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.newline_count(node) > 1 && ast.next_leaf(node).map(|it| ast.element_type(it)) == Some(RBRACE) {
            let (first_line_len, second_line_len) = {
                let text = ast.leaf_text(node);
                let first_nl = text.find('\n').unwrap_or(0);
                let rest = &text[(first_nl + 1).min(text.len())..];
                let second_nl = rest.find('\n').unwrap_or(0);
                (first_nl, second_nl)
            };
            let offset = ast.start_offset(node) + first_line_len + second_line_len + 1;
            emit(ast, offset, "Unexpected blank line(s) before \"}\"", true).if_autocorrect_allowed(|| {
                let new_text = {
                    let text = ast.leaf_text(node);
                    let first_nl = text.find('\n').unwrap_or(0);
                    let last_nl = text.rfind('\n').unwrap_or(0);
                    format!("{}\n{}", &text[..first_nl], &text[(last_nl + 1).min(text.len())..])
                };
                ast.replace_text_with(node, &new_text);
            });
        }
    }
}
