//! Port of ktlint-rule-engine `internal/SuppressionLocator.kt`: `@Suppress`/`@SuppressWarnings` on the
//! nearest `KtAnnotated` owner and formatter-tag comments, as offset ranges.

use std::collections::BTreeSet;

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::kt_tokens::COMMENTS;
use ktrs_syntax::SyntaxKind::{
    ANNOTATION_ENTRY, CONSTRUCTOR_CALLEE, RBRACE, TYPE_REFERENCE, VALUE_ARGUMENT,
};

use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{EditorConfig, KtlintVersion};
use crate::engine::ast_helpers::{is_kt_annotated, recursive_children, text_range};
use crate::engine::formatter_tags::FormatterTags;
use crate::engine::suppression_ids::{ALL_KTLINT_RULES_SUPPRESSION_ID, find_rule_suppression_ids, remove_surrounding};
use crate::rule::RuleId;

/// `range` is inclusive, in UTF-8 offsets of the tree the hints were built from; empty ids = all rules.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SuppressionHint {
    range: (i64, i64),
    disabled_rule_ids: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct CommentSuppressionHint {
    node: NodeId,
    disabled_rule_ids: BTreeSet<String>,
    block_start: bool,
}

pub struct SuppressionLocator {
    formatter_tags: FormatterTags,
    ktlint_version: KtlintVersion,
    /// `rootNode.text.hashCode()` of the text the hints were built from, and the tree version it was
    /// last checked against.
    hashcode_ast_node_text: Option<i32>,
    checked_modification_count: Option<u64>,
    suppression_hints: Vec<SuppressionHint>,
    /// Whether any leaf allocated so far could make a hint; `allocated_leaf_text` bytes scanned so far.
    may_have_hints: bool,
    scanned_leaf_text: usize,
    /// The hints covering the offset of the node last asked about, for that node and tree version.
    covering_key: Option<(NodeId, u64)>,
    covering: Vec<usize>,
    /// Every `ANNOTATION_ENTRY` in the arena (attached or not), from the first `scanned_nodes` nodes.
    annotation_entries: Vec<NodeId>,
    scanned_nodes: usize,
}

impl SuppressionLocator {
    pub fn new(editor_config: &EditorConfig) -> SuppressionLocator {
        let formatter_tags = FormatterTags::from(editor_config);
        SuppressionLocator {
            may_have_hints: formatter_tags.formatter_tag_off.is_some(),
            formatter_tags,
            ktlint_version: KtlintVersion::of(editor_config),
            hashcode_ast_node_text: None,
            checked_modification_count: None,
            suppression_hints: Vec::new(),
            scanned_leaf_text: 0,
            covering_key: None,
            covering: Vec::new(),
            annotation_entries: Vec::new(),
            scanned_nodes: 0,
        }
    }

    /// Whether `node` (at its start offset in `root_node`) is suppressed for the rule `rule_id`
    /// (`ignores_suppressions`: the rule is `IgnoreKtlintSuppressions`). The hints are rebuilt when the
    /// root text changed (format mode mutates it).
    pub fn suppress(
        &mut self,
        ast: &Ast,
        root_node: NodeId,
        node: NodeId,
        rule_id: RuleId,
        ignores_suppressions: bool,
    ) -> bool {
        if ignores_suppressions {
            return false;
        }
        if !self.may_have_hints {
            let text = ast.allocated_leaf_text();
            if text.len() <= self.scanned_leaf_text || !self.refresh_may_have_hints(ast) {
                return false;
            }
        }
        let modification_count = ast.modification_count();
        if self.checked_modification_count != Some(modification_count) {
            self.checked_modification_count = Some(modification_count);
            let hash_code = ast.text_hash_code(root_node);
            if self.hashcode_ast_node_text != Some(hash_code) {
                self.hashcode_ast_node_text = Some(hash_code);
                self.suppression_hints = self.find_suppression_hints(ast, root_node);
                self.covering_key = None;
            }
        }
        if self.suppression_hints.is_empty() {
            return false;
        }
        // Every rule asks about the same node in turn: filter the hints by offset once.
        if self.covering_key != Some((node, modification_count)) {
            self.covering_key = Some((node, modification_count));
            let offset = ast.start_offset(node) as i64;
            self.covering.clear();
            self.covering.extend(
                (0..self.suppression_hints.len())
                    .filter(|&i| self.suppression_hints[i].range.0 <= offset && offset <= self.suppression_hints[i].range.1),
            );
        }
        let rule_id = rule_id.value();
        self.covering.iter().any(|&i| {
            let hint = &self.suppression_hints[i];
            hint.disabled_rule_ids.is_empty() || hint.disabled_rule_ids.contains(rule_id)
        })
    }

