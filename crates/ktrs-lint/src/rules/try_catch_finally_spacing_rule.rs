//! Port of ktlint-ruleset-standard `TryCatchFinallySpacingRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{BLOCK, CATCH, FINALLY, LBRACE, RBRACE, TRY};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{INDENT_SIZE_PROPERTY, INDENT_STYLE_PROPERTY, PropertyRef};
use crate::indent_config::IndentConfig;
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2};
use crate::rules::STANDARD_RULE_ABOUT;
use crate::token_sets::COMMENTS;

const TRY_CATCH_FINALLY_TOKEN_SET: TokenSet = TokenSet::create(&[TRY, CATCH, FINALLY]);
// A part-of-comment node whose parent is TRY/CATCH/FINALLY is itself a COMMENTS node.
const VISITED_TYPES: TokenSet = TokenSet::or_set(&[COMMENTS, TokenSet::create(&[BLOCK, CATCH, FINALLY])]);

/// Checks spacing and wrapping of try-catch-finally.
pub struct TryCatchFinallySpacingRule {
    indent_config: IndentConfig,
}

impl TryCatchFinallySpacingRule {
    pub fn new() -> TryCatchFinallySpacingRule {
        TryCatchFinallySpacingRule { indent_config: IndentConfig::default_indent_config() }
    }
}

impl Default for TryCatchFinallySpacingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for TryCatchFinallySpacingRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:try-catch-finally-spacing")
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

    fn is_official_code_style(&self) -> bool {
        true
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.indent_config = IndentConfig::new(editor_config.get(&INDENT_STYLE_PROPERTY), editor_config.get(&INDENT_SIZE_PROPERTY));
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.is_part_of_comment(node) && ast.parent(node).is_some_and(|p| TRY_CATCH_FINALLY_TOKEN_SET.contains(ast.element_type(p))) {
            emit(ast, ast.start_offset(node), "No comment expected at this location", false);
            return;
        }
        match ast.element_type(node) {
            BLOCK => visit_block(&self.indent_config, ast, node, emit),
            CATCH | FINALLY => visit_clause(ast, node, emit),
            _ => {}
        }
    }
}

fn visit_block(indent_config: &IndentConfig, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    if !ast.parent(node).is_some_and(|p| TRY_CATCH_FINALLY_TOKEN_SET.contains(ast.element_type(p))) {
        return;
    }

    let lbrace = ast.find_child_by_type(node, LBRACE).expect("NullPointerException: findChildByType(LBRACE)!!");
    let next_sibling = ast.next_sibling_matching(lbrace, |it| !ast.is_part_of_comment(it)).expect("NullPointerException: nextSibling!!");
    if !ast.text(next_sibling).starts_with('\n') {
        emit(ast, ast.start_offset(lbrace) + 1, "Expected a newline after '{'", true).if_autocorrect_allowed(|| {
            let indent = indent_config.sibling_indent_of(ast, node);
            ast.upsert_whitespace_after_me(lbrace, &indent);
        });
    }

    let rbrace = ast.find_child_by_type(node, RBRACE).expect("NullPointerException: findChildByType(RBRACE)!!");
    let prev_sibling = ast.prev_sibling_matching(rbrace, |it| !ast.is_part_of_comment(it)).expect("NullPointerException: prevSibling!!");
    if !ast.text(prev_sibling).starts_with('\n') {
        emit(ast, ast.start_offset(rbrace), "Expected a newline before '}'", true).if_autocorrect_allowed(|| {
            let indent = indent_config.parent_indent_of(ast, node);
            ast.upsert_whitespace_before_me(rbrace, &indent);
        });
    }
}

fn visit_clause(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    let prev_leaf = ast.prev_leaf_matching(node, |it| !ast.is_part_of_comment(it)).expect("NullPointerException: prevLeaf!!");
    if !ast.text_matches(prev_leaf, " ") {
        let message = format!("A single space is required before '{}'", element_type_name(ast, node));
        emit(ast, ast.start_offset(node), &message, true).if_autocorrect_allowed(|| ast.upsert_whitespace_before_me(node, " "));
    }
}

fn element_type_name(ast: &Ast, node: NodeId) -> String {
    ast.element_type(node).debug_name().to_lowercase()
}
