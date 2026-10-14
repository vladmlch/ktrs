//! Port of ktlint-ruleset-standard `WrappingRule.kt` (id `wrapping`), split in upstream order: `block.rs`
//! (`beforeVisitBlock` .. `rearrangeBlock`), `lists.rs` (`rearrangeSuperTypeList` .. `rearrangeTypeArgumentList`),
//! `arrow.rs` (`rearrangeClosingQuote` .. `requireNewlineAfterLeaf`), `helpers.rs` (`isMultiLine` .. `getEndOfBlock`).

mod arrow;
mod block;
mod helpers;
mod lists;

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{
    self, ARROW, BLOCK, CLOSING_QUOTE, GT, LBRACE, LBRACKET, LPAR, LT, RBRACE, RBRACKET, RPAR, SUPER_TYPE_LIST, TYPE_ARGUMENT_LIST,
    TYPE_PARAMETER_LIST, VALUE_ARGUMENT_LIST, VALUE_PARAMETER_LIST,
};

use crate::editorconfig::{INDENT_SIZE_PROPERTY, INDENT_STYLE_PROPERTY, KtlintVersion, MAX_LINE_LENGTH_PROPERTY, PropertyRef};
use crate::indent_config::IndentConfig;
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, VisitorModifier};
use crate::rules::STANDARD_RULE_ABOUT;
use crate::rules::max_line_length_rule::max_line_length;

/// Inserts missing newlines (e.g. between parentheses of a multi-line function call), indented relative to the
/// parent (a best effort for when the indentation rule does not run).
pub struct WrappingRule {
    indent_config: IndentConfig,
    max_line_length: i32,
    ktlint_version: KtlintVersion,
}

impl WrappingRule {
    pub fn new() -> WrappingRule {
        WrappingRule {
            indent_config: IndentConfig::default_indent_config(),
            max_line_length: MAX_LINE_LENGTH_PROPERTY.default_value,
            ktlint_version: KtlintVersion::default(),
        }
    }
}

impl Default for WrappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for WrappingRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:wrapping")
    }

    fn visitor_modifiers(&self) -> &'static [VisitorModifier] {
        const MODIFIERS: &[VisitorModifier] = &[VisitorModifier::run_after("standard:annotation")];
        MODIFIERS
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![
            PropertyRef::from(&*INDENT_SIZE_PROPERTY),
            PropertyRef::from(&*INDENT_STYLE_PROPERTY),
            PropertyRef::from(&*MAX_LINE_LENGTH_PROPERTY),
        ]
    }

    // Upstream's `line` counter (bumped on every WHITE_SPACE) only feeds trace logging, which is not ported.
    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.indent_config = IndentConfig::new(editor_config.get(&INDENT_STYLE_PROPERTY), editor_config.get(&INDENT_SIZE_PROPERTY));
        self.max_line_length = max_line_length(editor_config);
        self.ktlint_version = KtlintVersion::of(editor_config);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        match ast.element_type(node) {
            BLOCK => self.before_visit_block(ast, node, emit),
            LPAR | LBRACKET => self.rearrange_block(ast, node, emit),
            SUPER_TYPE_LIST => self.rearrange_super_type_list(ast, node, emit),
            VALUE_PARAMETER_LIST | VALUE_ARGUMENT_LIST => self.rearrange_value_list(ast, node, emit),
            TYPE_ARGUMENT_LIST | TYPE_PARAMETER_LIST => self.rearrange_type_argument_list(ast, node, emit),
            ARROW => self.rearrange_arrow(ast, node, emit),
            CLOSING_QUOTE => self.rearrange_closing_quote(ast, node, emit),
            _ => {}
        }
    }

    fn visits_after_child_nodes(&self) -> bool {
        true
    }

    fn visited_after_types(&self) -> Option<TokenSet> {
        Some(TokenSet::create(&[BLOCK]))
    }

    fn after_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        self.after_visit_block(ast, node, emit);
    }
}

const VISITED_TYPES: TokenSet = TokenSet::create(&[
    BLOCK,
    LPAR,
    LBRACKET,
    SUPER_TYPE_LIST,
    VALUE_PARAMETER_LIST,
    VALUE_ARGUMENT_LIST,
    TYPE_ARGUMENT_LIST,
    TYPE_PARAMETER_LIST,
    ARROW,
    CLOSING_QUOTE,
]);

const LTOKEN_SET: TokenSet = TokenSet::create(&[LPAR, LBRACE, LBRACKET, LT]);

/// `MATCHING_RTOKEN_MAP`: `LTOKEN_SET.types` zipped with `RTOKEN_SET.types` (both in element-type index order).
fn matching_rtoken(element_type: SyntaxKind) -> Option<SyntaxKind> {
    match element_type {
        LBRACKET => Some(RBRACKET),
        LBRACE => Some(RBRACE),
        LPAR => Some(RPAR),
        LT => Some(GT),
        _ => None,
    }
}