    #[inline(always)]
    pub fn can_suppress(&self) -> bool {
        self.may_have_hints
    }

    /// Hints come only from `@Suppress`/`@SuppressWarnings` annotations and formatter-tag comments. Leaf
    /// text never changes and a new leaf's text is appended whole, so scanning the new text suffices.
    pub fn refresh_may_have_hints(&mut self, ast: &Ast) -> bool {
        let text = ast.allocated_leaf_text();
        if !self.may_have_hints && text.len() > self.scanned_leaf_text {
            self.may_have_hints = text[self.scanned_leaf_text..].contains("Suppress");
            self.scanned_leaf_text = text.len();
        }
        self.may_have_hints
    }

    /// Without formatter tags only annotations make hints, so the tracked annotation entries still in
    /// the tree give the same hints as the full walk (in another order, which `suppress` ignores).
    fn find_suppression_hints(&mut self, ast: &Ast, root_node: NodeId) -> Vec<SuppressionHint> {
        if self.formatter_tags.formatter_tag_off.is_some() {
            return self.find_suppression_hints_in_tree(ast, root_node);
        }
        self.annotation_entries.extend(
            ast.nodes_allocated_since(self.scanned_nodes)
                .filter(|&n| ast.element_type(n) == ANNOTATION_ENTRY),
        );
        self.scanned_nodes = ast.node_count();
        self.annotation_entries
            .iter()
            .filter(|&&n| n != root_node && ast.parent_matching(n, |p| p == root_node).is_some())
            .filter(|&&n| is_suppress_annotation(ast, n))
            .filter_map(|&n| create_suppression_hint_from_annotations(ast, n, self.ktlint_version))
            .collect()
    }

    fn find_suppression_hints_in_tree(&self, ast: &Ast, root_node: NodeId) -> Vec<SuppressionHint> {
        let mut suppression_hints = Vec::new();
        let mut comment_suppressions_hints = Vec::new();
        for node in recursive_children(ast, root_node) {
            let element_type = ast.element_type(node);
            if COMMENTS.contains(element_type) {
                if let Some(hint) = self.create_suppression_hint_from_comment(ast, node) {
                    comment_suppressions_hints.push(hint);
                }
            } else if element_type == ANNOTATION_ENTRY
                && is_suppress_annotation(ast, node)
                && let Some(hint) = create_suppression_hint_from_annotations(ast, node, self.ktlint_version)
            {
                suppression_hints.push(hint);
            }
        }
        suppression_hints.extend(to_suppression_hints(ast, comment_suppressions_hints));
        suppression_hints
    }

    fn create_suppression_hint_from_comment(
        &self,
        ast: &Ast,
        node: NodeId,
    ) -> Option<CommentSuppressionHint> {
        let owned;
        let text = if ast.is_leaf_element(node) {
            ast.leaf_text(node)
        } else {
            owned = ast.text(node);
            &owned
        };
        let text = text.strip_prefix("//").unwrap_or(text);
        let text = text.strip_prefix("/*").unwrap_or(text);
        let text = text.strip_suffix("*/").unwrap_or(text);
        let mut parts = text.trim().split(' ');
        let first = parts.next();
        let tail = || parts.map(str::to_owned).collect();
        if first == self.formatter_tags.formatter_tag_off.as_deref() {
            Some(CommentSuppressionHint {
                node,
                disabled_rule_ids: tail(),
                block_start: true,
            })
        } else if first == self.formatter_tags.formatter_tag_on.as_deref() {
            Some(CommentSuppressionHint {
                node,
                disabled_rule_ids: tail(),
                block_start: false,
            })
        } else {
            None
        }
    }
}

