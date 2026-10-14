//! Port of ktlint-ruleset-standard `BlankLineBetweenWhenConditions.kt`.

use std::sync::LazyLock;

use ktrs_ast::{Ast, NodeId};
use ktrs_editorconfig::PropertyType;
use ktrs_editorconfig::property_type::boolean_value_parser;
use ktrs_syntax::SyntaxKind::{WHEN, WHEN_ENTRY};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::{AstNodeExtension, AstNodeLines};
use crate::editorconfig::{EditorConfigProperty, KtlintVersion, PropertyRef};
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;

static LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY_TYPE: PropertyType<bool> = PropertyType {
    name: "ij_kotlin_line_break_after_multiline_when_entry",
    description: "Defines whether a blank line is to be added after a when entry. Contrary to default IDEA formatting, \
                  ktlint adds the blank line between all when-conditions if the when-statement contains at least one \
                  multiline when-condition. Or, it removes all blank lines between the when-conditions if the when-statement \
                  does not contain any multiline when-condition.",
    parser: boolean_value_parser,
    possible_values: &["true", "false"],
    lower_casing: true,
};

pub static LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY: LazyLock<EditorConfigProperty<bool>> =
    LazyLock::new(|| EditorConfigProperty::new(&LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY_TYPE, true));

/// The Kotlin Coding Conventions suggest to consider using a blank line after a multiline when-condition, which behavior is
/// managed via `ij_kotlin_line_break_after_multiline_when_entry`; ktlint adds/removes the blank line between all
/// when-conditions depending on whether the statement contains at least one multiline when-condition.
pub struct BlankLineBetweenWhenConditions {
    line_break_after_when_condition: bool,
    ktlint_version: KtlintVersion,
}

impl BlankLineBetweenWhenConditions {
    pub fn new() -> BlankLineBetweenWhenConditions {
        BlankLineBetweenWhenConditions {
            line_break_after_when_condition: LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY.default_value,
            ktlint_version: KtlintVersion::default(),
        }
    }
}

impl Default for BlankLineBetweenWhenConditions {
    fn default() -> Self {
        Self::new()
    }
}

const VISITED_TYPES: TokenSet = TokenSet::create(&[WHEN]);

impl RuleV2 for BlankLineBetweenWhenConditions {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:blank-line-between-when-conditions")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![PropertyRef::from(&*LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY)]
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.line_break_after_when_condition = editor_config.get(&LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY);
        self.ktlint_version = KtlintVersion::of(editor_config);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.element_type(node) == WHEN {
            self.visit_when_statement(ast, node, emit);
        }
    }
}

impl BlankLineBetweenWhenConditions {
    fn visit_when_statement(&self, ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>) {
        let has_multiline_when_condition = has_any_multiline_when_condition(ast, node, self.ktlint_version);
        if has_multiline_when_condition && self.line_break_after_when_condition {
            add_blank_lines_between_when_conditions(ast, node, emit_and_approve, self.ktlint_version);
        } else {
            remove_blank_lines_between_when_conditions(ast, node, emit_and_approve);
        }
    }
}

fn when_entries_but_first(ast: &Ast, node: NodeId) -> Vec<NodeId> {
    ast.children(node).filter(|&it| ast.element_type(it) == WHEN_ENTRY).skip(1).collect()
}

/// 1.8 checks the whitespace right after the previous entry, 2.0 the first newline after its EOL comment (#3261).
fn add_blank_lines_between_when_conditions(ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>, ktlint_version: KtlintVersion) {
    // Blank lines should only be added *between* when-conditions, so first when-condition is to be skipped
    for when_entry in when_entries_but_first(ast, node) {
        let whitespace = if ktlint_version.is_1_8() {
            find_whitespace_after_previous_code_sibling(ast, when_entry)
        } else {
            let Some(prev_code_leaf) = ast.prev_code_leaf(when_entry) else { continue };
            let last = ast
                .leaves_forwards_including_self(prev_code_leaf)
                .take_while(|&it| !ast.is_white_space_with_newline(it))
                .last()
                .expect("NoSuchElementException: Sequence is empty.");
            ast.next_leaf(last)
        };
        let Some(whitespace_before_when_entry) = whitespace.filter(|&it| !contains_blank_line(ast, it)) else { continue };
        emit_and_approve(
            ast,
            ast.start_offset(whitespace_before_when_entry) + 1,
            "Add a blank line between all when-conditions in case at least one multiline when-condition is found in the statement",
            true,
        )
        .if_autocorrect_allowed(|| {
            let text = format!("\n{}", ast.indent(when_entry));
            ast.upsert_whitespace_before_me(whitespace_before_when_entry, &text);
        });
    }
}

fn contains_blank_line(ast: &Ast, n: NodeId) -> bool {
    ast.is_white_space(n) && ast.newline_count(n) > 1
}

fn has_any_multiline_when_condition(ast: &Ast, n: NodeId, ktlint_version: KtlintVersion) -> bool {
    ast.children(n).any(|it| {
        ast.element_type(it) == WHEN_ENTRY && (ast.text_contains(it, '\n') || is_preceded_by_comment(ast, it, ktlint_version))
    })
}

/// 1.8 also counts a trailing EOL comment on the previous entry (#3177).
fn is_preceded_by_comment(ast: &Ast, n: NodeId, ktlint_version: KtlintVersion) -> bool {
    // Check if this when-entry is preceded by a comment on its own line - not a trailing comment on the previous when-entry
    let prev_non_whitespace = ast.prev_sibling_matching(n, |it| !ast.is_white_space(it));
    let Some(prev_non_whitespace) = prev_non_whitespace.filter(|&it| ast.is_part_of_comment(it)) else { return false };
    if ktlint_version.is_1_8() {
        return true;
    }

    // Found a comment before this when-entry, so check whether it's on its own line
    let whitespace_before_comment = ast.prev_sibling_matching(prev_non_whitespace, |it| ast.is_white_space(it));
    whitespace_before_comment.is_some_and(|it| ast.text_contains(it, '\n'))
}

fn find_whitespace_after_previous_code_sibling(ast: &Ast, n: NodeId) -> Option<NodeId> {
    ast.prev_code_sibling(n).map(|it| ast.last_child_leaf_or_self(it)).and_then(|it| ast.next_leaf_matching(it, |l| ast.is_white_space(l)))
}

fn remove_blank_lines_between_when_conditions(ast: &mut Ast, node: NodeId, emit_and_approve: &mut Emit<'_>) {
    // Blank lines should only be removed *between* when-conditions, so first when-condition is to be skipped
    for when_entry in when_entries_but_first(ast, node) {
        let Some(whitespace_before_when_entry) =
            find_whitespace_after_previous_code_sibling(ast, when_entry).filter(|&it| contains_blank_line(ast, it))
        else {
            continue;
        };
        emit_and_approve(
            ast,
            ast.start_offset(whitespace_before_when_entry) + 1,
            "Unexpected blank lines between when-condition if all when-conditions are single lines",
            true,
        )
        .if_autocorrect_allowed(|| {
            let text = format!("\n{}", ast.indent_without_newline_prefix(when_entry));
            ast.upsert_whitespace_before_me(whitespace_before_when_entry, &text);
        });
    }
}
