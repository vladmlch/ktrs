//! Port of ktlint-ruleset-standard `TrailingCommaOnCallSiteRule.kt`.

use std::sync::LazyLock;

use ktrs_ast::{Ast, NodeId};
use ktrs_editorconfig::PropertyType;
use ktrs_editorconfig::property_type::boolean_value_parser;
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{
    COLLECTION_LITERAL_EXPRESSION, COMMA, FUNCTION_LITERAL, GT, INDICES, RBRACKET, RPAR, TYPE_ARGUMENT_LIST, VALUE_ARGUMENT,
    VALUE_ARGUMENT_LIST,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{EditorConfigProperty, PropertyRef};
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, VisitorModifier};
use crate::rules::STANDARD_RULE_ABOUT;

static TRAILING_COMMA_ON_CALL_SITE_PROPERTY_TYPE: PropertyType<bool> = PropertyType {
    name: "ij_kotlin_allow_trailing_comma_on_call_site",
    description: "Defines whether a trailing comma (or no trailing comma) should be enforced on the calling side,\
                  e.g. argument-list, when-entries, lambda-arguments, indices, etc.\
                  When set, IntelliJ IDEA uses this property to allow usage of a trailing comma by discretion \
                  of the developer. KtLint however uses this setting to enforce consistent usage of the \
                  trailing comma when set.",
    parser: boolean_value_parser,
    possible_values: &["true", "false"],
    lower_casing: true,
};

pub static TRAILING_COMMA_ON_CALL_SITE_PROPERTY: LazyLock<EditorConfigProperty<bool>> = LazyLock::new(|| EditorConfigProperty {
    android_studio_code_style_default_value: false,
    ..EditorConfigProperty::new(&TRAILING_COMMA_ON_CALL_SITE_PROPERTY_TYPE, true)
});

const TYPES_ON_CALL_SITE: TokenSet = TokenSet::create(&[COLLECTION_LITERAL_EXPRESSION, INDICES, TYPE_ARGUMENT_LIST, VALUE_ARGUMENT_LIST]);

/// Linting trailing comma for call site (https://kotlinlang.org/docs/coding-conventions.html#trailing-commas).
pub struct TrailingCommaOnCallSiteRule {
    allow_trailing_comma_on_call_site: bool,
}

impl TrailingCommaOnCallSiteRule {
    pub fn new() -> TrailingCommaOnCallSiteRule {
        TrailingCommaOnCallSiteRule { allow_trailing_comma_on_call_site: TRAILING_COMMA_ON_CALL_SITE_PROPERTY.default_value }
    }
}

impl Default for TrailingCommaOnCallSiteRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for TrailingCommaOnCallSiteRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:trailing-comma-on-call-site")
    }

    fn visitor_modifiers(&self) -> &'static [VisitorModifier] {
        const MODIFIERS: &[VisitorModifier] = &[
            VisitorModifier::run_after_loaded_and_enabled("standard:wrapping"),
            VisitorModifier::RunAsLateAsPossible,
        ];
        MODIFIERS
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(TYPES_ON_CALL_SITE)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![PropertyRef::from(&*TRAILING_COMMA_ON_CALL_SITE_PROPERTY)]
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.allow_trailing_comma_on_call_site = editor_config.get(&TRAILING_COMMA_ON_CALL_SITE_PROPERTY);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        // Keep processing of element types in sync with Intellij Kotlin formatting settings.
        // https://github.com/JetBrains/intellij-kotlin/blob/master/formatter/src/org/jetbrains/kotlin/idea/formatter/trailingComma/util.kt
        match ast.element_type(node) {
            COLLECTION_LITERAL_EXPRESSION => self.visit_collection_literal_expression(ast, node, emit),
            INDICES => self.visit_indices(ast, node, emit),
            TYPE_ARGUMENT_LIST => self.visit_type_list(ast, node, emit),
            VALUE_ARGUMENT_LIST => self.visit_value_list(ast, node, emit),
            _ => {}
        }
    }
}

const NO_MATCHING_ELEMENT: &str = "NoSuchElementException: Sequence contains no element matching the predicate.";

impl TrailingCommaOnCallSiteRule {
    fn visit_collection_literal_expression(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let inspect_node = ast.children(node).filter(|&it| ast.element_type(it) == RBRACKET).last().expect(NO_MATCHING_ELEMENT);
        report_and_correct_trailing_comma_node_before(ast, node, inspect_node, self.is_trailing_comma_allowed(ast, node), emit);
    }