fn to_suppression_hints(
    ast: &Ast,
    comment_suppression_hints: Vec<CommentSuppressionHint>,
) -> Vec<SuppressionHint> {
    let mut suppression_hints = Vec::new();
    let mut block_comment_suppression_hints: Vec<CommentSuppressionHint> = Vec::new();
    for comment_suppression_hint in comment_suppression_hints {
        if comment_suppression_hint.block_start {
            block_comment_suppression_hints.push(comment_suppression_hint);
        } else if let Some(index) = block_comment_suppression_hints
            .iter()
            .rposition(|it| it.disabled_rule_ids == comment_suppression_hint.disabled_rule_ids)
        {
            let open_hint = block_comment_suppression_hints.remove(index);
            suppression_hints.push(SuppressionHint {
                range: (
                    start_offset(ast, open_hint.node),
                    start_offset(ast, comment_suppression_hint.node) - 1,
                ),
                disabled_rule_ids: comment_suppression_hint.disabled_rule_ids,
            });
        }
    }
    // The remaining hints were not closed.
    for it in block_comment_suppression_hints {
        let range = match rbrace_of_containing_block(ast, it.node) {
            // Up to the end of the next sibling when the outer element does not end with a `}`.
            None => {
                let end = ast
                    .next_sibling(it.node)
                    .map_or(text_range(ast, it.node).1, |next| text_range(ast, next).1);
                (start_offset(ast, it.node), end as i64)
            }
            // Up to, but excluding, the `}` of the containing block.
            Some(rbrace) => (start_offset(ast, it.node), start_offset(ast, rbrace) - 1),
        };
        suppression_hints.push(SuppressionHint {
            range,
            disabled_rule_ids: it.disabled_rule_ids,
        });
    }
    suppression_hints
}

fn rbrace_of_containing_block(ast: &Ast, n: NodeId) -> Option<NodeId> {
    ast.parent(n)
        .and_then(|p| ast.last_child_node(p))
        .filter(|&it| ast.element_type(it) == RBRACE)
}

fn is_suppress_annotation(ast: &Ast, n: NodeId) -> bool {
    ast.find_child_by_type(n, CONSTRUCTOR_CALLEE)
        .and_then(|it| ast.find_child_by_type(it, TYPE_REFERENCE))
        .is_some_and(|it| ast.text_matches(it, "Suppress") || ast.text_matches(it, "SuppressWarnings"))
}

fn create_suppression_hint_from_annotations(ast: &Ast, n: NodeId, ktlint_version: KtlintVersion) -> Option<SuppressionHint> {
    let suppressed_rule_ids: Vec<String> = recursive_children(ast, n)
        .into_iter()
        .filter(|&it| ast.element_type(it) == VALUE_ARGUMENT)
        .flat_map(|it| find_rule_suppression_ids(remove_surrounding(&ast.text(it), "\""), ktlint_version))
        .collect();
    if suppressed_rule_ids.is_empty() {
        return None;
    }
    let owner = ast.parent_matching(n, |it| is_kt_annotated(ast.element_type(it)))?;
    let (start, end) = text_range(ast, owner);
    let disabled_rule_ids = if suppressed_rule_ids
        .iter()
        .any(|id| id == ALL_KTLINT_RULES_SUPPRESSION_ID)
    {
        BTreeSet::new()
    } else {
        suppressed_rule_ids.into_iter().collect()
    };
    Some(SuppressionHint {
        range: (start as i64, end as i64 - 1),
        disabled_rule_ids,
    })
}

fn start_offset(ast: &Ast, n: NodeId) -> i64 {
    ast.start_offset(n) as i64
}
