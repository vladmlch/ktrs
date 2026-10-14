//! Port of IntelliJ `TreeElement` (links, offsets, raw relinking). Notifications (`subtreeChanged`,
//! `onInvalidated`, PSI events) have no observable effect for ktlint and are dropped; their cache
//! invalidation is done eagerly in `set_tree_parent`.

use crate::arena::{Ast, NONE, NodeId, opt, raw};
use crate::tree_util;

impl Ast {
    /// `clone()`: a deep, detached copy (`CompositeElement.clone` copies the children, a leaf clone
    /// shares its text).
    pub fn clone(&mut self, n: NodeId) -> NodeId {
        let copy = self.push_unlinked_copy(n);
        let mut child = self.first_child_node(n);
        while let Some(c) = child {
            let child_copy = self.clone(c);
            self.raw_add_children_without_notifications(copy, child_copy);
            child = self.tree_next(c);
        }
        copy
    }

    /// `getStartOffset()`: relative to the topmost ancestor (the file, or a dummy holder when detached).
    pub fn start_offset(&self, n: NodeId) -> usize {
        let mut result = 0;
        let mut current = n;
        while let Some(parent) = self.tree_parent(current) {
            result += self.start_offset_in_parent(current) as usize;
            current = parent;
        }
        result
    }

    /// `getStartOffsetInParent()`; `u32::MAX` (-1) without a parent.
    pub(crate) fn start_offset_in_parent(&self, n: NodeId) -> u32 {
        let node = self.node(n);
        if node.parent == NONE {
            return NONE;
        }
        let mut offset_in_parent = node.offset_in_parent.get();
        if offset_in_parent != NONE {
            return offset_in_parent;
        }
        let mut cur = n;
        loop {
            let prev = self.node(cur).prev;
            if prev == NONE {
                break;
            }
            cur = NodeId(prev);
            offset_in_parent = self.node(cur).offset_in_parent.get();
            if offset_in_parent != NONE {
                break;
            }
        }
        if offset_in_parent == NONE {
            offset_in_parent = 0;
            self.node(cur).offset_in_parent.set(0);
        }
        while cur != n {
            let next = NodeId(self.node(cur).next);
            offset_in_parent += self.node(cur).len;
            self.node(next).offset_in_parent.set(offset_in_parent);
            cur = next;
        }
        offset_in_parent
    }

    /// `setTreeParent`, plus the length bookkeeping IntelliJ does lazily via `subtreeChanged`.
    pub(crate) fn set_tree_parent(&mut self, n: NodeId, parent: u32) {
        let old = self.node(n).parent;
        if old == parent {
            return;
        }
        let metrics = (self.node(n).len, self.node(n).surplus, self.node(n).newlines);
        self.add_length_to_ancestors(old, metrics, false);
        self.node_mut(n).parent = parent;
        self.add_length_to_ancestors(parent, metrics, true);
        self.clear_text_hashes(old);
        self.clear_text_hashes(parent);
    }

    /// Clears the cached text hash of `element` and its ancestors, up to the first one already clear.
    pub(crate) fn clear_text_hashes(&self, element: u32) {
        let mut cur = element;
        while cur != NONE && self.nodes[cur as usize].text_hash.replace(NONE) != NONE {
            cur = self.nodes[cur as usize].parent;
        }
    }

    /// Adds (or removes) a subtree's `(len, surplus, newlines)` to every ancestor.
    fn add_length_to_ancestors(&mut self, mut ancestor: u32, (len, surplus, newlines): (u32, u32, u32), add: bool) {
        if len == 0 {
            return;
        }
        while ancestor != NONE {
            let node = &mut self.nodes[ancestor as usize];
            node.len = if add { node.len + len } else { node.len - len };
            node.surplus = if add { node.surplus + surplus } else { node.surplus - surplus };
            node.newlines = if add { node.newlines + newlines } else { node.newlines - newlines };
            let (next, parent) = (node.next, node.parent);
            self.clear_relative_offsets(next);
            ancestor = parent;
        }
    }

    pub(crate) fn set_tree_prev(&mut self, n: NodeId, prev: u32) {
        self.node_mut(n).prev = prev;
        self.clear_relative_offsets(n.0);
        self.clear_text_hashes(self.node(n).parent);
    }

    pub(crate) fn set_tree_next(&mut self, n: NodeId, next: u32) {
        self.node_mut(n).next = next;
        self.clear_relative_offsets(next);
        self.clear_text_hashes(self.node(n).parent);
    }

    pub(crate) fn clear_relative_offsets(&self, element: u32) {
        let mut cur = element;
        while cur != NONE && self.nodes[cur as usize].offset_in_parent.get() != NONE {
            self.nodes[cur as usize].offset_in_parent.set(NONE);
            cur = self.nodes[cur as usize].next;
        }
    }