    fn is_trailing_comma_allowed(&self, ast: &Ast, n: NodeId) -> bool {
        TYPES_ON_CALL_SITE.contains(ast.element_type(n)) && self.allow_trailing_comma_on_call_site
    }

    fn visit_indices(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let inspect_node = ast.children(node).filter(|&it| ast.element_type(it) == RBRACKET).last().expect(NO_MATCHING_ELEMENT);
        report_and_correct_trailing_comma_node_before(ast, node, inspect_node, self.is_trailing_comma_allowed(ast, node), emit);
    }

    fn visit_value_list(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.parent(node).map(|it| ast.element_type(it)) != Some(FUNCTION_LITERAL)
            && let Some(inspect_node) = ast.children(node).filter(|&it| ast.element_type(it) == RPAR).last()
        {
            report_and_correct_trailing_comma_node_before(ast, node, inspect_node, self.is_trailing_comma_allowed(ast, node), emit);
        }
    }

    fn visit_type_list(&self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        let inspect_node = ast.children(node).find(|&it| ast.element_type(it) == GT).expect(NO_MATCHING_ELEMENT);
        report_and_correct_trailing_comma_node_before(ast, node, inspect_node, self.is_trailing_comma_allowed(ast, node), emit);
    }
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

fn report_and_correct_trailing_comma_node_before(
    ast: &mut Ast,
    this: NodeId,
    inspect_node: NodeId,
    is_trailing_comma_allowed: bool,
    emit: &mut Emit<'_>,
) {
    let prev_leaf = ast.prev_leaf(inspect_node);
    let trailing_comma_node = prev_leaf.and_then(|it| find_previous_trailing_comma_node_or_null(ast, it));
    let trailing_comma_state = if is_multiline(ast, this) {
        if trailing_comma_node.is_some() { TrailingCommaState::Exists } else { TrailingCommaState::Missing }
    } else if trailing_comma_node.is_some() {
        TrailingCommaState::Redundant
    } else {
        TrailingCommaState::NotExists
    };
    match trailing_comma_state {
        TrailingCommaState::Exists => {
            if !is_trailing_comma_allowed {
                let trailing_comma_node = trailing_comma_node.unwrap();
                let message = format!("Unnecessary trailing comma before \"{}\"", ast.leaf_text(inspect_node));
                emit(ast, ast.start_offset(trailing_comma_node), &message, true).if_autocorrect_allowed(|| ast.remove(trailing_comma_node));
            }
        }
        TrailingCommaState::Missing => {
            if is_trailing_comma_allowed {
                let prev_node = ast.prev_code_leaf(inspect_node).expect("NullPointerException: prevCodeLeaf!!");
                let message = format!("Missing trailing comma before \"{}\"", ast.leaf_text(inspect_node));
                emit(ast, ast.start_offset(prev_node) + ast.text_length(prev_node), &message, true).if_autocorrect_allowed(|| {
                    if let Some(before) = ast.prev_code_sibling(inspect_node).and_then(|it| ast.next_sibling(it))
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
        TrailingCommaState::NotExists => {
            // Nothing to do
        }
    }
}

fn is_multiline(ast: &Ast, n: NodeId) -> bool {
    if ast.element_type(n) == VALUE_ARGUMENT_LIST {
        has_at_least_one_argument(ast, n) && has_value_argument_followed_by_white_space_with_newline(ast, n)
    } else {
        ast.text_contains(n, '\n')
    }
}

fn has_value_argument_followed_by_white_space_with_newline(ast: &Ast, n: NodeId) -> bool {
    find_value_argument_followed_by_white_space_with_newline(ast, n).is_some()
}

fn find_value_argument_followed_by_white_space_with_newline(ast: &Ast, n: NodeId) -> Option<NodeId> {
    ast.find_child_by_type(n, VALUE_ARGUMENT).and_then(|it| ast.next_sibling_matching(it, |s| ast.is_white_space_with_newline(s)))
}

fn has_at_least_one_argument(ast: &Ast, n: NodeId) -> bool {
    ast.children(n).any(|it| ast.element_type(it) == VALUE_ARGUMENT)
}

fn find_previous_trailing_comma_node_or_null(ast: &Ast, n: NodeId) -> Option<NodeId> {
    let code_leaf = if ast.is_code(n) { Some(n) } else { ast.prev_code_leaf(n) };
    code_leaf.filter(|&it| ast.element_type(it) == COMMA)
}
