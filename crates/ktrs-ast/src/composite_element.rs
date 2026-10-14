//! Port of IntelliJ `CompositeElement`'s child queries and edits. `ChangeUtil.prepareAndRunChangeAction`
//! only runs the action under ktlint's `FormatPomModel` (no reformat, no whitespace merging); what
//! survives of it is the NPE when the changed element is in no file.

use ktrs_parser::token_set::TokenSet;
use ktrs_syntax::SyntaxKind;

use crate::arena::{Ast, NONE, NodeId};
use crate::tree_util;

/// `Logger.assertTrue(false[, message])`: ktlint's logger rethrows the `Throwable` that `error` gets.
const LOG_ASSERTION_FAILED: &str = "java.lang.Throwable: Assertion failed";

impl Ast {
    pub fn find_child_by_type(&self, n: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let mut element = self.first_child_node(n);
        while let Some(e) = element {
            if self.element_type(e) == kind {
                return Some(e);
            }
            element = self.tree_next(e);
        }
        None
    }

    /// `getChildren(null)` appended to `out`: a snapshot, unaffected by later edits.
    pub fn get_children(&self, n: NodeId, out: &mut Vec<NodeId>) {
        if self.is_leaf_element(n) {
            return;
        }
        let mut child = self.node(n).first;
        while child != NONE {
            out.push(NodeId(child));
            child = self.nodes[child as usize].next;
        }
    }

    /// `getChildren(filter)` with a non-null filter, appended to `out`.
    pub fn get_children_filtered(&self, n: NodeId, filter: TokenSet, out: &mut Vec<NodeId>) {
        let mut child = self.first_child_node(n);
        while let Some(c) = child {
            if filter.contains(self.element_type(c)) {
                out.push(c);
            }
            child = self.tree_next(c);
        }
    }

    pub(crate) fn set_first_child_node(&mut self, n: NodeId, first_child: u32) {
        self.node_mut(n).first = first_child;
        self.clear_relative_offsets(first_child);
        self.clear_text_hashes(n.0);
    }

    pub(crate) fn set_last_child_node(&mut self, n: NodeId, last_child: u32) {
        self.node_mut(n).last = last_child;
        self.clear_text_hashes(n.0);
    }

    /// `addChild(child, anchorBefore)`: moves `child` (alone) before `anchor_before`, or to the end.
    pub fn add_child(&mut self, this: NodeId, child: NodeId, anchor_before: Option<NodeId>) {
        assert!(
            anchor_before.is_none_or(|a| self.node(a).parent == this.0),
            "{LOG_ASSERTION_FAILED}: anchorBefore == null || anchorBefore.getTreeParent() == parent"
        );
        let last = self.tree_next(child);
        let first = child;
        self.remove_children_inner(first, last);
        self.prepare_and_run_change_action(this);
        match anchor_before {
            Some(anchor_before) => self.insert_before(anchor_before, first),
            None => self.add(this, first),
        }
    }

    /// `addLeaf(leafType, leafText, anchorBefore)`: a new leaf, first parked in its own dummy holder,
    /// then moved in by `addChild`.
    pub fn add_leaf(&mut self, this: NodeId, leaf_type: SyntaxKind, leaf_text: &str, anchor_before: Option<NodeId>) {
        let holder = self.new_dummy_holder();
        let leaf = self.new_leaf(leaf_type, leaf_text);
        self.raw_add_children(holder, leaf);
        self.add_child(this, leaf, anchor_before);
    }

    pub fn remove_child(&mut self, _this: NodeId, child: NodeId) {
        self.remove_child_inner(child);
    }

    pub fn remove_range(&mut self, _this: NodeId, first: NodeId, first_which_stay_in_tree: Option<NodeId>) {
        self.remove_children_inner(first, first_which_stay_in_tree);
    }

    /// `replaceChild(oldChild, newChild)`: `old_child` ends up in a new dummy holder.
    pub fn replace_child(&mut self, this: NodeId, old_child: NodeId, new_child: NodeId) {
        assert!(self.node(old_child).parent == this.0, "{LOG_ASSERTION_FAILED}");
        let new_child_next = self.tree_next(new_child);
        if old_child != new_child {
            self.remove_children_inner(new_child, new_child_next);
            self.prepare_and_run_change_action(this);
            self.replace(old_child, new_child);
            self.repair_removed_element(Some(old_child));
        }
    }

