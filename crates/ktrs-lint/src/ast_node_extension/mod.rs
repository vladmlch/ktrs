//! Port of ktlint-rule-engine-core `ASTNodeExtension.kt`. Kotlin extension properties/functions become
//! methods on [`Ast`], split over three traits by upstream file position (`use crate::ast_node_extension::*`):
//! - [`AstNodeExtension`] (this file): leaf and sibling walks, parents, whitespace/comment predicates,
//!   `children`, up to `recursiveChildren`; plus `indent` and `hasModifier`, which predate the split;
//! - [`AstNodeLines`] (`lines.rs`): `column` through `lineLength`;
//! - [`AstNodeQueries`] (`queries.rs`): `afterCodeSibling` through `hasNoMaxLineLengthSuppression`.
//!
//! The edits (`upsertWhitespace*`, `replaceTextWith`, `replaceWith`, `remove`) are in `ast_node_edit.rs`.
//! An overload taking a predicate gets a `_matching` suffix (`nextLeaf { }` -> `next_leaf_matching`), a
//! nullable receiver (`ASTNode?.isWhiteSpace`) takes `impl Into<Option<NodeId>>`, and a `Sequence` result is
//! a lazy iterator over the live tree.

mod lines;
mod queries;

pub use lines::AstNodeLines;
pub use queries::AstNodeQueries;

use ktrs_ast::{Ast, NodeId, Preorder};
use ktrs_parser::kt_tokens::COMMENTS;
use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind::{self, FILE, MODIFIER_LIST, STRING_TEMPLATE, WHITE_SPACE};

pub trait AstNodeExtension {
    fn next_leaf(&self, n: NodeId) -> Option<NodeId>;
    fn next_leaf_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId>;
    fn first_child_leaf_or_self(&self, n: NodeId) -> NodeId;
    fn prev_leaf(&self, n: NodeId) -> Option<NodeId>;
    fn prev_leaf_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId>;
    fn last_child_leaf_or_self(&self, n: NodeId) -> NodeId;
    fn is_code(&self, n: NodeId) -> bool;
    fn prev_code_leaf(&self, n: NodeId) -> Option<NodeId>;
    fn next_code_leaf(&self, n: NodeId) -> Option<NodeId>;
    fn prev_code_sibling(&self, n: NodeId) -> Option<NodeId>;
    fn prev_sibling_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId>;
    fn prev_sibling(&self, n: NodeId) -> Option<NodeId>;
    fn next_code_sibling(&self, n: NodeId) -> Option<NodeId>;
    fn next_sibling_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId>;
    fn next_sibling(&self, n: NodeId) -> Option<NodeId>;
    fn parent(&self, n: NodeId) -> Option<NodeId>;
    fn find_parent_by_type(&self, n: NodeId, element_type: SyntaxKind) -> Option<NodeId>;
    fn parent_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId>;
    fn is_part_of_set(&self, n: NodeId, token_set: TokenSet) -> bool;
    fn is_part_of(&self, n: NodeId, element_type: SyntaxKind) -> bool;
    fn is_part_of_string(&self, n: NodeId) -> bool;
    fn is_white_space(&self, n: impl Into<Option<NodeId>>) -> bool;
    fn is_white_space_with_newline(&self, n: impl Into<Option<NodeId>>) -> bool;
    fn is_white_space_without_newline(&self, n: impl Into<Option<NodeId>>) -> bool;
    fn is_white_space_without_newline_or_null(&self, n: impl Into<Option<NodeId>>) -> bool;
    fn is_root(&self, n: NodeId) -> bool;
    fn is_leaf(&self, n: NodeId) -> bool;
    fn is_part_of_comment(&self, n: NodeId) -> bool;
    fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_;
    fn recursive_children(&self, n: NodeId) -> std::iter::Skip<Preorder<'_>>;
    fn indent(&self, n: NodeId) -> String;
    fn has_modifier(&self, n: NodeId, element_type: SyntaxKind) -> bool;
}

