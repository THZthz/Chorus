//! The static term matcher: which card declarations can match a query term list.

use std::collections::BTreeSet;

use story_content::ir::{AspectKey, CardDecl, CmpOp, EffectNode, QueryTerm};
use story_content::Content;
use story_core::CardId;

use super::visit_content_effects;

/// The static term matcher: which card declarations can match a query term
/// list in some live state. Sound over-approximation — `Pred` bodies admit
/// every card, negated terms admit everything not guaranteed by the
/// declaration alone, and an aspect term admits any card whose key some
/// `set` can write.
pub(crate) struct MatchOracle {
    written_keys: BTreeSet<AspectKey>,
}

impl MatchOracle {
    pub(crate) fn new(content: &Content) -> MatchOracle {
        let mut written_keys = BTreeSet::new();
        visit_content_effects(content, &mut |n| {
            if let EffectNode::Set { key, .. } = n {
                written_keys.insert(key.clone());
            }
        });
        MatchOracle { written_keys }
    }

    /// Every card declaration that can match the term list somewhere.
    pub(crate) fn can_match_cards(&self, content: &Content, terms: &[QueryTerm]) -> Vec<CardId> {
        content
            .cards
            .values()
            .filter(|decl| self.can_match(content, terms, decl))
            .map(|decl| decl.id.clone())
            .collect()
    }

    /// Whether one declaration can match the term list.
    pub(crate) fn can_match(
        &self,
        content: &Content,
        terms: &[QueryTerm],
        decl: &CardDecl,
    ) -> bool {
        terms
            .iter()
            .all(|t| self.term_can_hold(content, t, decl, false))
    }

    /// The cards whose liveness is NECESSARY for a target over these terms
    /// to resolve: declared values only, no mutation escape. Negated terms
    /// and inlined predicates are disjunctive — no single card's liveness is
    /// necessary — so they contribute nothing (a missing requirement only
    /// ever answers "plausible", never "impossible").
    pub(crate) fn requirement_cards(&self, content: &Content, terms: &[QueryTerm]) -> Vec<CardId> {
        if terms
            .iter()
            .any(|t| matches!(t, QueryTerm::Not(_) | QueryTerm::Pred(_)))
        {
            return Vec::new();
        }
        content
            .cards
            .values()
            .filter(|decl| {
                terms
                    .iter()
                    .all(|t| self.term_can_hold(content, t, decl, true))
            })
            .map(|decl| decl.id.clone())
            .collect()
    }

    /// Whether one declaration must match the term list in every state
    /// (used only to admit negated terms liberally).
    fn term_must_hold(&self, content: &Content, t: &QueryTerm, decl: &CardDecl) -> bool {
        self.term_can_hold(content, t, decl, true)
    }

    fn term_can_hold(
        &self,
        content: &Content,
        t: &QueryTerm,
        decl: &CardDecl,
        strict: bool,
    ) -> bool {
        match t {
            QueryTerm::Aspect(a) => {
                let value =
                    decl.aspects.get(&a.key).copied().unwrap_or_else(|| {
                        content.aspects.get(&a.key).map(|d| d.default).unwrap_or(0)
                    });
                let holds = match a.op {
                    None => value >= 1,
                    Some(CmpOp::Ge) => value >= a.value,
                    Some(CmpOp::Le) => value <= a.value,
                    Some(CmpOp::Gt) => value > a.value,
                    Some(CmpOp::Lt) => value < a.value,
                    Some(CmpOp::Eq) => value == a.value,
                    Some(CmpOp::Ne) => value != a.value,
                };
                holds || (!strict && self.written_keys.contains(&a.key))
            }
            QueryTerm::Name(c) => decl.id == *c,
            QueryTerm::Kind(k) => decl.kind.as_deref() == Some(k.as_str()),
            QueryTerm::Not(inner) => !self.term_must_hold(content, inner, decl),
            // An inlined predicate body reads like an arbitrary expression
            // over the candidate: admit every card liberally, nothing
            // strictly (a requirement over a predicate is disjunctive).
            QueryTerm::Pred(_) => !strict,
        }
    }
}
