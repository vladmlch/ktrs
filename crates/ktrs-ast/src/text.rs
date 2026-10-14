//! Text queries (`getText`, `textContains`, `textMatches`), the preorder walk, and the UTF-16
//! conversions for emits and lengths.

use crate::arena::{Ast, NodeId};

/// Preorder over `root`'s subtree, `root` included.
pub struct Preorder<'a> {
    ast: &'a Ast,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Preorder<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.ast.first_child_node(cur).or_else(|| {
            let mut c = cur;
            loop {
                if c == self.root {
                    return None;
                }
                if let Some(next) = self.ast.tree_next(c) {
                    return Some(next);
                }
                c = self.ast.tree_parent(c)?;
            }
        });
        Some(cur)
    }
}

impl Ast {
    /// `root` and its descendants in preorder (lazy over the live tree, like a Kotlin `sequence {}`).
    pub fn preorder(&self, root: NodeId) -> Preorder<'_> {
        Preorder { ast: self, root, next: Some(root) }
    }

    /// `getTextLength()` in UTF-16 units, the JVM's `String.length`: what columns and line lengths count.
    pub fn text_length_utf16(&self, n: NodeId) -> usize {
        self.text_length(n) - self.node(n).surplus as usize
    }

    /// The leaf texts of `n`'s subtree in order.
    pub fn text_chunks(&self, n: NodeId) -> impl Iterator<Item = &str> + '_ {
        self.preorder(n).filter(|&e| self.is_leaf_element(e)).map(|e| self.leaf_text(e))
    }

    /// `getText()`.
    pub fn text(&self, n: NodeId) -> String {
        let mut text = String::with_capacity(self.text_length(n));
        self.text_chunks(n).for_each(|chunk| text.push_str(chunk));
        text
    }

    /// `getText().hashCode()` (Java `String.hashCode()`), recomputed only along the paths edited since
    /// the last call: `hash(ab) = hash(a) * 31^utf16_len(b) + hash(b)`.
    pub fn text_hash_code(&self, n: NodeId) -> i32 {
        let hash = self.node(n).text_hash.get();
        if hash != u32::MAX {
            return hash as i32;
        }
        let hash = if self.is_leaf_element(n) {
            self.leaf_text(n).encode_utf16().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)))
        } else {
            let mut hash = 0u32;
            let mut child = self.first_child_node(n);
            while let Some(c) = child {
                let c_hash = self.text_hash_code(c) as u32;
                hash = hash.wrapping_mul(31u32.wrapping_pow(self.text_length_utf16(c) as u32)).wrapping_add(c_hash);
                child = self.tree_next(c);
            }
            hash
        };
        self.node(n).text_hash.set(hash);
        hash as i32
    }

    /// Whether the text of the leaves allocated so far (seeded or new, attached or not) contains `needle`: a
    /// monotonic gate for a search that needs a tree text containing it. Scans only the text allocated since the
    /// last call with this needle. Gotcha: a match spanning leaves allocated at different times may be missed.
    pub fn allocated_leaf_text_contains(&self, needle: &'static str) -> bool {
        let text = self.allocated_leaf_text();
        let mut scans = self.needle_scans.borrow_mut();
        let i = scans.iter().position(|(n, ..)| *n == needle).unwrap_or_else(|| {
            scans.push((needle, 0, false));
            scans.len() - 1
        });
        let (_, scanned, found) = &mut scans[i];
        if !*found && text.len() > *scanned {
            let mut start = scanned.saturating_sub(needle.len() - 1);
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            *found = text[start..].contains(needle);
            *scanned = text.len();
        }
        *found
    }

    /// `textContains(c)`; O(1) for `\n` (every node keeps its newline count).
    pub fn text_contains(&self, n: NodeId, c: char) -> bool {
        if c == '\n' {
            return self.node(n).newlines != 0;
        }
        self.text_chunks(n).any(|chunk| chunk.contains(c))
    }

    /// `textMatches(seq)`.
    pub fn text_matches(&self, n: NodeId, seq: &str) -> bool {
        if self.text_length(n) != seq.len() {
            return false;
        }
        let mut rest = seq;
        self.text_chunks(n).all(|chunk| match rest.strip_prefix(chunk) {
            Some(r) => {
                rest = r;
                true
            }
            None => false,
        })
    }

    /// The root of the tree `n` is in: the file, or the dummy holder of a detached node.
    pub fn tree_root(&self, n: NodeId) -> NodeId {
        let mut cur = n;
        while let Some(p) = self.tree_parent(cur) {
            cur = p;
        }
        cur
    }

    /// The UTF-16 offset (what the JVM's `startOffset` counts) of the UTF-8 `byte_offset` into the
    /// text of `n`'s tree. Descends by the per-node UTF-16 surplus: emits in a non-ASCII file would
    /// otherwise each walk the whole file.
    pub fn utf16_offset(&self, n: NodeId, byte_offset: usize) -> usize {
        if self.is_ascii() {
            return byte_offset;
        }
        let mut cur = self.tree_root(n);
        let byte_offset = byte_offset.min(self.text_length(cur));
        let (mut rest, mut surplus) = (byte_offset, 0);
        'descend: while rest > 0 {
            if self.is_leaf_element(cur) {
                surplus += self.leaf_text(cur)[..rest].chars().map(|c| c.len_utf8() - c.len_utf16()).sum::<usize>();
                break;
            }
            let mut child = self.first_child_node(cur);
            while let Some(c) = child {
                if rest < self.text_length(c) {
                    cur = c;
                    continue 'descend;
                }
                rest -= self.text_length(c);
                surplus += self.node(c).surplus as usize;
                child = self.tree_next(c);
            }
            break;
        }
        byte_offset - surplus
    }
}

/// UTF-8 minus UTF-16 length of `text`.
pub(crate) fn utf16_surplus(text: &str) -> u32 {
    if text.is_ascii() {
        return 0;
    }
    text.chars().map(|c| (c.len_utf8() - c.len_utf16()) as u32).sum()
}

pub(crate) fn newline_count(text: &str) -> u32 {
    text.bytes().filter(|&b| b == b'\n').count() as u32
}
