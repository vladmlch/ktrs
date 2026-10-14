//! `TrailingCommaOnDeclarationSiteRule.kt` from `reportAndCorrectTrailingCommaNodeBefore` to the end.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    ARROW, COLLECTION_LITERAL_EXPRESSION, COMMA, DESTRUCTURING_DECLARATION, DESTRUCTURING_DECLARATION_ENTRY, ENUM_ENTRY,
    FUNCTION_LITERAL, LBRACKET, LPAR, RBRACKET, RPAR, SEMICOLON, VALUE_ARGUMENT, VALUE_ARGUMENT_LIST, VALUE_PARAMETER,
    VALUE_PARAMETER_LIST, WHEN_ENTRY, WHEN_ENTRY_GUARD, WHITE_SPACE,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::{AstNodeExtension, AstNodeLines};
use crate::rule::Emit;

const FAILED_REQUIREMENT: &str = "IllegalArgumentException: Failed requirement.";

pub(super) fn report_and_correct_trailing_comma_node_before(
    ast: &mut Ast,
    this: NodeId,
    inspect_node: NodeId,
    is_trailing_comma_allowed: bool,
    emit: &mut Emit<'_>,
) {
    let prev_leaf = ast.prev_leaf(inspect_node);
    let trailing_comma_node = prev_leaf.and_then(|it| find_previous_trailing_comma_node_or_null(ast, it));
    let trailing_comma_state = if has_when_entry_guard(ast, this) {
        // The compiler won't allow any comma in the when-entry in case it contains a guard clause
        TrailingCommaState::NotExists
    } else if is_multiline(ast, this) {
        if trailing_comma_node.is_some() { TrailingCommaState::Exists } else { TrailingCommaState::Missing }
    } else if trailing_comma_node.is_some() {
        TrailingCommaState::Redundant
    } else {
        TrailingCommaState::NotExists
    };
    match trailing_comma_state {
        TrailingCommaState::Exists => {
            let trailing_comma_node = trailing_comma_node.unwrap();
            if is_trailing_comma_allowed {
                let last_node_before_arrow = ast
                    .parent(inspect_node)
                    .filter(|&it| ast.element_type(it) == WHEN_ENTRY)
                    .and_then(|it| ast.find_child_by_type(it, ARROW))
                    .and_then(|it| ast.prev_leaf(it));
                if let Some(last_node_before_arrow) = last_node_before_arrow
                    && !ast.is_white_space_with_newline(last_node_before_arrow)
                {
                    let message = format!("Expected a newline between the trailing comma and  \"{}\"", ast.leaf_text(inspect_node));
                    emit(ast, ast.start_offset(trailing_comma_node), &message, true).if_autocorrect_allowed(|| {
                        let indent = ast.indent(ast.parent(inspect_node).expect("NullPointerException: parent!!"));
                        ast.upsert_whitespace_after_me(last_node_before_arrow, &indent);
                    });
                }
            } else {
                let message = format!("Unnecessary trailing comma before \"{}\"", ast.leaf_text(inspect_node));
                emit(ast, ast.start_offset(trailing_comma_node), &message, true).if_autocorrect_allowed(|| ast.remove(trailing_comma_node));
            }
        }
        TrailingCommaState::Missing => {
            if is_trailing_comma_allowed {
                let leaf_before_arrow_or_null = leaf_before_arrow_or_null(ast, this);
                let add_new_line = !leaf_before_arrow_or_null.is_none_or(|it| ast.is_white_space_with_newline(it));
                let prev_node = ast.prev_code_leaf(inspect_node).expect("NullPointerException: prevCodeLeaf!!");
                let offset = ast.start_offset(prev_node) + ast.text_length(prev_node);
                let message = if add_new_line {
                    format!("Missing trailing comma and newline before \"{}\"", ast.leaf_text(inspect_node))
                } else {
                    format!("Missing trailing comma before \"{}\"", ast.leaf_text(inspect_node))
                };
                emit(ast, offset, &message, true).if_autocorrect_allowed(|| {
                    if add_new_line {
                        let indent = ast.indent(ast.parent(prev_node).expect("NullPointerException: parent!!"));
                        let leaf_before_arrow = leaf_before_arrow_or_null.unwrap();
                        if ast.is_white_space(leaf_before_arrow) {
                            ast.replace_text_with(leaf_before_arrow, &indent);
                        } else if let Some(before) = ast.prev_code_leaf(inspect_node).and_then(|it| ast.next_leaf(it))
                            && let Some(parent) = ast.parent(before)
                        {
                            let white_space = ast.new_leaf(WHITE_SPACE, &indent);
                            ast.add_child(parent, white_space, Some(before));
                        }
                    }

                    if ast.parent(inspect_node).map(|it| ast.element_type(it)) == Some(ENUM_ENTRY) {
                        let parent_indent = ast
                            .parent(prev_node)
                            .and_then(|it| ast.prev_leaf(it))
                            .filter(|&it| ast.is_white_space(it))
                            .map_or_else(|| ast.indent(prev_node), |it| ast.text(it));
                        if let Some(parent) = ast.parent(inspect_node) {
                            let comma = ast.new_leaf(COMMA, ",");
                            ast.add_child(parent, comma, Some(inspect_node));
                            let white_space = ast.new_leaf(WHITE_SPACE, &parent_indent);
                            ast.add_child(parent, white_space, None);
                            let semicolon = ast.new_leaf(SEMICOLON, ";");
                            ast.add_child(parent, semicolon, None);
                        }
                        ast.remove(inspect_node);
                    } else if let Some(before) = ast.prev_code_leaf(inspect_node).and_then(|it| ast.next_leaf(it))
                        && let Some(parent) = ast.parent(before)
                    {
                        let comma = ast.new_leaf(COMMA, ",");
                        ast.add_child(parent, comma, Some(before));
                    }
                });
            }
        }
        TrailingCommaState::Redundant => {
            let trailing_comma_node = trailing_comma_node.unwrap();
            let message = format!("Unnecessary trailing comma before \"{}\"", ast.leaf_text(inspect_node));
            emit(ast, ast.start_offset(trailing_comma_node), &message, true).if_autocorrect_allowed(|| ast.remove(trailing_comma_node));
        }
        TrailingCommaState::NotExists => {}
    }
}

