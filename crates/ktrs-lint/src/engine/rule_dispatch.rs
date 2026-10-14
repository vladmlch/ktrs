//! Which rules a traversal calls `before_visit_child_nodes` on, per element type: precomputed in
//! rule order, so a node walks only its own rules instead of testing every rule's `visited_types`.

use ktrs_syntax::SyntaxKind;

use crate::rule::TokenSet;

pub(crate) struct RuleDispatch {
    /// Per element type: the `(start, len)` of its rule indices in `indices`.
    spans: Vec<(u32, u32)>,
    indices: Vec<u16>,
    /// The rules with an `after_visit_child_nodes`, in rule order (a few of all).
    pub(crate) after: Vec<u16>,
}

impl RuleDispatch {
    /// Precomputes dispatch for all syntax kinds once for the given rules.
    pub(crate) fn build(rules: impl IntoIterator<Item = (Option<TokenSet>, bool)>) -> RuleDispatch {
        let rules: Vec<(Option<TokenSet>, bool)> = rules.into_iter().collect();
        let rule_count = rules.len();
        let mut spans = vec![(0u32, 0u32); SyntaxKind::DUMMY_HOLDER as usize + 1];
        let mut indices = Vec::with_capacity(rule_count * 32);
        let after: Vec<u16> = rules
            .iter()
            .enumerate()
            .filter(|&(_, &(_, visits_after))| visits_after)
            .map(|(i, _)| i as u16)
            .collect();

        for raw in 0..=SyntaxKind::DUMMY_HOLDER as u16 {
            let kind = SyntaxKind::from_raw(raw);
            let start = indices.len();
            for (i, &(visited_types, _)) in rules.iter().enumerate() {
                if visited_types.is_none_or(|types| types.contains(kind)) {
                    indices.push(i as u16);
                }
            }
            spans[raw as usize] = (start as u32, (indices.len() - start) as u32);
        }

        indices.shrink_to_fit();
        RuleDispatch {
            spans,
            indices,
            after,
        }
    }

    /// The indices of the rules whose `visited_types` include `kind`.
    #[inline]
    pub(crate) fn rules_for(&self, kind: SyntaxKind) -> &[u16] {
        let (start, len) = self.spans[kind as usize];
        &self.indices[start as usize..(start + len) as usize]
    }
}