impl AstNodeExtension for Ast {
    fn next_leaf(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            let mut cur = self.leaf_next(n);
            while let Some(x) = cur.filter(|&x| self.text_length(x) == 0) {
                cur = self.leaf_next(x);
            }
            return cur;
        }
        let mut node = next_leaf_any(self, n);
        while let Some(x) = node.filter(|&x| self.text_length(x) == 0) {
            node = if self.is_leaf_element(x) { self.leaf_next(x) } else { next_leaf_any(self, x) };
        }
        node
    }

    fn next_leaf_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        let mut node = next_leaf_any(self, n);
        while let Some(x) = node.filter(|&x| !predicate(x)) {
            node = next_leaf_any(self, x);
        }
        node
    }

    fn first_child_leaf_or_self(&self, n: NodeId) -> NodeId {
        let mut node = n;
        while let Some(first) = self.first_child_node(node) {
            node = first;
        }
        node
    }

    fn prev_leaf(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            let mut cur = self.leaf_prev(n);
            while let Some(x) = cur.filter(|&x| self.text_length(x) == 0) {
                cur = self.leaf_prev(x);
            }
            return cur;
        }
        let mut node = prev_leaf_any(self, n);
        while let Some(x) = node.filter(|&x| self.text_length(x) == 0) {
            node = if self.is_leaf_element(x) { self.leaf_prev(x) } else { prev_leaf_any(self, x) };
        }
        node
    }

    fn prev_leaf_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        let mut node = prev_leaf_any(self, n);
        while let Some(x) = node.filter(|&x| !predicate(x)) {
            node = prev_leaf_any(self, x);
        }
        node
    }

    fn last_child_leaf_or_self(&self, n: NodeId) -> NodeId {
        let mut node = n;
        while let Some(last) = self.last_child_node(node) {
            node = last;
        }
        node
    }

    fn is_code(&self, n: NodeId) -> bool {
        !self.is_white_space(n) && !self.is_part_of_comment(n)
    }

    fn prev_code_leaf(&self, n: NodeId) -> Option<NodeId> {
        let mut node = self.prev_leaf(n);
        while let Some(x) = node.filter(|&x| !self.is_code(x)) {
            node = self.prev_leaf(x);
        }
        node
    }

    fn next_code_leaf(&self, n: NodeId) -> Option<NodeId> {
        let mut node = self.next_leaf(n);
        while let Some(x) = node.filter(|&x| !self.is_code(x)) {
            node = self.next_leaf(x);
        }
        node
    }

    fn prev_code_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.prev_sibling_matching(n, |it| self.is_code(it))
    }

    fn prev_sibling_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        let mut node = self.tree_prev(n);
        while let Some(x) = node {
            if predicate(x) {
                return Some(x);
            }
            node = self.tree_prev(x);
        }
        None
    }

    fn prev_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.tree_prev(n)
    }

    fn next_code_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.next_sibling_matching(n, |it| self.is_code(it))
    }

    fn next_sibling_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        let mut node = self.tree_next(n);
        while let Some(x) = node {
            if predicate(x) {
                return Some(x);
            }
            node = self.tree_next(x);
        }
        None
    }

    fn next_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.tree_next(n)
    }

    fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.tree_parent(n)
    }

    fn find_parent_by_type(&self, n: NodeId, element_type: SyntaxKind) -> Option<NodeId> {
        self.parent_matching(n, |it| self.element_type(it) == element_type)
    }

    fn parent_matching(&self, n: NodeId, predicate: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        let mut node = self.parent(n);
        while let Some(x) = node.filter(|&x| !predicate(x)) {
            node = self.parent(x);
        }
        node
    }

    fn is_part_of_set(&self, n: NodeId, token_set: TokenSet) -> bool {
        token_set.contains(self.element_type(n)) || self.parent_matching(n, |it| token_set.contains(self.element_type(it))).is_some()
    }

    fn is_part_of(&self, n: NodeId, element_type: SyntaxKind) -> bool {
        self.element_type(n) == element_type || self.find_parent_by_type(n, element_type).is_some()
    }

    fn is_part_of_string(&self, n: NodeId) -> bool {
        self.find_parent_by_type(n, STRING_TEMPLATE).is_some()
    }

    fn is_white_space(&self, n: impl Into<Option<NodeId>>) -> bool {
        n.into().is_some_and(|n| self.element_type(n) == WHITE_SPACE)
    }

    fn is_white_space_with_newline(&self, n: impl Into<Option<NodeId>>) -> bool {
        n.into().is_some_and(|n| self.is_white_space(n) && self.text_contains(n, '\n'))
    }

    fn is_white_space_without_newline(&self, n: impl Into<Option<NodeId>>) -> bool {
        n.into().is_some_and(|n| self.is_white_space(n) && !self.text_contains(n, '\n'))
    }

    fn is_white_space_without_newline_or_null(&self, n: impl Into<Option<NodeId>>) -> bool {
        n.into().is_none_or(|n| self.is_white_space_without_newline(n))
    }

    fn is_root(&self, n: NodeId) -> bool {
        self.element_type(n) == FILE
    }

    fn is_leaf(&self, n: NodeId) -> bool {
        self.first_child_node(n).is_none()
    }

    fn is_part_of_comment(&self, n: NodeId) -> bool {
        self.is_part_of_set(n, COMMENTS)
    }

    fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.first_child_node(n), |&node| self.next_sibling(node))
    }

    /// Preorder, `n` excluded.
    fn recursive_children(&self, n: NodeId) -> std::iter::Skip<Preorder<'_>> {
        self.preorder(n).skip(1)
    }

    fn indent(&self, n: NodeId) -> String {
        let indent = indent_internal(self, n);
        if indent.starts_with('\n') { indent.to_owned() } else { format!("\n{indent}") }
    }

    fn has_modifier(&self, n: NodeId, element_type: SyntaxKind) -> bool {
        self.find_child_by_type(n, MODIFIER_LIST)
            .is_some_and(|list| self.children(list).any(|it| self.element_type(it) == element_type))
    }
}

fn next_leaf_any(ast: &Ast, n: NodeId) -> Option<NodeId> {
    if ast.first_child_node(n).is_none() {
        return next_leaf_strict(ast, n);
    }
    Some(ast.first_child_leaf_or_self(n))
}

fn next_leaf_strict(ast: &Ast, n: NodeId) -> Option<NodeId> {
    match ast.next_sibling(n) {
        Some(next) => Some(ast.first_child_leaf_or_self(next)),
        None => ast.parent(n).and_then(|p| next_leaf_strict(ast, p)),
    }
}

fn prev_leaf_any(ast: &Ast, n: NodeId) -> Option<NodeId> {
    match ast.prev_sibling(n) {
        Some(prev) => Some(ast.last_child_leaf_or_self(prev)),
        None => ast.parent(n).and_then(|p| prev_leaf_any(ast, p)),
    }
}

/// `indentInternal()`: the text after the last newline of the nearest preceding newline whitespace.
fn indent_internal(ast: &Ast, n: NodeId) -> &str {
    ast.leaves(n, false)
        .find(|&it| ast.is_white_space_with_newline(it))
        .map(|it| {
            let text = ast.leaf_text(it);
            &text[text.rfind('\n').map_or(0, |i| i + 1)..]
        })
        .unwrap_or("")
}
