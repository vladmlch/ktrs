//! `ASTNodeExtension.kt` from `column` to `lineLength`: columns, indents, leaf ranges and line contents.
//! Columns and lengths count UTF-16 units, as the JVM's `textLength`/`String.length` do.

use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::EOL_COMMENT;

use super::{AstNodeExtension, indent_internal};

pub trait AstNodeLines {
    fn column(&self, n: NodeId) -> usize;
    fn indent_without_newline_prefix(&self, n: NodeId) -> String;
    fn has_new_line_in_closed_range(&self, from: NodeId, to: NodeId) -> bool;
    fn no_new_line_in_closed_range(&self, from: NodeId, to: NodeId) -> bool;
    fn no_new_line_in_open_range(&self, from: NodeId, to: NodeId) -> bool;
    fn leaves_in_open_range(&self, from: NodeId, to: NodeId) -> impl Iterator<Item = NodeId> + '_;
    fn leaves_in_closed_range(&self, from: NodeId, to: NodeId) -> impl Iterator<Item = NodeId> + '_;
    fn leaves_forwards_including_self(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_;
    fn leaves_backwards_including_self(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_;
    fn leaves_on_line(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_;
    /// `Sequence<ASTNode>.dropTrailingEolComment()`.
    fn drop_trailing_eol_comment<'a>(&'a self, leaves: impl Iterator<Item = NodeId> + 'a) -> impl Iterator<Item = NodeId> + 'a;
    fn first_leaf_on_line_or_self(&self, n: NodeId) -> NodeId;
    fn last_leaf_on_line_or_null(&self, n: NodeId) -> Option<NodeId>;
    /// `Sequence<ASTNode>.lineLength`.
    fn line_length(&self, leaves: impl IntoIterator<Item = NodeId>) -> usize;
}

impl AstNodeLines for Ast {
    fn column(&self, n: NodeId) -> usize {
        let mut leaf = self.prev_leaf(n);
        let mut offset_to_the_left = 0;
        while let Some(l) = leaf {
            if self.is_white_space_with_newline(l) {
                let text = self.leaf_text(l);
                let after_last_newline = &text[text.rfind('\n').map_or(0, |i| i + 1)..];
                offset_to_the_left += after_last_newline.encode_utf16().count();
                break;
            }
            offset_to_the_left += self.text_length_utf16(l);
            leaf = self.prev_leaf(l);
        }
        offset_to_the_left + 1
    }

    fn indent_without_newline_prefix(&self, n: NodeId) -> String {
        let indent = indent_internal(self, n);
        indent.strip_prefix('\n').unwrap_or(indent).to_owned()
    }

    fn has_new_line_in_closed_range(&self, from: NodeId, to: NodeId) -> bool {
        self.leaves_in_closed_range(from, to).any(|it| self.text_contains(it, '\n'))
    }

    fn no_new_line_in_closed_range(&self, from: NodeId, to: NodeId) -> bool {
        !self.leaves_in_closed_range(from, to).any(|it| self.text_contains(it, '\n'))
    }

    fn no_new_line_in_open_range(&self, from: NodeId, to: NodeId) -> bool {
        !self.leaves_in_open_range(from, to).any(|it| self.text_contains(it, '\n'))
    }

    /// psiUtil `from.leaves()` (the leaves after `from`) up to `to` or `to`'s last leaf, both excluded.
    fn leaves_in_open_range(&self, from: NodeId, to: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let to_last_leaf = self.last_child_leaf_or_self(to);
        self.leaves(from, true).take_while(move |&it| it != to && it != to_last_leaf)
    }

    fn leaves_in_closed_range(&self, from: NodeId, to: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let stop_at_leaf = self.next_leaf(self.last_child_leaf_or_self(to));
        self.leaves_forwards_including_self(self.first_child_leaf_or_self(from)).take_while(move |&it| Some(it) != stop_at_leaf)
    }

    /// `n` itself when it has no children (`isLeaf`, so also an empty composite), then psiUtil `leaves(true)`.
    fn leaves_forwards_including_self(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.is_leaf(n).then_some(n).into_iter().chain(self.leaves(n, true))
    }

    fn leaves_backwards_including_self(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.is_leaf(n).then_some(n).into_iter().chain(self.leaves(n, false))
    }

    /// The leaves of the line `n` starts on (the first one is the newline whitespace before it, if any).
    fn leaves_on_line(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let last_leaf_on_line_or_null = self.last_leaf_on_line_or_null(n);
        let take_all = last_leaf_on_line_or_null.is_none();
        self.leaves_forwards_including_self(self.first_leaf_on_line_or_self(n))
            .take_while(move |&it| take_all || self.prev_leaf(it) != last_leaf_on_line_or_null)
    }

    fn drop_trailing_eol_comment<'a>(&'a self, leaves: impl Iterator<Item = NodeId> + 'a) -> impl Iterator<Item = NodeId> + 'a {
        leaves.take_while(move |&it| {
            !(self.is_white_space_without_newline(it) && self.next_leaf(it).is_some_and(|n| self.element_type(n) == EOL_COMMENT))
                && self.element_type(it) != EOL_COMMENT
        })
    }

    fn first_leaf_on_line_or_self(&self, n: NodeId) -> NodeId {
        self.prev_leaf_matching(n, |it| (self.text_contains(it, '\n') && !self.is_part_of_comment(it)) || self.prev_leaf(it).is_none())
            .unwrap_or(n)
    }

    fn last_leaf_on_line_or_null(&self, n: NodeId) -> Option<NodeId> {
        self.next_leaf_matching(n, |it| self.text_contains(it, '\n')).and_then(|it| self.prev_leaf(it))
    }

    /// The length of the first non-empty line of the leaves' joined text. Panics `IllegalArgumentException`
    /// unless the first leaf contains a newline or starts the file.
    fn line_length(&self, leaves: impl IntoIterator<Item = NodeId>) -> usize {
        let mut leaves = leaves.into_iter().peekable();
        let Some(&first) = leaves.peek() else { return 0 };
        assert!(
            self.text_contains(first, '\n') || self.prev_leaf(first).is_none(),
            "IllegalArgumentException: First node in non-empty sequence must be a whitespace containing a newline"
        );
        // Streams `joinToString("").trimStart('\n').substringBefore('\n').length` without building the text.
        let (mut leading, mut length) = (true, 0);
        for leaf in leaves {
            if !leading && self.newline_count(leaf) == 0 {
                length += self.text_length_utf16(leaf);
                continue;
            }
            if leading && self.newline_count(leaf) == 0 {
                if self.text_length(leaf) > 0 {
                    leading = false;
                    length += self.text_length_utf16(leaf);
                }
                continue;
            }
            let process_chunk = |chunk: &str, leading: &mut bool, length: &mut usize| -> Option<usize> {
                if *leading {
                    let trimmed = chunk.trim_start_matches('\n');
                    if trimmed.is_empty() {
                        return None;
                    }
                    *leading = false;
                    if let Some(pos) = trimmed.find('\n') {
                        return Some(*length + utf16_len(&trimmed[..pos]));
                    }
                    *length += utf16_len(trimmed);
                } else if let Some(pos) = chunk.find('\n') {
                    return Some(*length + utf16_len(&chunk[..pos]));
                } else {
                    *length += utf16_len(chunk);
                }
                None
            };
            if self.is_leaf_element(leaf) {
                if let Some(res) = process_chunk(self.leaf_text(leaf), &mut leading, &mut length) {
                    return res;
                }
            } else {
                for chunk in self.text_chunks(leaf) {
                    if let Some(res) = process_chunk(chunk, &mut leading, &mut length) {
                        return res;
                    }
                }
            }
        }
        length
    }
}

#[inline]
fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.encode_utf16().count()
    }
}