    /// `addChildren(firstChild, lastChild, anchorBefore)`: one `addChild` per node of the range.
    pub fn add_children(&mut self, this: NodeId, first_child: NodeId, last_child: Option<NodeId>, anchor_before: Option<NodeId>) {
        let mut f = Some(first_child);
        while f != last_child {
            let current = f.expect(
                "java.lang.NullPointerException: Cannot invoke \"org.jetbrains.kotlin.com.intellij.lang.ASTNode.getTreeNext()\" because \"f\" is null",
            );
            f = self.tree_next(current);
            self.add_child(this, current, anchor_before);
        }
    }

    pub fn raw_add_children(&mut self, this: NodeId, first: NodeId) {
        self.raw_add_children_without_notifications(this, first);
    }

    pub(crate) fn raw_add_children_without_notifications(&mut self, this: NodeId, first: NodeId) {
        match self.last_child_node(this) {
            None => {
                let chain_last = self.raw_set_parents(first, this);
                let prev_l = tree_util::prev_leaf(self, this);
                let next_l = tree_util::next_leaf(self, this);
                self.set_first_child_node(this, first.0);
                self.set_last_child_node(this, chain_last.0);

                let (new_first, new_last) = self.prepare_chain_leaves(first, None);
                if let (Some(nf), Some(nl)) = (new_first, new_last) {
                    if let Some(prev) = prev_l {
                        self.set_leaf_next(prev, nf.0);
                        self.set_leaf_prev(nf, prev.0);
                    } else {
                        self.set_leaf_prev(nf, NONE);
                    }
                    if let Some(next) = next_l {
                        self.set_leaf_prev(next, nl.0);
                        self.set_leaf_next(nl, next.0);
                    } else {
                        self.set_leaf_next(nl, NONE);
                    }
                }
            }
            Some(last) => self.raw_insert_after_me_without_notifications(last, first),
        }
    }

    fn raw_set_parents(&mut self, child: NodeId, parent: NodeId) -> NodeId {
        self.raw_remove_up_to_without_notifications(child, None, false);
        let mut child = child;
        loop {
            self.set_tree_parent(child, parent.0);
            match self.tree_next(child) {
                None => return child,
                Some(tree_next) => child = tree_next,
            }
        }
    }

    fn repair_removed_element(&mut self, old_child: Option<NodeId>) {
        if let Some(old_child) = old_child {
            let tree_element = self.new_dummy_holder();
            self.raw_add_children(tree_element, old_child);
        }
    }

    fn add(&mut self, parent: NodeId, first: NodeId) {
        self.raw_add_children(parent, first);
    }

    fn remove(&mut self, first: Option<NodeId>, last: Option<NodeId>) {
        if let Some(first) = first {
            self.raw_remove_up_to(first, last);
        }
    }

    fn insert_before(&mut self, anchor_before: NodeId, first: NodeId) {
        self.raw_insert_before_me(anchor_before, first);
    }

    fn replace(&mut self, old_child: NodeId, new_child: NodeId) {
        self.raw_replace_with_list(old_child, Some(new_child));
    }

    fn remove_child_inner(&mut self, child: NodeId) {
        let next = self.tree_next(child);
        self.remove_children_inner(child, next);
    }

    fn remove_children_inner(&mut self, first: NodeId, last: Option<NodeId>) {
        if tree_util::get_file_element(self, first).is_some() {
            self.prepare_and_run_change_action(NodeId(self.node(first).parent));
            self.remove(Some(first), last);
            self.repair_removed_element(Some(first));
        } else {
            self.raw_remove_up_to(first, last);
        }
    }

    /// What remains of `ChangeUtil.prepareAndRunChangeAction(action, changedElement)`: it dereferences
    /// `TreeUtil.getFileElement(changedElement)`, so an element outside any file throws.
    fn prepare_and_run_change_action(&self, changed_element: NodeId) {
        assert!(
            changed_element.0 != NONE && tree_util::get_file_element(self, changed_element).is_some(),
            "NullPointerException: ChangeUtil.prepareAndRunChangeAction on an element outside any file"
        );
    }
}