fn is_multiline(ast: &Ast, n: NodeId) -> bool {
    let child = |t| ast.find_child_by_type(n, t).expect("NullPointerException: findChildByType!!");
    if ast.parent(n).map(|it| ast.element_type(it)) == Some(FUNCTION_LITERAL) {
        is_multiline(ast, ast.parent(n).unwrap())
    } else if ast.element_type(n) == FUNCTION_LITERAL {
        ast.has_new_line_in_closed_range(child(VALUE_PARAMETER_LIST), child(ARROW))
    } else if ast.element_type(n) == WHEN_ENTRY {
        ast.has_new_line_in_closed_range(ast.first_child_node(n).unwrap(), child(ARROW))
    } else if ast.element_type(n) == DESTRUCTURING_DECLARATION {
        ast.has_new_line_in_closed_range(
            // Get the LPAR or LBRACKET before the first entry
            opening_element_destructuring_declaration_entries(ast, n),
            // Get the RPAR or RBRACKET after the last entry
            closing_element_destructuring_declaration_entries(ast, n),
        )
    } else if ast.element_type(n) == VALUE_ARGUMENT_LIST
        && ast.children(n).filter(|&it| ast.element_type(it) == VALUE_ARGUMENT).count() == 1
        && ast.element_type(child(VALUE_ARGUMENT_LIST)) == COLLECTION_LITERAL_EXPRESSION
    {
        // special handling for collection literal
        // @Annotation([
        //    "something",
        // ])
        ast.has_new_line_in_closed_range(child(RBRACKET), child(RPAR))
    } else if ast.element_type(n) == VALUE_PARAMETER_LIST && ast.find_child_by_type(n, VALUE_PARAMETER).is_none() {
        false
    } else {
        ast.text_contains(n, '\n')
    }
}

fn opening_element_destructuring_declaration_entries(ast: &Ast, n: NodeId) -> NodeId {
    assert!(ast.element_type(n) == DESTRUCTURING_DECLARATION, "{FAILED_REQUIREMENT}");
    let entry = ast.find_child_by_type(n, DESTRUCTURING_DECLARATION_ENTRY).expect("NullPointerException: findChildByType!!");
    let it = ast.prev_code_sibling(entry).expect("NullPointerException: prevCodeSibling!!");
    assert!(matches!(ast.element_type(it), LPAR | LBRACKET), "{FAILED_REQUIREMENT}");
    it
}

pub(super) fn closing_element_destructuring_declaration_entries(ast: &Ast, n: NodeId) -> NodeId {
    assert!(ast.element_type(n) == DESTRUCTURING_DECLARATION, "{FAILED_REQUIREMENT}");
    let last_entry = ast
        .children(n)
        .filter(|&it| ast.element_type(it) == DESTRUCTURING_DECLARATION_ENTRY)
        .last()
        .expect(super::NO_MATCHING_ELEMENT);
    let it = ast
        .next_sibling_matching(last_entry, |it| ast.is_code(it) && ast.element_type(it) != COMMA)
        .expect("NullPointerException: nextSibling!!");
    assert!(matches!(ast.element_type(it), RPAR | RBRACKET), "{FAILED_REQUIREMENT}");
    it
}

fn leaf_before_arrow_or_null(ast: &Ast, n: NodeId) -> Option<NodeId> {
    Some(n)
        .filter(|&it| matches!(ast.element_type(it), WHEN_ENTRY | FUNCTION_LITERAL))
        .and_then(|it| ast.find_child_by_type(it, ARROW))
        .and_then(|it| ast.prev_leaf(it))
}

fn find_previous_trailing_comma_node_or_null(ast: &Ast, n: NodeId) -> Option<NodeId> {
    let code_leaf = if ast.is_code(n) { Some(n) } else { ast.prev_code_leaf(n) };
    code_leaf.filter(|&it| ast.element_type(it) == COMMA)
}

fn has_when_entry_guard(ast: &Ast, n: NodeId) -> bool {
    ast.element_type(n) == WHEN_ENTRY && has_when_entry_guard_kotlin21(ast, n)
}

fn has_when_entry_guard_kotlin21(ast: &Ast, n: NodeId) -> bool {
    ast.children(n).any(|it| ast.element_type(it) == WHEN_ENTRY_GUARD)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrailingCommaState {
    /// The trailing comma is needed and exists
    Exists,
    /// The trailing comma is needed and doesn't exist
    Missing,
    /// The trailing comma isn't needed and doesn't exist
    NotExists,
    /// The trailing comma isn't needed, but exists
    Redundant,
}
