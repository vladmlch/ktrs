//! Port of ktlint-ruleset-standard `NoBlankLineInListRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    self, CLASS_BODY, SUPER_TYPE_LIST, TYPE_ARGUMENT_LIST, TYPE_CONSTRAINT_LIST, TYPE_PARAMETER_LIST, VALUE_ARGUMENT_LIST,
    VALUE_PARAMETER_LIST, WHITE_SPACE,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[WHITE_SPACE]);

// The MODIFIER_LIST is handled by ModifierListSpacingRule.
const LIST_TYPES: [SyntaxKind; 6] =
    [SUPER_TYPE_LIST, TYPE_ARGUMENT_LIST, TYPE_CONSTRAINT_LIST, TYPE_PARAMETER_LIST, VALUE_ARGUMENT_LIST, VALUE_PARAMETER_LIST];

pub struct NoBlankLineInListRule;

impl RuleV2 for NoBlankLineInListRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-blank-line-in-list")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn is_official_code_style(&self) -> bool {
        true
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if !ast.is_white_space(node) || ast.newline_count(node) < 2 {
            return;
        }
        // Whether the whitespace before the first and after the last element is part of the list depends on the list type
        // (VALUE_ARGUMENT_LIST: inside; SUPER_TYPE_LIST: the leading one is a child of the CLASS), hence the sibling checks.
        if let Some(tree_parent_element_type) = ast.parent(node).map(|it| ast.element_type(it)).filter(|it| LIST_TYPES.contains(it)) {
            visit_white_space(ast, node, emit, tree_parent_element_type, false);
        }
        if let Some(tree_parent_element_type) = ast.next_sibling(node).map(|it| ast.element_type(it)).filter(|it| LIST_TYPES.contains(it)) {
            visit_white_space(ast, node, emit, tree_parent_element_type, tree_parent_element_type == TYPE_CONSTRAINT_LIST);
        }
        if let Some(tree_parent_element_type) = ast.prev_sibling(node).map(|it| ast.element_type(it)).filter(|it| LIST_TYPES.contains(it)) {
            let replace_with_singe_space = ast.next_sibling(node).map(|it| ast.element_type(it)) == Some(CLASS_BODY);
            visit_white_space(ast, node, emit, tree_parent_element_type, replace_with_singe_space);
        }
    }
}

fn visit_white_space(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>, part_of_element_type: SyntaxKind, replace_with_singe_space: bool) {
    if ast.leaf_text(node).matches('\n').count() < 2 {
        return;
    }
    let text = ast.leaf_text(node).to_owned();
    let lines: Vec<&str> = text.split('\n').collect();
    emit(ast, ast.start_offset(node) + 1, &format!("Unexpected blank line(s) in {}", element_type_description(part_of_element_type)), true)
        .if_autocorrect_allowed(|| {
            if replace_with_singe_space {
                ast.replace_text_with(node, " ");
            } else {
                ast.replace_text_with(node, &format!("{}\n{}", lines[0], lines[lines.len() - 1]));
            }
        });
}

fn element_type_description(element_type: SyntaxKind) -> String {
    element_type.debug_name().to_lowercase().replace('_', " ")
}
