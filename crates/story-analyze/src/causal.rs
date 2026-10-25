//! Tier 1 — the `CausalGraph` static analyzer (story-analyze spec §4): a
//! producer/consumer hypergraph over an abstract fact domain, extracted from
//! compiled content without touching any `State`. Recipe effect lists, their
//! completion books, card-carried books, expiry books, event books, decks,
//! and boot instantiations become hyperedges from requirement facts to
//! produced facts; backward chaining over them answers "what can produce X"
//! and proves structural unreachability.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{Book, CardDecl, EffectNode, Expr, ZoneName};
use story_content::Content;
use story_core::CardId;

mod analysis;
mod build;
mod fact;
mod oracle;

pub use analysis::goal_facts;
pub use fact::{Fact, Hyperedge, Plausibility, Provenance, Site};
pub(crate) use oracle::MatchOracle;

use build::Builder;

// ---------------------------------------------------------------------------
// The graph
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct CausalGraph {
    edges: Vec<Hyperedge>,
    producers: BTreeMap<Fact, Vec<usize>>,
    consumers: BTreeMap<Fact, Vec<usize>>,
    card_declared_aspects: BTreeMap<CardId, Vec<story_content::ir::AspectKey>>,
    zone_facts: Vec<(CardId, ZoneName)>,
    host_opaque: bool,
    grounded: BTreeSet<Fact>,
    pub(crate) indexed: analysis::IndexedCausal,
}

impl CausalGraph {
    /// Extract the hypergraph from compiled content.
    pub fn build(content: &Content) -> CausalGraph {
        let _span = tracing::info_span!("causal_build").entered();
        let mut builder = Builder::new(content);
        builder.boot_state();
        builder.boot_cards();
        builder.decks();
        {
            let _span = tracing::info_span!("causal_recipes").entered();
            builder.recipes();
        }
        {
            let _span = tracing::info_span!("causal_card_books").entered();
            builder.card_books();
        }
        {
            let _span = tracing::info_span!("causal_expiry_books").entered();
            builder.expiry_books();
        }
        {
            let _span = tracing::info_span!("causal_event_books").entered();
            builder.event_books();
        }
        let mut graph = {
            let _span = tracing::info_span!("causal_finish").entered();
            builder.finish()
        };
        visit_content_effects(content, &mut |n| {
            if matches!(n, EffectNode::Op { .. }) {
                graph.host_opaque = true;
            }
        });
        graph.grounded = graph.grounded_set();
        graph.indexed = analysis::IndexedCausal::new(
            &graph.edges,
            content,
            &graph.card_declared_aspects,
            &graph.zone_facts,
        );
        graph
    }

    /// The extracted hyperedges, in build order.
    pub fn edges(&self) -> &[Hyperedge] {
        &self.edges
    }

    /// Every fact produced by at least one hyperedge in the causal graph.
    pub fn facts(&self) -> impl Iterator<Item = &Fact> {
        self.producers.keys()
    }

    /// Whether any effect list references a host-registered `Op`; content
    /// relying on them degrades to Tier-2-only guarantees.
    pub fn host_opaque(&self) -> bool {
        self.host_opaque
    }

    /// The cards some runtime transition site (recipe, completion book,
    /// expiry book, or event book) can bring back live. Boot and bare deck
    /// sites are excluded: boot happens once at t=0, and deck draws already
    /// ride the drawing site's edge.
    pub fn recipe_mintable_cards(&self) -> BTreeSet<CardId> {
        let mut out = BTreeSet::new();
        for edge in &self.edges {
            if !matches!(edge.site, Site::Boot(_) | Site::BootState | Site::Deck(_)) {
                for f in &edge.produces {
                    if let Fact::CardLive(card) = f {
                        out.insert(card.clone());
                    }
                }
            }
        }
        out
    }

