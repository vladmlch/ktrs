//! Port of ktlint-ruleset-standard `ChainWrappingRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::kt_tokens::OPERATIONS;
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{
    ANDAND, COMMA, DIV, DOT, ELSE_KEYWORD, ELVIS, LBRACE, LPAR, MINUS, MUL, OPERATION_REFERENCE, OROR, PERC, PLUS,
    PREFIX_EXPRESSION, SAFE_ACCESS,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{INDENT_SIZE_PROPERTY, INDENT_STYLE_PROPERTY, PropertyRef};
use crate::indent_config::IndentConfig;
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2};
use crate::rules::STANDARD_RULE_ABOUT;

const SAME_LINE_TOKENS: TokenSet = TokenSet::create(&[MUL, DIV, PERC, ANDAND, OROR]);
const PREFIX_TOKENS: TokenSet = TokenSet::create(&[PLUS, MINUS]);
const NEXT_LINE_TOKENS: TokenSet = TokenSet::create(&[DOT, SAFE_ACCESS, ELVIS]);
const VISITED_TYPES: TokenSet = TokenSet::or_set(&[NEXT_LINE_TOKENS, SAME_LINE_TOKENS, PREFIX_TOKENS]);

pub struct ChainWrappingRule {
    indent_config: IndentConfig,
}

impl ChainWrappingRule {
    pub fn new() -> ChainWrappingRule {
        ChainWrappingRule { indent_config: IndentConfig::default_indent_config() }
    }
}

impl Default for ChainWrappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for ChainWrappingRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:chain-wrapping")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![PropertyRef::from(&*INDENT_SIZE_PROPERTY), PropertyRef::from(&*INDENT_STYLE_PROPERTY)]
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.indent_config = IndentConfig::new(editor_config.get(&INDENT_STYLE_PROPERTY), editor_config.get(&INDENT_SIZE_PROPERTY));
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let element_type = ast.element_type(node);
        if NEXT_LINE_TOKENS.contains(element_type) {
            if ast.is_part_of_comment(node) {
                return;
            }
            let next_leaf = ast.next_code_leaf(node).and_then(|it| ast.prev_leaf(it));
            if ast.is_white_space_with_newline(next_leaf) && !is_elvis_operator_and_comment(ast, node) {
                let message = format!("Line must not end with \"{}\"", ast.leaf_text(node));
                let indent_config = &self.indent_config;
                emit(ast, ast.start_offset(node), &message, true).if_autocorrect_allowed(|| {
                    // <prevLeaf><node="."><nextLeaf="\n"> becomes <prevLeaf><nextLeaf="\n"><node=".">
                    if ast.element_type(node) == ELVIS {
                        let indent = indent_config.child_indent_of(ast, node);
                        ast.upsert_whitespace_before_me(node, &indent);
                        ast.upsert_whitespace_after_me(node, " ");
                    } else {
                        ast.remove(node);
                        ast.raw_insert_after_me(next_leaf.unwrap(), node);
                    }
                });
            }
        } else if SAME_LINE_TOKENS.contains(element_type) || PREFIX_TOKENS.contains(element_type) {
            if ast.is_part_of_comment(node) {
                return;
            }
            let prev_leaf = ast.prev_leaf(node);
            if is_part_of_spread(ast, node) {
                // Allow `fn(\n *typedArray<...>()\n)`
                return;
            }
            if PREFIX_TOKENS.contains(element_type) && is_in_prefix_position(ast, node) {
                // Allow `fn(\n -42\n)`
                return;
            }

            if let Some(prev_leaf) = prev_leaf.filter(|&it| ast.is_white_space_with_newline(it)) {
                let message = format!("Line must not begin with \"{}\"", ast.leaf_text(node));
                emit(ast, ast.start_offset(node), &message, true).if_autocorrect_allowed(|| {
                    // <insertionPoint><prevLeaf="\n"><node="&&"><nextLeaf=" "> becomes
                    // <insertionPoint><space><node="&&"><prevLeaf="\n">
                    let next_leaf = ast.next_leaf(node);
                    let white_space_to_be_deleted = if ast.is_white_space_with_newline(next_leaf) {
                        // Removing the whitespace before the node keeps the indent of the next line
                        Some(prev_leaf)
                    } else if ast.is_white_space_without_newline(next_leaf) {
                        next_leaf
                    } else {
                        None
                    };

                    let parent = ast.parent(node);
                    if let Some(parent) = parent.filter(|&p| ast.element_type(p) == OPERATION_REFERENCE) {
                        let insert_before_sibling = ast.prev_code_sibling(parent).and_then(|it| ast.next_sibling(it));
                        ast.remove(parent);
                        if let Some(insert_before_sibling) = insert_before_sibling
                            && let Some(sibling_parent) = ast.parent(insert_before_sibling)
                        {
                            ast.add_child(sibling_parent, parent, Some(insert_before_sibling));
                        }
                        ast.upsert_whitespace_before_me(parent, " ");
                    } else {
                        let insertion_point = ast
                            .prev_code_leaf(prev_leaf)
                            .expect("NullPointerException: null cannot be cast to non-null type LeafPsiElement");
                        ast.remove(node);
                        ast.raw_insert_after_me(insertion_point, node);
                        ast.upsert_whitespace_after_me(insertion_point, " ");
                    }
                    if let Some(white_space) = white_space_to_be_deleted {
                        ast.remove(white_space);
                    }
                });
            }
        }
    }
}

fn is_part_of_spread(ast: &Ast, node: NodeId) -> bool {
    ast.element_type(node) == MUL
        && ast.prev_code_leaf(node).is_some_and(|leaf| {
            let type_ = ast.element_type(leaf);
            type_ == LPAR || type_ == COMMA || type_ == LBRACE || type_ == ELSE_KEYWORD || OPERATIONS.contains(type_)
        })
}

fn is_in_prefix_position(ast: &Ast, node: NodeId) -> bool {
    ast.parent(node).and_then(|p| ast.parent(p)).map(|pp| ast.element_type(pp)) == Some(PREFIX_EXPRESSION)
}

fn is_elvis_operator_and_comment(ast: &Ast, node: NodeId) -> bool {
    ast.element_type(node) == ELVIS
        && ast.leaves(node, true).next().is_some_and(|it| ast.is_white_space_without_newline(it) || ast.is_part_of_comment(it))
}