    /// First leaf in preorder under `n` (or `n` itself if a leaf).
    pub fn first_leaf(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            Some(n)
        } else {
            let mut cur = self.first_child_node(n);
            while let Some(c) = cur {
                if let Some(leaf) = self.first_leaf(c) {
                    return Some(leaf);
                }
                cur = self.tree_next(c);
            }
            None
        }
    }

    /// Last leaf in preorder under `n` (or `n` itself if a leaf).
    pub fn last_leaf(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            Some(n)
        } else {
            let mut cur = self.last_child_node(n);
            while let Some(c) = cur {
                if let Some(leaf) = self.last_leaf(c) {
                    return Some(leaf);
                }
                cur = self.tree_prev(c);
            }
            None
        }
    }

    fn chain_leaves_boundaries(&self, start: NodeId, end: Option<NodeId>) -> (Option<NodeId>, Option<NodeId>) {
        let mut cur = Some(start);
        let mut first_leaf = None;
        let mut last_leaf = None;
        while let Some(c) = cur.filter(|&c| Some(c) != end) {
            if let Some(fl) = self.first_leaf(c) {
                if first_leaf.is_none() {
                    first_leaf = Some(fl);
                }
                last_leaf = self.last_leaf(c);
            }
            cur = self.tree_next(c);
        }
        (first_leaf, last_leaf)
    }

    pub(crate) fn prepare_chain_leaves(&mut self, start: NodeId, end: Option<NodeId>) -> (Option<NodeId>, Option<NodeId>) {
        let mut cur = Some(start);
        let mut first_leaf = None;
        let mut last_leaf = None;
        while let Some(c) = cur.filter(|&c| Some(c) != end) {
            if let Some(fl) = self.first_leaf(c) {
                if first_leaf.is_none() {
                    first_leaf = Some(fl);
                }
                if let Some(prev) = last_leaf {
                    self.set_leaf_next(prev, fl.0);
                    self.set_leaf_prev(fl, prev.0);
                }
                last_leaf = self.last_leaf(c);
            }
            cur = self.tree_next(c);
        }
        (first_leaf, last_leaf)
    }

    pub(crate) fn unlink_chain_leaves(&mut self, start: NodeId, end: Option<NodeId>) {
        let (first_leaf, last_leaf) = self.chain_leaves_boundaries(start, end);
        if let (Some(fl), Some(ll)) = (first_leaf, last_leaf) {
            let prev_l = self.leaf_prev(fl);
            let next_l = self.leaf_next(ll);
            if let Some(p) = prev_l {
                self.set_leaf_next(p, raw(next_l));
            }
            if let Some(n) = next_l {
                self.set_leaf_prev(n, raw(prev_l));
            }
            self.set_leaf_prev(fl, NONE);
            self.set_leaf_next(ll, NONE);
        }
    }

    /// `rawInsertBeforeMe(firstNew)`: inserts `first_new` and every sibling after it.
    pub fn raw_insert_before_me(&mut self, this: NodeId, first_new: NodeId) {
        match self.tree_prev(this) {
            None => {
                self.raw_remove_up_to_last(first_new);
                let next_l = self.first_leaf(this).or_else(|| tree_util::next_leaf(self, this));
                let prev_l = next_l.and_then(|n| self.leaf_prev(n)).or_else(|| tree_util::prev_leaf(self, this));

                let p = self.node(this).parent;
                if p != NONE {
                    self.set_first_child_node(NodeId(p), first_new.0);
                }
                let mut cur_new = first_new;
                loop {
                    let tree_next = self.node(cur_new).next;
                    assert!(tree_next != this.0, "Attempt to create cycle");
                    self.set_tree_parent(cur_new, p);
                    if tree_next == NONE {
                        self.set_tree_prev(this, cur_new.0);
                        self.set_tree_next(cur_new, this.0);
                        break;
                    }
                    cur_new = NodeId(tree_next);
                }

                let (new_first, new_last) = self.prepare_chain_leaves(first_new, Some(this));
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
            Some(anchor_prev) => self.raw_insert_after_me(anchor_prev, first_new),
        }
    }

    /// `rawInsertAfterMe(firstNew)`: inserts `first_new` and every sibling after it.
    pub fn raw_insert_after_me(&mut self, this: NodeId, first_new: NodeId) {
        self.raw_insert_after_me_without_notifications(this, first_new);
    }

    pub(crate) fn raw_insert_after_me_without_notifications(&mut self, this: NodeId, first_new: NodeId) {
        self.raw_remove_up_to_without_notifications(first_new, None, false);
        let prev_l = self.last_leaf(this).or_else(|| tree_util::prev_leaf(self, this));
        let next_l = prev_l.and_then(|p| self.leaf_next(p)).or_else(|| tree_util::next_leaf(self, this));

        let p = self.node(this).parent;
        let tree_next = self.node(this).next;
        self.set_tree_prev(first_new, this.0);
        self.set_tree_next(this, first_new.0);
        let mut cur_new = first_new;
        loop {
            let n = self.node(cur_new).next;
            assert!(n != this.0, "Attempt to create cycle");
            self.set_tree_parent(cur_new, p);
            if n == NONE {
                if tree_next == NONE {
                    if p != NONE {
                        self.set_tree_parent(cur_new, p);
                        self.set_last_child_node(NodeId(p), cur_new.0);
                    }
                } else {
                    self.set_tree_next(cur_new, tree_next);
                    self.set_tree_prev(NodeId(tree_next), cur_new.0);
                }
                break;
            }
            cur_new = NodeId(n);
        }

        let (new_first, new_last) = self.prepare_chain_leaves(first_new, opt(tree_next));
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

    /// `rawRemove()`: unlinks `this`, leaving it with no parent or siblings.
    pub fn raw_remove(&mut self, this: NodeId) {
        self.unlink_chain_leaves(this, self.tree_next(this));
        let node = self.node(this);
        let (next, parent, prev) = (node.next, node.parent, node.prev);
        if prev != NONE {
            self.set_tree_next(NodeId(prev), next);
        } else if parent != NONE {
            self.set_first_child_node(NodeId(parent), next);
        }
        if next != NONE {
            self.set_tree_prev(NodeId(next), prev);
        } else if parent != NONE {
            self.set_last_child_node(NodeId(parent), prev);
        }
        self.invalidate(this);
    }

    /// `rawReplaceWithList(firstNew)`.
    pub fn raw_replace_with_list(&mut self, this: NodeId, first_new: Option<NodeId>) {
        if let Some(first_new) = first_new {
            self.raw_insert_after_me_without_notifications(this, first_new);
        }
        self.raw_remove(this);
    }

    pub(crate) fn invalidate(&mut self, this: NodeId) {
        self.set_tree_next(this, NONE);
        self.set_tree_prev(this, NONE);
        self.set_tree_parent(this, NONE);
    }

    pub fn raw_remove_up_to_last(&mut self, this: NodeId) {
        self.raw_remove_up_to(this, None);
    }

    /// `rawRemoveUpTo(end)`: detaches `this` up to (excluding) `end`; the range stays linked as a chain.
    pub fn raw_remove_up_to(&mut self, this: NodeId, end: Option<NodeId>) {
        self.raw_remove_up_to_without_notifications(this, end, true);
    }

    pub(crate) fn raw_remove_up_to_without_notifications(&mut self, this: NodeId, end: Option<NodeId>, _invalidate: bool) {
        if Some(this) == end {
            return;
        }
        let parent = self.node(this).parent;
        let start_prev = self.node(this).prev;
        let end_prev = end.map_or(NONE, |e| self.node(e).prev);
        if let Some(end) = end {
            assert!(self.node(end).parent == parent, "Trying to remove non-child");
            let mut element = opt(this.0);
            while element.is_some() && element != Some(end) {
                element = self.tree_next(element.unwrap());
            }
            assert!(element == Some(end), "end is not successor of this in the getTreeNext() chain");
        }
        self.unlink_chain_leaves(this, end);
        if parent != NONE {
            if self.node(this).prev == NONE {
                self.set_first_child_node(NodeId(parent), raw(end));
            }
            if end.is_none() {
                self.set_last_child_node(NodeId(parent), start_prev);
            }
        }
        if start_prev != NONE {
            self.set_tree_next(NodeId(start_prev), raw(end));
        }
        if let Some(end) = end {
            self.set_tree_prev(end, start_prev);
        }
        self.set_tree_prev(this, NONE);
        if end_prev != NONE {
            self.set_tree_next(NodeId(end_prev), NONE);
        }
        if parent != NONE {
            let mut element = this.0;
            while element != NONE {
                self.set_tree_parent(NodeId(element), NONE);
                element = self.nodes[element as usize].next;
            }
        }
    }

    /// True when `ancestor` is a proper ancestor of `descendant`.
    #[inline]
    pub fn is_ancestor(&self, ancestor: NodeId, descendant: NodeId) -> bool {
        if self.is_leaf_element(ancestor) {
            return false;
        }
        let target = ancestor.0;
        let mut cur = self.nodes[descendant.0 as usize].parent;
        while cur != NONE {
            if cur == target {
                return true;
            }
            cur = self.nodes[cur as usize].parent;
        }
        false
    }
}
