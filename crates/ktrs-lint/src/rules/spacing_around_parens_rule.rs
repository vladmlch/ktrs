//! Port of ktlint-ruleset-standard `SpacingAroundParensRule.kt` (id `paren-spacing`).

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{
    BLOCK_COMMENT, CONSTRUCTOR_CALLEE, EOL_COMMENT, FUNCTION_TYPE, IDENTIFIER, KDOC_START, LPAR, PRIMARY_CONSTRUCTOR, RPAR,
    SUPER_KEYWORD, SUPER_TYPE_CALL_ENTRY, VALUE_ARGUMENT_LIST, VALUE_PARAMETER_LIST,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{EditorConfig, KtlintVersion};
use crate::rule::{About, Emit, RuleId, RuleV2};
use crate::rules::STANDARD_RULE_ABOUT;

const ELEMENT_LIST_TOKEN_SET: TokenSet = TokenSet::create(&[VALUE_PARAMETER_LIST, VALUE_ARGUMENT_LIST]);
const COMMENT_TYPES: TokenSet = TokenSet::create(&[EOL_COMMENT, BLOCK_COMMENT, KDOC_START]);
const VISITED_TYPES: TokenSet = TokenSet::create(&[LPAR, RPAR]);

/// Ensures there are no extra spaces around parentheses.
///
/// See https://kotlinlang.org/docs/reference/coding-conventions.html#horizontal-whitespace
#[derive(Default)]
pub struct SpacingAroundParensRule {
    ktlint_version: KtlintVersion,
}

impl RuleV2 for SpacingAroundParensRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:paren-spacing")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.ktlint_version = KtlintVersion::of(editor_config);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.element_type(node) == LPAR || ast.element_type(node) == RPAR {
            let spacing_before = is_unexpected_spacing_before_parenthesis(ast, node, self.ktlint_version);
            let spacing_after = is_unexpected_spacing_after_parenthesis(ast, node);
            if spacing_before && spacing_after {
                fix_unexpected_spacing_around(ast, node, emit);
            } else if spacing_before {
                fix_unexpected_spacing_before(ast, node, emit);
            } else if spacing_after {
                fix_unexpect_spacing_after(ast, node, emit);
            }
        }
    }
}

fn is_unexpected_spacing_before_parenthesis(ast: &Ast, node: NodeId, ktlint_version: KtlintVersion) -> bool {
    let prev_leaf = ast.prev_leaf(node);
    if ast.is_white_space_with_newline(prev_leaf) && has_no_newline_after_lpar(ast, node, ktlint_version) {
        true
    } else if !ast.is_white_space_without_newline(prev_leaf) {
        false
    } else if ast.element_type(node) == LPAR {
        ast.parent(node).is_some_and(|p| ELEMENT_LIST_TOKEN_SET.contains(ast.element_type(p)))
            && (is_unexpected_spacing_between_identifier_and_element_list(ast, node)
                || is_unexpected_spacing_in_call_to_super(ast, node)
                || is_unexpected_spacing_in_explicit_constructor(ast, node)
                || is_unexpected_spacing_in_super_type_call_entry(ast, node))
    } else if ast.element_type(node) == RPAR {
        // Disallow:
        //    val foo = fn("foo" )
        //    val foo = fn( )
        prev_leaf.and_then(|it| ast.prev_sibling(it)).map(|it| ast.element_type(it)) != Some(LPAR)
    } else {
        false
    }
}

fn prev_white_space_leaf(ast: &Ast, node: NodeId) -> Option<NodeId> {
    ast.prev_leaf(node).filter(|&it| ast.is_white_space(it))
}

fn grand_parent_type(ast: &Ast, node: NodeId) -> Option<ktrs_syntax::SyntaxKind> {
    ast.parent(node).and_then(|p| ast.parent(p)).map(|pp| ast.element_type(pp))
}

fn is_unexpected_spacing_between_identifier_and_element_list(ast: &Ast, node: NodeId) -> bool {
    prev_white_space_leaf(ast, node)
        // Disallow `fun foo () {}` and `@Deprecated ("bar)`
        .filter(|&it| ast.prev_leaf(it).map(|l| ast.element_type(l)) == Some(IDENTIFIER))
        // But do allow `val foo: @Composable () -> Unit`
        .map(|_| grand_parent_type(ast, node) != Some(FUNCTION_TYPE))
        .unwrap_or(false)
}

fn is_unexpected_spacing_in_call_to_super(ast: &Ast, node: NodeId) -> bool {
    // Disallow `constructor(string: String) : super ()`
    prev_white_space_leaf(ast, node)
        .map(|it| ast.prev_leaf(it).map(|l| ast.element_type(l)) == Some(SUPER_KEYWORD))
        .unwrap_or(false)
}

