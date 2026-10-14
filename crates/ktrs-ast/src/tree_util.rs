//! Port of the IntelliJ `TreeUtil` statics ktlint reaches (free functions: `TreeUtil.nextLeaf(node)` ->
//! `tree_util::next_leaf(ast, node)`, so they can't be mistaken for ktlint's own `nextLeaf`), and of the
//! compiler's psiUtil `ASTNode.leaves`. These walks only ever return `LeafElement`s.

use crate::arena::{Ast, NodeId};

/// `TreeUtil.findLastLeaf(element)`.
pub fn find_last_leaf(ast: &Ast, element: NodeId) -> Option<NodeId> {
    ast.last_leaf(element)
}

/// `TreeUtil.getFileElement(element)`: the enclosing file or dummy holder, `element` included.
pub fn get_file_element(ast: &Ast, element: NodeId) -> Option<NodeId> {
    let mut parent = Some(element);
    while let Some(p) = parent {
        if ast.is_file_element(p) {
            return Some(p);
        }
        parent = ast.tree_parent(p);
    }
    None
}

/// `TreeUtil.nextLeaf(node)`.
pub fn next_leaf(ast: &Ast, start: NodeId) -> Option<NodeId> {
    if ast.is_leaf_element(start) {
        return ast.leaf_next(start);
    }
    if let Some(last) = ast.last_leaf(start) {
        return ast.leaf_next(last);
    }
    let mut element = Some(start);
    while let Some(e) = element {
        let mut next_tree = ast.tree_next(e);
        while let Some(t) = next_tree {
            if let Some(next) = ast.first_leaf(t) {
                return Some(next);
            }
            next_tree = ast.tree_next(t);
        }
        element = ast.tree_parent(e);
    }
    None
}

/// `TreeUtil.prevLeaf(node)`.
pub fn prev_leaf(ast: &Ast, start: NodeId) -> Option<NodeId> {
    if ast.is_leaf_element(start) {
        return ast.leaf_prev(start);
    }
    if let Some(first) = ast.first_leaf(start) {
        return ast.leaf_prev(first);
    }
    let mut start = Some(start);
    while let Some(s) = start {
        let mut prev_tree = ast.tree_prev(s);
        while let Some(t) = prev_tree {
            if let Some(prev) = ast.last_leaf(t) {
                return Some(prev);
            }
            prev_tree = ast.tree_prev(t);
        }
        start = ast.tree_parent(s);
    }
    None
}

/// `TreeUtil.CommonParentState`, the part `prevLeaf(start, state)` fills. (`strongWhiteSpaceHolder` keys
/// on `UNCLOSED_ELEMENT_PROPERTY` user data, which only the incremental reparser sets.)
#[derive(Clone, Copy, Debug, Default)]
pub struct CommonParentState {
    /// The ancestor-or-self of `start` whose previous sibling holds the returned leaf.
    pub next_leaf_branch_start: Option<NodeId>,
    /// That previous sibling.
    pub start_leaf_branch_start: Option<NodeId>,
}

/// `TreeUtil.prevLeaf(start, commonParent)`.
pub fn prev_leaf_with_state(ast: &Ast, start: NodeId, common_parent: &mut CommonParentState) -> Option<NodeId> {
    let mut start = Some(start);
    while let Some(s) = start {
        common_parent.next_leaf_branch_start = Some(s);
        let mut prev_tree = ast.tree_prev(s);
        while let Some(t) = prev_tree {
            if let Some(prev) = ast.last_leaf(t) {
                common_parent.start_leaf_branch_start = Some(t);
                return Some(prev);
            }
            prev_tree = ast.tree_prev(t);
        }
        start = ast.tree_parent(s);
    }
    None
}

impl Ast {
    /// psiUtil `ASTNode.leaves(forward)`: the leaves after (or before) `node`, nearest first.
    pub fn leaves(&self, node: NodeId, forward: bool) -> Leaves<'_> {
        let next = if forward { next_leaf(self, node) } else { prev_leaf(self, node) };
        Leaves { ast: self, next, forward }
    }
}

pub struct Leaves<'a> {
    ast: &'a Ast,
    next: Option<NodeId>,
    forward: bool,
}

impl Iterator for Leaves<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = if self.forward { self.ast.leaf_next(cur) } else { self.ast.leaf_prev(cur) };
        Some(cur)
    }
}
