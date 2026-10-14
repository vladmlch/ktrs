use std::cell::{Cell, RefCell};

use ktrs_syntax::{Parse, SyntaxKind, Tree};

use crate::text::{newline_count, utf16_surplus};

/// An `ASTNode` reference. Ids are never reused, so a detached node stays addressable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub(crate) u32);

pub(crate) const NONE: u32 = u32::MAX;
const LEAF: u8 = 1;
/// `instanceof FileElement`: the file root and every `DummyHolder`'s tree element.
const FILE_ELEMENT: u8 = 2;

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: SyntaxKind,
    flags: u8,
    pub(crate) parent: u32,
    pub(crate) first: u32,
    pub(crate) last: u32,
    pub(crate) prev: u32,
    pub(crate) next: u32,
    /// Leaf: start of its text in `Ast::text`. `ERROR_ELEMENT`: index into `Ast::errors`.
    data: u32,
    /// Text length; a composite's is the sum over its children, kept current on every relink.
    pub(crate) len: u32,
    /// UTF-8 minus UTF-16 length of the text, kept like `len`: UTF-16 offsets and lengths without a walk.
    pub(crate) surplus: u32,
    /// `\n` count of the text, kept like `len`: `textContains('\n')` without a walk.
    pub(crate) newlines: u32,
    /// Java `String.hashCode()` of the text, lazily filled; `u32::MAX` = not computed. A cached node's
    /// hash is current and an uncached node's ancestors are uncached: every link change clears its parent chain.
    pub(crate) text_hash: Cell<u32>,
    /// `myStartOffsetInParent`, lazily filled; `NONE` = not computed. The valid entries of a child
    /// list are always a prefix of it, as in IntelliJ.
    pub(crate) offset_in_parent: Cell<u32>,
}

const _: () = assert!(std::mem::size_of::<Node>() == 48);

/// The arena. Navigation takes `&self` and never allocates; edits take `&mut self`.
#[derive(Debug)]
pub struct Ast {
    pub(crate) nodes: Vec<Node>,
    /// Append-only leaf text: the seeded source first, then the text of every new leaf.
    text: String,
    errors: Vec<String>,
    root: NodeId,
    ascii: bool,
    psi_file_name: String,
    /// Bumped by every edit: `node_mut` and `push` are the only writers of links, lengths and leaves.
    modification_count: u64,
    /// Per needle of `allocated_leaf_text_contains`: bytes of `text` scanned, and whether it was found.
    pub(crate) needle_scans: RefCell<Vec<(&'static str, usize, bool)>>,
}

pub(crate) fn opt(raw: u32) -> Option<NodeId> {
    (raw != NONE).then_some(NodeId(raw))
}

pub(crate) fn raw(node: Option<NodeId>) -> u32 {
    node.map_or(NONE, |n| n.0)
}

impl Ast {
    /// The tree of a parsed file (`text` already LF-normalized), rooted at a `FILE` file element.
    pub fn from_parse(parse: &Parse) -> Ast {
        let mut ast = Ast {
            nodes: Vec::with_capacity(parse.tree.len()),
            text: String::with_capacity(parse.tree.text().len()),
            errors: Vec::new(),
            root: NodeId(0),
            ascii: true,
            psi_file_name: "File.kt".to_owned(),
            modification_count: 0,
            needle_scans: RefCell::default(),
        };
        ast.root = ast.seed(parse);
        ast
    }