fn is_unexpected_spacing_in_explicit_constructor(ast: &Ast, node: NodeId) -> bool {
    // Disallow `class Foo constructor ()`
    prev_white_space_leaf(ast, node)
        .map(|it| ast.prev_leaf(it).and_then(|l| ast.parent(l)).map(|p| ast.element_type(p)) == Some(PRIMARY_CONSTRUCTOR))
        .unwrap_or(false)
}

fn is_unexpected_spacing_in_super_type_call_entry(ast: &Ast, node: NodeId) -> bool {
    // Disallow `class Foo : Bar ("test")` and `class Foo : Bar<String> ("test")`
    prev_white_space_leaf(ast, node)
        .map(|it| {
            grand_parent_type(ast, node) == Some(SUPER_TYPE_CALL_ENTRY)
                && ast.prev_sibling(it).map(|s| ast.element_type(s)) == Some(CONSTRUCTOR_CALLEE)
        })
        .unwrap_or(false)
}

fn is_unexpected_spacing_after_parenthesis(ast: &Ast, node: NodeId) -> bool {
    if ast.element_type(node) == LPAR
        && ast.is_white_space_with_newline(ast.next_sibling(node))
        && has_no_other_newline_before_rpar(ast, node)
    {
        true
    } else if ast.element_type(node) == LPAR {
        ast.next_leaf(node)
            .filter(|&it| !is_next_leaf_a_comment(ast, it))
            .map(|it| is_unexpected_space_after_lpar(ast, it) || is_unexpected_newline_after_lpar(ast, it))
            .unwrap_or(false)
    } else {
        false
    }
}

fn is_unexpected_space_after_lpar(ast: &Ast, node: NodeId) -> bool {
    // Disallow `fn( )`, `fn( "bar")` and `( (1 + 2) / 3)`
    ast.is_white_space_without_newline(node)
}

fn is_unexpected_newline_after_lpar(ast: &Ast, node: NodeId) -> bool {
    // Disallow `fn(\n)`
    ast.is_white_space_with_newline(node) && ast.next_leaf(node).map(|it| ast.element_type(it)) == Some(RPAR)
}

fn has_no_other_newline_before_rpar(ast: &Ast, node: NodeId) -> bool {
    ast.next_sibling(node)
        .filter(|&it| ast.is_white_space_with_newline(it))
        .map(|it| {
            !ast.siblings(it, true)
                .take_while(|&s| ast.element_type(s) != RPAR)
                .any(|s| ast.is_white_space_with_newline(s))
        })
        .unwrap_or(false)
}

fn is_next_leaf_a_comment(ast: &Ast, node: NodeId) -> bool {
    ast.next_leaf(node).is_some_and(|it| COMMENT_TYPES.contains(ast.element_type(it)))
}

/// 1.8 does not stop at an EOL comment (#3236).
fn has_no_newline_after_lpar(ast: &Ast, node: NodeId, ktlint_version: KtlintVersion) -> bool {
    ast.prev_sibling(node)
        .filter(|&it| ast.is_white_space_with_newline(it))
        .filter(|&it| ast.prev_sibling(it).map(|s| ast.element_type(s)) != Some(LPAR))
        .map(|it| {
            !ast.siblings(it, false)
                .take_while(|&s| ast.element_type(s) != LPAR)
                .any(|s| ast.text_contains(s, '\n') || (!ktlint_version.is_1_8() && ast.element_type(s) == EOL_COMMENT))
        })
        .unwrap_or(false)
}

fn fix_unexpected_spacing_around(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    let message = format!("Unexpected spacing around \"{}\"", ast.leaf_text(node));
    emit(ast, ast.start_offset(node), &message, true).if_autocorrect_allowed(|| {
        let prev_leaf = ast.prev_leaf(node).expect("NullPointerException: prevLeaf!!");
        ast.remove(prev_leaf);
        let next_leaf = ast.next_leaf(node).expect("NullPointerException: nextLeaf!!");
        ast.remove(next_leaf);
    });
}

fn fix_unexpected_spacing_before(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    let message = format!("Unexpected spacing before \"{}\"", ast.leaf_text(node));
    let prev_leaf = ast.prev_leaf(node).expect("NullPointerException: prevLeaf!!");
    emit(ast, ast.start_offset(prev_leaf), &message, true).if_autocorrect_allowed(|| {
        if let Some(prev_leaf) = ast.prev_leaf(node) {
            ast.remove(prev_leaf);
        }
    });
}

fn fix_unexpect_spacing_after(ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
    let message = format!("Unexpected spacing after \"{}\"", ast.leaf_text(node));
    emit(ast, ast.start_offset(node) + 1, &message, true).if_autocorrect_allowed(|| {
        let next_leaf = ast.next_leaf(node).expect("NullPointerException: nextLeaf!!");
        ast.remove(next_leaf);
    });
}