    /// Indexes of the edges that can produce the fact. An increase producer
    /// of `(card, key)` also produces `CardHasAspect(card, key)` — raising
    /// the value from the declared default makes the aspect hold.
    pub fn producers_of(&self, fact: &Fact) -> Vec<usize> {
        let mut out = self.producers.get(fact).cloned().unwrap_or_default();
        if let Fact::CardHasAspect(card, key) = fact {
            for unclamped in [false, true] {
                let polarity = Fact::AspectCanIncrease {
                    card: Some(card.clone()),
                    key: key.clone(),
                    unclamped,
                };
                out.extend(self.producers.get(&polarity).cloned().unwrap_or_default());
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Indexes of the edges that require the fact.
    pub fn consumers_of(&self, fact: &Fact) -> &[usize] {
        self.consumers
            .get(fact)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The plausibility pre-check for Tier 2 (story-analyze spec §4 output
    /// 2): reject queries whose required facts are statically impossible
    /// before any search.
    pub fn plausibility(&self, goal: &Fact) -> Plausibility {
        if self.grounded.contains(goal) {
            Plausibility::Plausible
        } else if self.host_opaque {
            Plausibility::UnknownHost
        } else {
            Plausibility::StaticallyImpossible
        }
    }

    /// Backward provenance (story-analyze spec §4 output 1): a chain from the
    /// goal's required fact through a capable edge and its requirements,
    /// grounding in the boot state. `None` when no grounding cone exists.
    pub fn explain_how_to_get(&self, goal: &Fact) -> Option<Provenance> {
        if !self.grounded.contains(goal) {
            return None;
        }
        let mut in_progress = BTreeSet::new();
        self.chain(goal, &mut in_progress)
    }

    /// The fixpoint of facts some edge sequence grounds from the boot state:
    /// an edge's products ground when every requirement has already grounded.
    /// Raising an aspect grounds the aspect's presence (`producers_of`'s
    /// bridging, mirrored here): a raised value reads >= 1.
    fn grounded_set(&self) -> BTreeSet<Fact> {
        let _span = tracing::info_span!("causal_grounded").entered();
        let mut grounded: BTreeSet<Fact> = BTreeSet::new();
        let mut unsatisfied: Vec<usize> = self.edges.iter().map(|e| e.requires.len()).collect();
        let mut queue: Vec<Fact> = Vec::new();

        for edge in &self.edges {
            if edge.requires.is_empty() {
                for p in &edge.produces {
                    queue.push(p.clone());
                }
            }
        }

        while let Some(fact) = queue.pop() {
            if !grounded.insert(fact.clone()) {
                continue;
            }
            if let Fact::AspectCanIncrease {
                card: Some(card),
                key,
                ..
            } = &fact
            {
                let bridged = Fact::CardHasAspect(card.clone(), key.clone());
                if !grounded.contains(&bridged) {
                    queue.push(bridged);
                }
            }
            if let Some(edge_indices) = self.consumers.get(&fact) {
                for &ei in edge_indices {
                    unsatisfied[ei] -= 1;
                    if unsatisfied[ei] == 0 {
                        for p in &self.edges[ei].produces {
                            if !grounded.contains(p) {
                                queue.push(p.clone());
                            }
                        }
                    }
                }
            }
        }

        grounded
    }

    /// Depth-first provenance witness over grounded facts; the in-progress
    /// set keeps the chain acyclic (a fact grounded only through itself is
    /// refused, which the fixpoint already ruled out for the goal).
    fn chain(&self, fact: &Fact, in_progress: &mut BTreeSet<Fact>) -> Option<Provenance> {
        if !in_progress.insert(fact.clone()) {
            return None;
        }
        let mut found = None;
        for &ei in &self.producers_of(fact) {
            let edge = &self.edges[ei];
            let mut requires = Vec::new();
            let mut complete = true;
            for r in &edge.requires {
                match self.chain(r, in_progress) {
                    Some(p) => requires.push(p),
                    None => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                found = Some(Provenance {
                    fact: fact.clone(),
                    site: edge.site.clone(),
                    requires,
                });
                break;
            }
        }
        in_progress.remove(fact);
        found
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The mint template of a card: the aliased card's body when the declaration
/// carries `mint <card>` (the kernel's `Engine::template`, statically).
pub(crate) fn template_of<'a>(content: &'a Content, card: &CardId) -> Option<&'a CardDecl> {
    let mut cur = card;
    let mut seen = BTreeSet::new();
    loop {
        let decl = content.card(cur)?;
        match &decl.mint_alias {
            Some(next) if seen.insert(cur.clone()) => cur = next,
            _ => return Some(decl),
        }
    }
}

/// A count expression with a statically known non-negative value; anything
/// else (dynamic) can fail at effect time.
fn is_static_count(e: &Expr) -> bool {
    matches!(story_content::ir::const_eval(e), Some(n) if n >= 0)
}

/// Walk every effect list in the content (recipes, books, cards, events).
pub(crate) fn visit_content_effects(content: &Content, f: &mut impl FnMut(&EffectNode)) {
    fn walk_list(nodes: &[EffectNode], f: &mut impl FnMut(&EffectNode)) {
        for n in nodes {
            walk_node(n, f);
        }
    }
    fn walk_node(n: &EffectNode, f: &mut impl FnMut(&EffectNode)) {
        f(n);
        if let EffectNode::Branch { then, els, .. } = n {
            walk_list(then, f);
            walk_list(els, f);
        }
    }
    fn walk_book(b: &Book, f: &mut impl FnMut(&EffectNode)) {
        for r in &b.rules {
            walk_list(&r.effects, f);
        }
    }
    for v in content.verbs.values() {
        for r in v.recipes.values() {
            walk_list(&r.effects, f);
            if let Some(b) = &r.on_complete {
                walk_book(b, f);
            }
        }
    }
    for c in content.cards.values() {
        for (_, b) in &c.on_complete {
            walk_book(b, f);
        }
        if let Some(b) = &c.on_expire {
            walk_book(b, f);
        }
    }
    for b in content.events.values() {
        walk_book(b, f);
    }
}