    /// Appends `parse`'s whole tree as a new detached file element and returns its root.
    pub(crate) fn seed(&mut self, parse: &Parse) -> NodeId {
        let tree: &Tree = &parse.tree;
        self.modification_count += 1;
        let base = self.nodes.len() as u32;
        let text_base = self.text.len() as u32;
        self.text.push_str(tree.text());
        let ascii = tree.text().is_ascii();
        self.ascii &= ascii;
        let mut error = self.errors.len() as u32;
        self.errors.extend(parse.error_messages.iter().cloned());
        let mut last_leaf_id = NONE;
        for e in 0..tree.len() as u32 {
            let id = base + e;
            let range = tree.text_range(e);
            let start: u32 = range.start().into();
            let kind = tree.kind(e);
            let (flags, data) = if tree.is_token(e) {
                (LEAF, text_base + start)
            } else if kind == SyntaxKind::ERROR_ELEMENT {
                error += 1;
                (0, error - 1)
            } else {
                (0, 0)
            };
            let (parent, offset) = match tree.parent(e) {
                Some(p) => (base + p, start - u32::from(tree.text_range(p).start())),
                None => (NONE, NONE),
            };
            let prev = tree.prev_sibling(e).map_or(NONE, |p| base + p);
            let (surplus, newlines) = if flags == LEAF {
                let text = &tree.text()[range];
                (if ascii { 0 } else { utf16_surplus(text) }, newline_count(text))
            } else {
                (0, 0)
            };
            let (first, last) = if flags == LEAF {
                (last_leaf_id, NONE)
            } else {
                (NONE, NONE)
            };
            self.nodes.push(Node {
                kind,
                flags,
                parent,
                first,
                last,
                prev,
                next: NONE,
                data,
                len: range.len().into(),
                surplus,
                newlines,
                text_hash: Cell::new(NONE),
                offset_in_parent: Cell::new(offset),
            });
            if flags == LEAF {
                if last_leaf_id != NONE {
                    self.nodes[last_leaf_id as usize].last = id;
                }
                last_leaf_id = id;
            }
            if prev != NONE {
                self.nodes[prev as usize].next = id;
            } else if parent != NONE {
                self.nodes[parent as usize].first = id;
            }
            if parent != NONE {
                self.nodes[parent as usize].last = id;
            }
        }
        for e in (base as usize + 1..self.nodes.len()).rev() {
            let parent = self.nodes[e].parent;
            if parent != NONE {
                let surplus = self.nodes[e].surplus;
                let newlines = self.nodes[e].newlines;
                self.nodes[parent as usize].surplus += surplus;
                self.nodes[parent as usize].newlines += newlines;
            }
        }
        let root = &mut self.nodes[base as usize];
        root.kind = SyntaxKind::FILE;
        root.flags = FILE_ELEMENT;
        NodeId(base)
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// The `psiFileName` of `createPsiFileFromText(psiFileName, text)`: the file's path, or `File.kt`/`File.kts`.
    /// It is the root's `LightVirtualFile` name ([`crate::psi::KtFile::virtual_file_path`]).
    pub fn psi_file_name(&self) -> &str {
        &self.psi_file_name
    }

    pub fn set_psi_file_name(&mut self, psi_file_name: &str) {
        psi_file_name.clone_into(&mut self.psi_file_name);
    }

    /// Number of nodes ever allocated (attached or not).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The nodes allocated after the first `start` (attached or not); a node's type never changes.
    pub fn nodes_allocated_since(&self, start: usize) -> impl Iterator<Item = NodeId> + use<> {
        (start as u32..self.nodes.len() as u32).map(NodeId)
    }

    pub(crate) fn node(&self, n: NodeId) -> &Node {
        &self.nodes[n.0 as usize]
    }

    pub(crate) fn node_mut(&mut self, n: NodeId) -> &mut Node {
        self.modification_count += 1;
        &mut self.nodes[n.0 as usize]
    }

    /// Changes whenever the tree or any leaf does; equal counts mean an identical tree.
    pub fn modification_count(&self) -> u64 {
        self.modification_count
    }

    /// The text of every leaf ever allocated, in allocation order (append-only; a leaf's text is one
    /// contiguous slice of it). Lets a caller cheaply ask whether any leaf could contain a word.
    pub fn allocated_leaf_text(&self) -> &str {
        &self.text
    }

    pub fn element_type(&self, n: NodeId) -> SyntaxKind {
        self.node(n).kind
    }

    pub fn tree_parent(&self, n: NodeId) -> Option<NodeId> {
        opt(self.node(n).parent)
    }

    pub fn tree_next(&self, n: NodeId) -> Option<NodeId> {
        opt(self.node(n).next)
    }

    pub fn tree_prev(&self, n: NodeId) -> Option<NodeId> {
        opt(self.node(n).prev)
    }

    pub fn first_child_node(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            None
        } else {
            opt(self.node(n).first)
        }
    }

    pub fn last_child_node(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            None
        } else {
            opt(self.node(n).last)
        }
    }

