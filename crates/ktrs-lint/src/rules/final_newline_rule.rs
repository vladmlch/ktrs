//! Port of ktlint-ruleset-standard `FinalNewlineRule.kt`.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{FILE, WHITE_SPACE};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{INSERT_FINAL_NEWLINE_PROPERTY, PropertyRef};
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, TokenSet, TraversalState};
use crate::rules::STANDARD_RULE_ABOUT;

const VISITED_TYPES: TokenSet = TokenSet::create(&[FILE]);

pub struct FinalNewlineRule {
    insert_final_newline: bool,
    traversal: TraversalState,
}

impl FinalNewlineRule {
    pub fn new() -> FinalNewlineRule {
        FinalNewlineRule { insert_final_newline: INSERT_FINAL_NEWLINE_PROPERTY.default_value, traversal: TraversalState::default() }
    }
}

impl Default for FinalNewlineRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for FinalNewlineRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:final-newline")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![PropertyRef::from(&*INSERT_FINAL_NEWLINE_PROPERTY)]
    }

    fn traversal_state(&self) -> Option<TraversalState> {
        Some(self.traversal)
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.insert_final_newline = editor_config.get(&INSERT_FINAL_NEWLINE_PROPERTY);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.is_root(node) {
            if ast.text_length(node) == 0 {
                self.traversal.stop_traversal_of_ast();
                return;
            }
            let last_node = last_child_node_of(ast, node);
            if self.insert_final_newline && (!ast.is_white_space(last_node) || ast.is_white_space_without_newline(last_node)) {
                // `textLength - 1` is the start of the last character (UTF-16); emit takes UTF-8 offsets
                let last_char_len = ast.text(node).chars().last().map_or(1, char::len_utf8);
                emit(ast, ast.text_length(node) - last_char_len, "File must end with a newline (\\n)", true).if_autocorrect_allowed(|| {
                    let white_space = ast.new_leaf(WHITE_SPACE, "\n");
                    ast.add_child(node, white_space, None);
                });
            } else if !self.insert_final_newline && ast.is_white_space_with_newline(last_node) {
                emit(ast, ast.start_offset(last_node), "Redundant newline (\\n) at the end of file", true)
                    .if_autocorrect_allowed(|| ast.remove(last_node));
            }
        }
        self.traversal.stop_traversal_of_ast();
    }
}

fn last_child_node_of(ast: &Ast, node: NodeId) -> NodeId {
    match ast.last_child_node(node) {
        None => node,
        Some(last) => last_child_node_of(ast, last),
    }
}
