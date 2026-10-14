//! Port of ktlint-ruleset-standard `NoMultipleSpacesRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{KDOC_MARKDOWN_LINK, KDOC_TAG, WHITE_SPACE};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[WHITE_SPACE]);

pub struct NoMultipleSpacesRule;

impl RuleV2 for NoMultipleSpacesRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-multi-spaces")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if !ast.is_white_space(node) || ast.text_length(node) <= 1 {
            return;
        }
        if is_possible_alignment_of_kdoc_tag(ast, node) {
            return;
        }
        let text = ast.leaf_text(node);
        let before_indentation_len = remove_indentation(text).len();
        if before_indentation_len > 1 {
            emit(ast, ast.start_offset(node) + 1, "Unnecessary long whitespace", true).if_autocorrect_allowed(|| {
                let remainder = ast.leaf_text(node)[before_indentation_len..].to_owned();
                ast.replace_text_with(node, &format!(" {remainder}"));
            });
        }
    }
}

fn remove_indentation(text: &str) -> &str {
    text.split_once('\n').map_or(text, |(before, _)| before)
}

// allow multiple spaces in KDoc in case of KDOC_TAG for alignment, e.g.
// @param foo      stuff
// @param foobar   stuff2
fn is_possible_alignment_of_kdoc_tag(ast: &Ast, node: NodeId) -> bool {
    ast.prev_sibling(node).map(|it| ast.element_type(it)) == Some(KDOC_MARKDOWN_LINK)
        && ast.parent(node).map(|it| ast.element_type(it)) == Some(KDOC_TAG)
}