    #[inline]
    pub fn leaf_prev(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            opt(self.node(n).first)
        } else {
            None
        }
    }

    #[inline]
    pub fn leaf_next(&self, n: NodeId) -> Option<NodeId> {
        if self.is_leaf_element(n) {
            opt(self.node(n).last)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) fn set_leaf_prev(&mut self, n: NodeId, prev: u32) {
        debug_assert!(self.is_leaf_element(n));
        self.node_mut(n).first = prev;
    }

    #[inline]
    pub(crate) fn set_leaf_next(&mut self, n: NodeId, next: u32) {
        debug_assert!(self.is_leaf_element(n));
        self.node_mut(n).last = next;
    }

    /// `node instanceof LeafElement` (a composite without children is not a leaf).
    pub fn is_leaf_element(&self, n: NodeId) -> bool {
        self.node(n).flags & LEAF != 0
    }

    /// `node instanceof FileElement` (`FileASTNode`): the file root or a dummy holder.
    pub fn is_file_element(&self, n: NodeId) -> bool {
        self.node(n).flags & FILE_ELEMENT != 0
    }

    pub fn text_length(&self, n: NodeId) -> usize {
        self.node(n).len as usize
    }

    pub fn newline_count(&self, n: NodeId) -> u32 {
        self.node(n).newlines
    }

    /// A leaf's text (`LeafElement.getChars`). Panics on a composite.
    pub fn leaf_text(&self, n: NodeId) -> &str {
        let node = self.node(n);
        assert!(node.flags & LEAF != 0, "leaf_text of composite {:?}", node.kind);
        &self.text[node.data as usize..(node.data + node.len) as usize]
    }

    /// `PsiErrorElement.getErrorDescription()` of an `ERROR_ELEMENT` from the parser ("" otherwise).
    pub fn error_description(&self, n: NodeId) -> &str {
        let node = self.node(n);
        match node.kind {
            SyntaxKind::ERROR_ELEMENT if node.flags & LEAF == 0 => self.errors.get(node.data as usize).map_or("", String::as_str),
            _ => "",
        }
    }

    /// Whether every leaf's text is ASCII, so byte offsets and lengths equal UTF-16 ones.
    pub fn is_ascii(&self) -> bool {
        self.ascii
    }

    /// A detached leaf (`new LeafPsiElement(type, text)`, `new PsiWhiteSpaceImpl(text)`, `ASTFactory.leaf`).
    pub fn new_leaf(&mut self, kind: SyntaxKind, text: &str) -> NodeId {
        let data = self.text.len() as u32;
        self.text.push_str(text);
        self.ascii &= text.is_ascii();
        self.push(kind, LEAF, data, (text.len() as u32, utf16_surplus(text), newline_count(text)))
    }

    /// A detached, empty composite (`new KtBlockExpression(null)` and the like).
    pub fn new_composite(&mut self, kind: SyntaxKind) -> NodeId {
        self.push(kind, 0, 0, (0, 0, 0))
    }

    /// `DummyHolderFactory.createHolder(...).getTreeElement()`.
    pub(crate) fn new_dummy_holder(&mut self) -> NodeId {
        self.push(SyntaxKind::DUMMY_HOLDER, FILE_ELEMENT, 0, (0, 0, 0))
    }

    /// A detached copy of `n`'s node record without links (`TreeElement.clone`'s shallow part).
    pub(crate) fn push_unlinked_copy(&mut self, n: NodeId) -> NodeId {
        let Node { kind, flags, data, len, surplus, newlines, .. } = *self.node(n);
        self.push(kind, flags, data, if flags & LEAF != 0 { (len, surplus, newlines) } else { (0, 0, 0) })
    }

    /// A detached node with the text metrics `(len, surplus, newlines)`.
    fn push(&mut self, kind: SyntaxKind, flags: u8, data: u32, (len, surplus, newlines): (u32, u32, u32)) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.modification_count += 1;
        self.nodes.push(Node {
            kind,
            flags,
            parent: NONE,
            first: NONE,
            last: NONE,
            prev: NONE,
            next: NONE,
            data,
            len,
            surplus,
            newlines,
            text_hash: Cell::new(NONE),
            offset_in_parent: Cell::new(NONE),
        });
        id
    }
}
