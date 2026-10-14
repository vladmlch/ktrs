//! Port of ktlint-ruleset-standard `LambdaReturnRule.kt`. Upstream reads the indent and max line length in
//! `beforeFirstNode` but never uses them; only the property declarations are kept.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    BLOCK, CALL_EXPRESSION, FUNCTION_LITERAL, IDENTIFIER, LABEL, LABEL_QUALIFIER, LABELED_EXPRESSION, LAMBDA_ARGUMENT,
    LAMBDA_EXPRESSION, REFERENCE_EXPRESSION, RETURN,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{CODE_STYLE_PROPERTY, INDENT_SIZE_PROPERTY, INDENT_STYLE_PROPERTY, MAX_LINE_LENGTH_PROPERTY, PropertyRef};
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[FUNCTION_LITERAL]);

/// Do not use a labeled return for the last statement in a lambda
/// (<https://kotlinlang.org/docs/coding-conventions.html#returns-in-a-lambda>).
pub struct LambdaReturnRule;

impl RuleV2 for LambdaReturnRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:lambda-return")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![
            PropertyRef::from(&*CODE_STYLE_PROPERTY),
            PropertyRef::from(&*INDENT_SIZE_PROPERTY),
            PropertyRef::from(&*INDENT_STYLE_PROPERTY),
            PropertyRef::from(&*MAX_LINE_LENGTH_PROPERTY),
        ]
    }

    fn is_experimental(&self) -> bool {
        true
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let last_return = Some(node)
            .filter(|&it| ast.element_type(it) == FUNCTION_LITERAL)
            .and_then(|it| ast.find_child_by_type(it, BLOCK))
            .and_then(|block| ast.children(block).filter(|&it| ast.element_type(it) == RETURN).last())
            .filter(|&it| ast.next_code_sibling(it).is_none());
        if let Some(last_return) = last_return {
            visit_return_statement(ast, last_return, emit);
        }
    }
}

fn visit_return_statement(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    assert!(ast.element_type(node) == RETURN, "IllegalArgumentException: Failed requirement.");

    let Some(label_qualifier) = ast.find_child_by_type(node, LABEL_QUALIFIER) else { return };
    let siblings: Vec<NodeId> = ast.siblings(label_qualifier, true).skip_while(|&it| ast.is_white_space(it)).collect();
    if siblings.is_empty() || !refers_to_inner_most_lambda_argument(ast, node, qualifier_identifier(ast, Some(label_qualifier))) {
        return;
    }
    emit(ast, ast.start_offset(node), "Unnecessary return", true).if_autocorrect_allowed(|| {
        let parent = ast.parent(node).expect("NullPointerException: parent!!");
        // Collected first: moving while walking would also try to move the RETURN itself.
        for sibling in siblings {
            ast.add_child(parent, sibling, Some(node));
        }
        ast.remove(node);
    });
}

fn refers_to_inner_most_lambda_argument<'a>(ast: &'a Ast, node: NodeId, qualifier_identifier: Option<&'a str>) -> bool {
    qualifier_identifier == labeled_lambda_argument_identifier(ast, node).or_else(|| unlabeled_lambda_argument_identifier(ast, node))
}

/// `foo.let someLabel@{ ... }` -> `someLabel`.
fn labeled_lambda_argument_identifier(ast: &Ast, node: NodeId) -> Option<&str> {
    let lambda = ast.parent_matching(node, |it| ast.element_type(it) == LAMBDA_EXPRESSION)?;
    if ast.tree_parent(lambda).map(|p| ast.element_type(p)) != Some(LABELED_EXPRESSION) {
        return None;
    }
    qualifier_identifier(ast, ast.prev_code_sibling(lambda))
}

/// `foo.let { ... }` -> `let`.
fn unlabeled_lambda_argument_identifier(ast: &Ast, node: NodeId) -> Option<&str> {
    let lambda = ast.parent_matching(node, |it| ast.element_type(it) == LAMBDA_EXPRESSION)?;
    if ast.tree_parent(lambda).map(|p| ast.element_type(p)) == Some(LABELED_EXPRESSION) {
        return None;
    }
    let lambda_argument = ast.parent_matching(lambda, |it| ast.element_type(it) == LAMBDA_ARGUMENT)?;
    let call_expression = ast.parent_matching(lambda_argument, |it| ast.element_type(it) == CALL_EXPRESSION)?;
    let reference_expression = ast.find_child_by_type(call_expression, REFERENCE_EXPRESSION)?;
    ast.find_child_by_type(reference_expression, IDENTIFIER).map(|it| ast.leaf_text(it))
}

fn qualifier_identifier(ast: &Ast, node: Option<NodeId>) -> Option<&str> {
    let label = node.filter(|&it| ast.element_type(it) == LABEL_QUALIFIER).and_then(|it| ast.find_child_by_type(it, LABEL))?;
    ast.find_child_by_type(label, IDENTIFIER).map(|it| ast.leaf_text(it))
}
