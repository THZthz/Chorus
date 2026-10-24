//! Tier 1 — the `CausalGraph` static analyzer (story-analyze spec §4): a
//! producer/consumer hypergraph over an abstract fact domain, extracted from
//! compiled content without touching any `State`. Recipe effect lists, their
//! completion books, card-carried books, expiry books, event books, decks,
//! and boot instantiations become hyperedges from requirement facts to
//! produced facts; backward chaining over them answers "what can produce X"
//! and proves structural unreachability.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{
    AspectKey, Book, CardDecl, CmpOp, DestroyCount, EffectNode, Expr, HandleName, Into, LegacyKey,
    QueryTerm, RecipeDecl, SetOp, Target, ZoneName,
};
use story_content::Content;
use story_core::{CardId, DeckId, EndingId, RecipeId, VerbId};

// ---------------------------------------------------------------------------
// The fact domain (story-analyze spec §4)
// ---------------------------------------------------------------------------

/// One node of the fact domain. Every variant is a capability question about
/// reachable states, never a statement about one state.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fact {
    /// A live instance of the card can exist.
    CardLive(CardId),
    /// A live instance of the card can sit in the zone.
    CardInZone(CardId, ZoneName),
    /// The card can hold the aspect key (boot declarations and increase
    /// producers ground it; see `CausalGraph::producers_of`).
    CardHasAspect(CardId, AspectKey),
    /// Signed polarity: some effect can raise the aspect's value. `unclamped`
    /// records whether the write may pass below 0 (`set`/`legacy` clamp at 0
    /// unless written `unclamped`), so upper-bound preconditions chain only
    /// through decrease producers.
    AspectCanIncrease {
        card: Option<CardId>,
        key: AspectKey,
        unclamped: bool,
    },
    /// Signed polarity: some effect can lower the aspect's value.
    AspectCanDecrease {
        card: Option<CardId>,
        key: AspectKey,
        unclamped: bool,
    },
    /// Some effect can write the legacy key.
    LegacyCanChange(LegacyKey),
    /// An instance of the recipe can open.
    RecipeCanOpen(RecipeId),
    /// An opened instance of the recipe can run to completion.
    RecipeCanComplete(RecipeId),
    /// The event book can be ignited.
    EventSignaled(String),
    /// The ending can be reached.
    EndingReached(EndingId),
}

impl std::fmt::Display for Fact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CardLive(c) => write!(f, "card \"{c}\" live"),
            Self::CardInZone(c, z) => write!(f, "card \"{c}\" in zone \"{z}\""),
            Self::CardHasAspect(c, k) => write!(f, "card \"{c}\" has aspect \"{k}\""),
            Self::AspectCanIncrease {
                card: Some(c), key, ..
            } => write!(f, "aspect \"{c}.{key}\" can increase"),
            Self::AspectCanIncrease { card: None, key, .. } => {
                write!(f, "aspect \"{key}\" can increase")
            }
            Self::AspectCanDecrease {
                card: Some(c), key, ..
            } => write!(f, "aspect \"{c}.{key}\" can decrease"),
            Self::AspectCanDecrease { card: None, key, .. } => {
                write!(f, "aspect \"{key}\" can decrease")
            }
            Self::LegacyCanChange(k) => write!(f, "legacy \"{k}\" can change"),
            Self::RecipeCanOpen(r) => write!(f, "recipe \"{r}\" can open"),
            Self::RecipeCanComplete(r) => write!(f, "recipe \"{r}\" can complete"),
            Self::EventSignaled(e) => write!(f, "event \"{e}\" signaled"),
            Self::EndingReached(e) => write!(f, "ending \"{e}\" reached"),
        }
    }
}

/// Where a hyperedge was extracted from; the provenance label of the edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Site {
    /// A recipe's effect list and its completion book's products.
    Recipe(RecipeId),
    /// The recipe's abort path (T4): effects rolled back, the recipe's
    /// completion book alone walks, `ResolutionFailed` emitted. A failing
    /// context-free book (expire, event) rolls back silently and produces
    /// nothing, so context-free sites emit no abort edge.
    RecipeAbort(RecipeId),
    /// A card-carried completion book; one edge per triggering route (each
    /// recipe that can bind the card or mint it into an interior) and per
    /// book rule, so every requirement set is a necessary conjunction.
    CardBook {
        card: CardId,
        qualifier: Option<AspectKey>,
    },
    /// A card's `on expire` book (context-free; `subject` is the dying card).
    CardExpire(CardId),
    /// A top-level event book.
    Event(String),
    /// A deck's entries and fallback.
    Deck(DeckId),
    /// Boot instantiation of one card (own declaration; mint aliasing never
    /// applies at boot).
    Boot(CardId),
    /// The boot state itself: declared legacy defaults carry values, and the
    /// player and the autonomous heartbeat can open their verbs' recipes.
    BootState,
}

/// One hyperedge: a site whose effect list can run when every requirement
/// fact holds, producing every produced fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperedge {
    pub site: Site,
    /// Necessary preconditions of this route: an effect's target liveness,
    /// the enclosing recipe's open gate and its unique-card `when` reads, a
    /// book's ignition condition. Requirements are conjunctions; disjunctive
    /// routes split into separate edges.
    pub requires: BTreeSet<Fact>,
    /// Every fact this route can produce (an over-approximation: branch arms
    /// and book rules union, never intersect).
    pub produces: BTreeSet<Fact>,
}

/// The verdict of the Tier-1 plausibility pre-check for one goal fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plausibility {
    /// A producer cone grounds the fact from the boot state.
    Plausible,
    /// No producer cone exists and the content is fully transparent
    /// (`EffectNode::Op` never appears): statically impossible.
    StaticallyImpossible,
    /// No producer cone exists, but the content references host-registered
    /// ops that are opaque to extraction: answer unknown, never impossible
    /// (story-analyze spec §4).
    UnknownHost,
}

/// A backward-provenance witness: the fact, the edge that can produce it,
/// and the provenance of each requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub fact: Fact,
    pub site: Site,
    pub requires: Vec<Provenance>,
}

// ---------------------------------------------------------------------------
// The graph
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct CausalGraph {
    edges: Vec<Hyperedge>,
    producers: BTreeMap<Fact, Vec<usize>>,
    zone_facts: Vec<(CardId, ZoneName)>,
    host_opaque: bool,
    grounded: BTreeSet<Fact>,
}

impl CausalGraph {
    /// Extract the hypergraph from compiled content.
    pub fn build(content: &Content) -> CausalGraph {
        let mut builder = Builder::new(content);
        builder.boot_state();
        builder.boot_cards();
        builder.decks();
        builder.recipes();
        builder.card_books();
        builder.expiry_books();
        builder.event_books();
        let mut graph = builder.finish();
        visit_content_effects(content, &mut |n| {
            if matches!(n, EffectNode::Op { .. }) {
                graph.host_opaque = true;
            }
        });
        graph.grounded = graph.grounded_set();
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
        let mut grounded: BTreeSet<Fact> = BTreeSet::new();
        loop {
            let mut changed = false;
            for edge in &self.edges {
                if edge.requires.iter().all(|r| grounded.contains(r)) {
                    for p in &edge.produces {
                        changed |= grounded.insert(p.clone());
                        if let Fact::AspectCanIncrease {
                            card: Some(card),
                            key,
                            ..
                        } = p
                        {
                            changed |=
                                grounded.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                        }
                    }
                }
            }
            if !changed {
                return grounded;
            }
        }
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
// Extraction
// ---------------------------------------------------------------------------

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

/// The per-site context of an effect-list walk.
#[derive(Clone, Copy)]
struct SiteCtx<'a> {
    /// Context-free books (expire, event): `into interior` fails and slot
    /// targets resolve empty.
    context_free: bool,
    /// The enclosing recipe, for slot-target filter linking.
    recipe: Option<&'a RecipeDecl>,
    /// The carrying card inside a card or expiry book (`subject`).
    subject: Option<&'a CardId>,
}

/// The facts extracted from one effect list.
#[derive(Default)]
struct Extraction {
    requires: BTreeSet<Fact>,
    produces: BTreeSet<Fact>,
    fallible: bool,
    handles: BTreeMap<HandleName, BTreeSet<CardId>>,
}

impl Extraction {
    /// The seed handle pool of a book site: `subject` is the carrying card.
    fn with_subject(subject: Option<&CardId>) -> Extraction {
        let mut ex = Extraction::default();
        if let Some(c) = subject {
            ex.handles
                .entry("subject".to_string())
                .or_default()
                .insert(c.clone());
        }
        ex
    }
}

struct Builder<'a> {
    content: &'a Content,
    oracle: MatchOracle,
    edges: Vec<Hyperedge>,
}

impl<'a> Builder<'a> {
    fn new(content: &'a Content) -> Builder<'a> {
        Builder {
            oracle: MatchOracle::new(content),
            content,
            edges: Vec::new(),
        }
    }

    fn finish(self) -> CausalGraph {
        let mut producers: BTreeMap<Fact, Vec<usize>> = BTreeMap::new();
        let mut zones: BTreeSet<(CardId, ZoneName)> = BTreeSet::new();
        for (i, edge) in self.edges.iter().enumerate() {
            for r in &edge.requires {
                if let Fact::CardInZone(c, z) = r {
                    zones.insert((c.clone(), z.clone()));
                }
            }
            for p in &edge.produces {
                if let Fact::CardInZone(c, z) = p {
                    zones.insert((c.clone(), z.clone()));
                }
                producers.entry(p.clone()).or_default().push(i);
            }
        }
        for list in producers.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        CausalGraph {
            edges: self.edges,
            producers,
            zone_facts: zones.into_iter().collect(),
            host_opaque: false,
            grounded: BTreeSet::new(),
        }
    }

    fn push(&mut self, site: Site, requires: BTreeSet<Fact>, produces: BTreeSet<Fact>) {
        if !produces.is_empty() {
            self.edges.push(Hyperedge {
                site,
                requires,
                produces,
            });
        }
    }

    /// The boot state grounds declared legacy values and the recipes the
    /// player and the heartbeat can open.
    fn boot_state(&mut self) {
        let mut produces = BTreeSet::new();
        for l in self.content.legacies.values() {
            produces.insert(Fact::LegacyCanChange(l.key.clone()));
        }
        for verb in self.content.verbs.values() {
            if !matches!(
                verb.mode,
                story_content::ir::VerbMode::Committable | story_content::ir::VerbMode::Autonomous
            ) {
                continue;
            }
            for rec in verb.recipes.values() {
                produces.insert(Fact::RecipeCanOpen(rec.id.clone()));
            }
        }
        self.push(Site::BootState, BTreeSet::new(), produces);
    }

    /// Boot instantiations produce their own declarations.
    fn boot_cards(&mut self) {
        let mut cards: Vec<&CardDecl> = self.content.cards.values().collect();
        cards.sort_by_key(|c| c.decl_index);
        for decl in cards {
            let Some(face) = &decl.boot else { continue };
            if face.count == 0 {
                continue;
            }
            let mut produces = BTreeSet::new();
            produces.insert(Fact::CardLive(decl.id.clone()));
            produces.insert(Fact::CardInZone(decl.id.clone(), face.zone.clone()));
            for key in decl.aspects.keys() {
                produces.insert(Fact::CardHasAspect(decl.id.clone(), key.clone()));
            }
            let id = decl.id.clone();
            self.push(Site::Boot(id), BTreeSet::new(), produces);
        }
    }

    /// A deck's entries and fallback can be minted by any draw.
    fn decks(&mut self) {
        for deck in self.content.decks.values() {
            let mut produces = BTreeSet::new();
            for entry in &deck.entries {
                produces.insert(Fact::CardLive(entry.clone()));
            }
            if let Some(f) = &deck.fallback {
                produces.insert(Fact::CardLive(f.clone()));
            }
            let id = deck.id.clone();
            self.push(Site::Deck(id), BTreeSet::new(), produces);
        }
    }

    /// One edge per recipe (effects plus its completion book's products),
    /// plus a distinct abort hyperedge when the effect list can fail (T4).
    fn recipes(&mut self) {
        let mut verbs: Vec<&story_content::ir::VerbDecl> = self.content.verbs.values().collect();
        verbs.sort_by_key(|v| v.decl_index);
        for verb in verbs {
            let mut recipes: Vec<&RecipeDecl> = verb.recipes.values().collect();
            recipes.sort_by_key(|r| r.decl_index);
            for rec in recipes {
                let ctx = SiteCtx {
                    context_free: false,
                    recipe: Some(rec),
                    subject: None,
                };
                let mut ex = Extraction::default();
                self.walk_list(&rec.effects, ctx, &mut ex);

                let mut requires = ex.requires.clone();
                requires.insert(Fact::RecipeCanOpen(rec.id.clone()));
                self.require_when(&mut requires, rec.when.as_ref(), None);

                // The book's rule products ride the success edge; their
                // `when`s are downstream conditions, so they add no
                // requirements here (over-approximation stays sound).
                let mut produces = ex.produces.clone();
                if let Some(book) = &rec.on_complete {
                    produces.extend(self.book_products(book, ctx));
                }
                produces.insert(Fact::RecipeCanComplete(rec.id.clone()));
                let id = rec.id.clone();
                self.push(Site::Recipe(id.clone()), requires.clone(), produces);

                if ex.fallible {
                    // The abort path walks the recipe's completion book alone
                    // and emits `ResolutionFailed`; card books are skipped.
                    let mut abort_products = BTreeSet::new();
                    if let Some(book) = &rec.on_complete {
                        abort_products.extend(self.book_products(book, ctx));
                    }
                    self.edges.push(Hyperedge {
                        site: Site::RecipeAbort(id),
                        requires,
                        produces: abort_products,
                    });
                }
            }
        }
    }

    /// Card-carried completion books, one edge per triggering route and per
    /// book rule: a recipe that can bind the card (a slot filter can match
    /// it) or mint it into an interior (its effect list produces the card).
    /// An aspect-qualified book fires under the recipe's `declares` list
    /// without the carrier carrying the qualifier; the interior-pool pass
    /// needs the carrier's own key, so that route requires it.
    fn card_books(&mut self) {
        // Recipes that can mint each card into an interior, from the recipe
        // effect lists built above.
        let minters = self.minters();
        let mut cards: Vec<&CardDecl> = self.content.cards.values().collect();
        cards.sort_by_key(|c| c.decl_index);
        for decl in cards {
            let binders = self.binders(&decl.id);
            let routes: Vec<RecipeId> = binders
                .into_iter()
                .chain(minters.get(&decl.id).into_iter().flatten().cloned())
                .collect();
            for (qualifier, book) in &decl.on_complete {
                let ctx = SiteCtx {
                    context_free: false,
                    recipe: None,
                    subject: Some(&decl.id),
                };
                for route in &routes {
                    for rule in &book.rules {
                        let mut requires = BTreeSet::new();
                        requires.insert(Fact::RecipeCanComplete(route.clone()));
                        if let Some(q) = qualifier {
                            let declared = self
                                .content
                                .recipe(route.as_str())
                                .is_some_and(|r| r.declares.iter().any(|d| d == q));
                            if !declared {
                                requires.insert(Fact::CardHasAspect(decl.id.clone(), q.clone()));
                            }
                        }
                        self.require_when(&mut requires, rule.when.as_ref(), Some(&decl.id));
                        let mut ex = Extraction::with_subject(ctx.subject);
                        self.walk_list(&rule.effects, ctx, &mut ex);
                        let site = Site::CardBook {
                            card: decl.id.clone(),
                            qualifier: qualifier.clone(),
                        };
                        self.push(site, requires, ex.produces);
                    }
                }
            }
        }
    }

    /// Recipes whose slots can bind the card (a binding route).
    fn binders(&self, card: &CardId) -> Vec<RecipeId> {
        let Some(decl) = self.content.card(card) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for verb in self.content.verbs.values() {
            for rec in verb.recipes.values() {
                if rec
                    .slots
                    .iter()
                    .any(|sd| self.oracle.can_match(self.content, &sd.query, decl))
                {
                    out.push(rec.id.clone());
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Recipes whose effect lists can mint the card (a storage route).
    fn minters(&self) -> BTreeMap<CardId, Vec<RecipeId>> {
        let mut out: BTreeMap<CardId, BTreeSet<RecipeId>> = BTreeMap::new();
        for verb in self.content.verbs.values() {
            for rec in verb.recipes.values() {
                let ctx = SiteCtx {
                    context_free: false,
                    recipe: Some(rec),
                    subject: None,
                };
                let mut ex = Extraction::default();
                self.walk_list(&rec.effects, ctx, &mut ex);
                for fact in &ex.produces {
                    if let Fact::CardLive(card) = fact {
                        out.entry(card.clone()).or_default().insert(rec.id.clone());
                    }
                }
            }
        }
        out.into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect()
    }

    /// Expiry books: context-free, subject is the dying card; a book on a
    /// card without a lifetime never fires (no timer is ever armed).
    fn expiry_books(&mut self) {
        let mut cards: Vec<&CardDecl> = self.content.cards.values().collect();
        cards.sort_by_key(|c| c.decl_index);
        for decl in cards {
            let Some(book) = &decl.on_expire else {
                continue;
            };
            if decl.lifetime.is_none() {
                continue;
            }
            let ctx = SiteCtx {
                context_free: true,
                recipe: None,
                subject: Some(&decl.id),
            };
            for rule in &book.rules {
                let mut requires = BTreeSet::new();
                requires.insert(Fact::CardLive(decl.id.clone()));
                self.require_when(&mut requires, rule.when.as_ref(), Some(&decl.id));
                let mut ex = Extraction::with_subject(ctx.subject);
                self.walk_list(&rule.effects, ctx, &mut ex);
                let id = decl.id.clone();
                self.push(Site::CardExpire(id), requires, ex.produces);
            }
        }
    }

    /// Top-level event books: ignited only by `signal`.
    fn event_books(&mut self) {
        for (name, book) in &self.content.events {
            let ctx = SiteCtx {
                context_free: true,
                recipe: None,
                subject: None,
            };
            for rule in &book.rules {
                let mut requires = BTreeSet::new();
                requires.insert(Fact::EventSignaled(name.clone()));
                self.require_when(&mut requires, rule.when.as_ref(), None);
                let mut ex = Extraction::with_subject(ctx.subject);
                self.walk_list(&rule.effects, ctx, &mut ex);
                self.push(Site::Event(name.clone()), requires, ex.produces);
            }
        }
    }

    /// The union of every rule's products (selection law: any single rule
    /// can fire, so the union over-approximates).
    fn book_products(&self, book: &Book, ctx: SiteCtx<'_>) -> BTreeSet<Fact> {
        let mut out = BTreeSet::new();
        for rule in &book.rules {
            let mut ex = Extraction::default();
            self.walk_list(&rule.effects, ctx, &mut ex);
            out.extend(ex.produces);
        }
        out
    }

    /// Unique-card and legacy reads of a condition are necessary facts of
    /// the route — but only when the comparison cannot hold on the declared
    /// defaults: a read falls back to the aspect or legacy default when the
    /// key is absent (T1 totality), so `warden.restore < 1` holds without
    /// any producer, while `warden.hunger >= 3` needs the key raised past
    /// the default. A lower-bound read `key >= n` with `n >= 1` requires
    /// the fact exactly when the effective default is below `n`; upper
    /// bounds and disequalities never require it. Reads inside arithmetic
    /// stay unmodeled (which value they need is not local) — a missing
    /// requirement only ever answers "plausible", never "impossible".
    /// Aggregate scopes stay unmodeled (which candidate would satisfy them
    /// is disjunctive).
    fn require_when(
        &self,
        requires: &mut BTreeSet<Fact>,
        when: Option<&Expr>,
        subject: Option<&CardId>,
    ) {
        let Some(when) = when else { return };
        let mut stack: Vec<&Expr> = vec![when];
        while let Some(e) = stack.pop() {
            match e {
                Expr::Cmp {
                    op: CmpOp::Ge | CmpOp::Gt,
                    left,
                    right,
                } => {
                    if let Expr::Lit(n) = right.as_ref() {
                        self.require_read(left, *n, subject, requires);
                        continue;
                    }
                    // A mirrored literal on the left reads the same.
                    if matches!(left.as_ref(), Expr::Lit(_)) {
                        if let Expr::NamePath { .. } | Expr::Subject(_) | Expr::Legacy(_) =
                            right.as_ref()
                        {
                            continue; // `5 >= key` never needs the key raised
                        }
                    }
                    stack.push(&**left);
                    stack.push(&**right);
                }
                Expr::Arith { left, right, .. }
                | Expr::Cmp { left, right, .. }
                | Expr::Logic { left, right, .. } => {
                    stack.push(&**left);
                    stack.push(&**right);
                }
                Expr::Not(inner) => stack.push(&**inner),
                _ => {}
            }
        }
    }

    /// The read requirement of `read >= n`: necessary only when the
    /// effective default sits below `n`.
    fn require_read(
        &self,
        read: &Expr,
        n: i64,
        subject: Option<&CardId>,
        requires: &mut BTreeSet<Fact>,
    ) {
        if n < 1 {
            return;
        }
        match read {
            Expr::NamePath { card, key } => {
                if self.default_of(Some(card), key) < n {
                    requires.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                }
            }
            Expr::Subject(key) => {
                if let Some(card) = subject {
                    if self.default_of(Some(card), key) < n {
                        requires.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                    }
                }
            }
            Expr::Legacy(key) => {
                let default = self
                    .content
                    .legacies
                    .get(key.as_str())
                    .map(|d| d.default)
                    .unwrap_or(0);
                if default < n {
                    requires.insert(Fact::LegacyCanChange(key.clone()));
                }
            }
            _ => {}
        }
    }

    /// The effective read default of one card's aspect: the card's
    /// declared value, else the aspect registry default, else 0.
    fn default_of(&self, card: Option<&CardId>, key: &str) -> i64 {
        let registry = self
            .content
            .aspects
            .get(key)
            .map(|d| d.default)
            .unwrap_or(0);
        card.and_then(|c| self.content.card(c))
            .map(|d| d.aspects.get(key).copied().unwrap_or(registry))
            .unwrap_or(registry)
    }

    /// Walk one effect list into `ex`. Branch arms union into the same
    /// extraction; fallibility is the disjunction of the nodes.
    fn walk_list(&self, nodes: &[EffectNode], ctx: SiteCtx<'_>, ex: &mut Extraction) {
        for node in nodes {
            self.walk_node(node, ctx, ex);
        }
    }

    fn walk_node(&self, node: &EffectNode, ctx: SiteCtx<'_>, ex: &mut Extraction) {
        match node {
            EffectNode::Spawn {
                prototype,
                count,
                into,
                handle,
            } => {
                ex.fallible |= !is_static_count(count);
                ex.fallible |= Self::is_name_in_use_mint(self, prototype);
                ex.fallible |= self.may_fail_into(into, ctx);
                for fact in self.mint_products(prototype, Some(into)) {
                    ex.produces.insert(fact);
                }
                if let Some(h) = handle {
                    ex.handles
                        .entry(h.clone())
                        .or_default()
                        .insert(prototype.clone());
                }
            }
            EffectNode::Destroy { target, count } => {
                ex.fallible |= match count {
                    DestroyCount::All => false,
                    DestroyCount::AtMost(e) => !is_static_count(e),
                };
                for c in self.requirement_cards(target, ctx, ex) {
                    ex.requires.insert(Fact::CardLive(c));
                }
            }
            EffectNode::Consume { target, .. } => {
                // A consume shortage is the effect failure (T4).
                ex.fallible = true;
                for c in self.requirement_cards(target, ctx, ex) {
                    ex.requires.insert(Fact::CardLive(c));
                }
            }
            EffectNode::Swap {
                target,
                count,
                to,
                into,
            } => {
                ex.fallible |= !is_static_count(count);
                ex.fallible |= Self::is_name_in_use_mint(self, to);
                if let Some(i) = into {
                    ex.fallible |= self.may_fail_into(i, ctx);
                }
                for c in self.requirement_cards(target, ctx, ex) {
                    ex.requires.insert(Fact::CardLive(c));
                }
                for fact in self.mint_products(to, into.as_ref()) {
                    ex.produces.insert(fact);
                }
            }
            EffectNode::Set {
                target,
                key,
                op,
                unclamped,
                ..
            } => {
                for c in self.target_cards(target, ctx, ex) {
                    let card = Some(c);
                    match op {
                        SetOp::Assign => {
                            ex.produces.insert(Fact::AspectCanIncrease {
                                card: card.clone(),
                                key: key.clone(),
                                unclamped: *unclamped,
                            });
                            ex.produces.insert(Fact::AspectCanDecrease {
                                card,
                                key: key.clone(),
                                unclamped: *unclamped,
                            });
                        }
                        SetOp::Add => {
                            ex.produces.insert(Fact::AspectCanIncrease {
                                card,
                                key: key.clone(),
                                unclamped: *unclamped,
                            });
                        }
                        SetOp::Sub => {
                            ex.produces.insert(Fact::AspectCanDecrease {
                                card,
                                key: key.clone(),
                                unclamped: *unclamped,
                            });
                        }
                    }
                }
            }
            EffectNode::Move { target, into } => {
                ex.fallible |= self.may_fail_into(into, ctx);
                for c in self.requirement_cards(target, ctx, ex) {
                    ex.requires.insert(Fact::CardLive(c.clone()));
                }
                for c in self.target_cards(target, ctx, ex) {
                    if let Into::Zone(z) = into {
                        ex.produces.insert(Fact::CardInZone(c, z.clone()));
                    }
                }
            }
            EffectNode::Legacy { key, .. } => {
                ex.produces.insert(Fact::LegacyCanChange(key.clone()));
            }
            EffectNode::Narrate { .. } => {}
            EffectNode::Roll { count, sides, .. } => {
                // Invalid operands are the effect failure (T11): a dynamic
                // count or a sides expression that is not statically >= 1
                // can fail.
                ex.fallible |= !is_static_count(count);
                ex.fallible |= !story_content::ir::const_eval(sides).is_some_and(|s| s >= 1);
            }
            EffectNode::Draw { deck, count, into } => {
                ex.fallible |= !is_static_count(count);
                // An unknown deck fails; exhaustion fails only when the deck
                // declares neither fallback nor wrap.
                ex.fallible |= !self
                    .content
                    .decks
                    .get(deck.as_str())
                    .is_some_and(|d| d.fallback.is_some() || d.wrap);
                let mut cards = BTreeSet::new();
                if let Some(d) = self.content.decks.get(deck.as_str()) {
                    cards.extend(d.entries.iter().cloned());
                    cards.extend(d.fallback.iter().cloned());
                }
                for c in cards {
                    ex.produces.insert(Fact::CardLive(c.clone()));
                    if let Some(Into::Zone(z)) = into {
                        ex.produces.insert(Fact::CardInZone(c, z.clone()));
                    }
                }
            }
            EffectNode::Start { verb, via, .. } => match via {
                Some(r) => {
                    ex.produces.insert(Fact::RecipeCanOpen(r.clone()));
                }
                None => {
                    if let Some(v) = self.content.verbs.get(verb.as_str()) {
                        for r in v.recipes.values() {
                            ex.produces.insert(Fact::RecipeCanOpen(r.id.clone()));
                        }
                    }
                }
            },
            EffectNode::Continue { .. } => {}
            EffectNode::Relate { .. } | EffectNode::Unrelate { .. } => {}
            EffectNode::Signal { event } => {
                ex.produces.insert(Fact::EventSignaled(event.clone()));
            }
            EffectNode::Ending { id } => {
                ex.produces.insert(Fact::EndingReached(id.clone()));
            }
            EffectNode::Branch { then, els, .. } => {
                self.walk_list(then, ctx, ex);
                self.walk_list(els, ctx, ex);
            }
            EffectNode::Op { .. } => {
                // Host-registered effects are opaque to extraction (§4).
            }
        }
    }

    /// The card facts a mint of `prototype` can produce, with the mint alias
    /// resolved so they reflect the minted template. Zone facts only for
    /// statically known destinations.
    fn mint_products(&self, prototype: &CardId, into: Option<&Into>) -> BTreeSet<Fact> {
        let mut out = BTreeSet::new();
        out.insert(Fact::CardLive(prototype.clone()));
        if let Some(template) = template_of(self.content, prototype) {
            for key in template.aspects.keys() {
                out.insert(Fact::CardHasAspect(prototype.clone(), key.clone()));
            }
        }
        if let Some(Into::Zone(z)) = into {
            out.insert(Fact::CardInZone(prototype.clone(), z.clone()));
        }
        out
    }

    /// A `SingleCopy` mint is the hard `NameInUse` failure when a live copy
    /// blocks it; every other mint policy skips instead of failing.
    fn is_name_in_use_mint(&self, card: &CardId) -> bool {
        template_of(self.content, card)
            .is_some_and(|d| d.mint == story_core::MintPolicy::SingleCopy)
    }

    /// Whether an `into` destination can fail to resolve: `interior` inside
    /// a context-free book (no running situation), or a carrier whose host
    /// card may have no live instance at effect time.
    fn may_fail_into(&self, into: &Into, ctx: SiteCtx<'_>) -> bool {
        match into {
            Into::Interior => ctx.context_free,
            Into::Zone(_) => false,
            Into::Carrier(_) => true,
        }
    }

    /// The cards whose liveness the target necessarily requires (the strict
    /// read of the term list; see `MatchOracle::requirement_cards`).
    fn requirement_cards(&self, t: &Target, ctx: SiteCtx<'_>, ex: &Extraction) -> Vec<CardId> {
        match t {
            Target::Name { card, .. } => vec![card.clone()],
            Target::Slot(name) => {
                let Some(rec) = ctx.recipe else {
                    return Vec::new();
                };
                let Some(sd) = rec.slots.iter().find(|s| &s.name == name) else {
                    return Vec::new();
                };
                self.oracle.requirement_cards(self.content, &sd.query)
            }
            Target::Query { terms, .. } | Target::BareQuery(terms) => {
                self.oracle.requirement_cards(self.content, terms)
            }
            Target::Handle(h) => ex
                .handles
                .get(h)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// Every card a target can address (story-analyze spec §4): a name
    /// target links directly, slot and query targets link every card that
    /// can match the filter or terms, a handle resolves to its enclosing
    /// spawn (or the book's `subject`), and a slot target in a context-free
    /// book resolves empty.
    fn target_cards(&self, t: &Target, ctx: SiteCtx<'_>, ex: &Extraction) -> Vec<CardId> {
        match t {
            Target::Name { card, .. } => vec![card.clone()],
            Target::Slot(name) => {
                let Some(rec) = ctx.recipe else {
                    return Vec::new();
                };
                let Some(sd) = rec.slots.iter().find(|s| &s.name == name) else {
                    return Vec::new();
                };
                self.oracle.can_match_cards(self.content, &sd.query)
            }
            Target::Query { terms, .. } | Target::BareQuery(terms) => {
                self.oracle.can_match_cards(self.content, terms)
            }
            Target::Handle(h) => ex
                .handles
                .get(h)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Search support (story-analyze spec §4 outputs 3 and 4): the delete-
// relaxation heuristic and dead-mechanism pruning.
// ---------------------------------------------------------------------------

impl CausalGraph {
    /// The `h_add` value of the goal's facts at a state (§4 output 4): the
    /// classic delete-relaxation estimate — fact cost 0 when the fact holds,
    /// otherwise the minimum over producer edges of `1 + Σ` requirement
    /// costs; the goal value sums its conjuncts. It orders the open queue
    /// only: it is inadmissible under decrease-requiring preconditions and
    /// never affects soundness.
    pub fn h_add(
        &self,
        state: &story_core::State,
        engine: &story_engine::Engine,
        goals: &BTreeSet<Fact>,
    ) -> u32 {
        // Base costs: facts that hold in the state cost nothing.
        let mut cost: BTreeMap<Fact, u32> = BTreeMap::new();
        for fact in self.grounded_state_facts(state, engine) {
            cost.insert(fact, 0);
        }
        // Fixpoint relaxation (the hypergraph is small; plain sweeps are
        // exact and allocation-free). An edge requiring an aspect read is
        // grounded by the aspect's increase capability or by a live mint
        // whose template declares the aspect at >= 1.
        fn increase<'a>(
            cost: &'a BTreeMap<Fact, u32>,
            content: &Content,
            r: &Fact,
        ) -> Option<&'a u32> {
            let direct = cost.get(r);
            let bridged = match r {
                Fact::CardHasAspect(card, key) => {
                    let inc = cost
                        .get(&Fact::AspectCanIncrease {
                            card: Some(card.clone()),
                            key: key.clone(),
                            unclamped: false,
                        })
                        .or_else(|| {
                            cost.get(&Fact::AspectCanIncrease {
                                card: Some(card.clone()),
                                key: key.clone(),
                                unclamped: true,
                            })
                        });
                    let decl_has = template_of(content, card)
                        .and_then(|d| d.aspects.get(key))
                        .copied()
                        .unwrap_or(0)
                        >= 1;
                    let live = if decl_has {
                        cost.get(&Fact::CardLive(card.clone()))
                    } else {
                        None
                    };
                    match (inc, live) {
                        (Some(a), Some(b)) => Some(if a <= b { a } else { b }),
                        (Some(a), None) => Some(a),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    }
                }
                _ => None,
            };
            match (direct, bridged) {
                (Some(a), Some(b)) => Some(if a <= b { a } else { b }),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        }
        let content = engine.content();
        let mut rounds = 0;
        loop {
            let mut changed = false;
            rounds += 1;
            for edge in &self.edges {
                // Boot and bare deck edges have empty requirements for Tier-1
                // static grounding; mid-search `state` already grounds live
                // entities directly, and runtime draws ride their site edge.
                if matches!(edge.site, Site::Boot(_) | Site::BootState | Site::Deck(_)) {
                    continue;
                }
                if let Site::Recipe(r) | Site::RecipeAbort(r) = &edge.site {
                    let exhausted = content
                        .recipe(r.as_str())
                        .and_then(|d| d.max)
                        .is_some_and(|m| state.recipe_runs.get(r).copied().unwrap_or(0) >= m)
                        && !state.situations.values().any(|s| &s.recipe == r);
                    if exhausted {
                        continue;
                    }
                }
                let mut sum = 1u32;
                let mut ready = true;
                for r in &edge.requires {
                    match increase(&cost, content, r) {
                        Some(c) => sum = sum.saturating_add(*c),
                        None => {
                            ready = false;
                            break;
                        }
                    }
                }
                if ready {
                    for p in &edge.produces {
                        let next = sum.min(*cost.get(p).unwrap_or(&u32::MAX));
                        if next < *cost.get(p).unwrap_or(&u32::MAX) {
                            cost.insert(p.clone(), next);
                            changed = true;
                        }
                    }
                }
            }
            if !changed || rounds > 64 {
                break;
            }
        }
        goals
            .iter()
            .map(|g| increase(&cost, content, g).copied().unwrap_or(u32::MAX / 4))
            .fold(0u32, |a, b| a.saturating_add(b))
    }

    /// The facts the state itself grounds (the heuristic's base costs).
    pub(crate) fn grounded_state_facts(
        &self,
        state: &story_core::State,
        engine: &story_engine::Engine,
    ) -> BTreeSet<Fact> {
        use story_content::ir::{Scope, VerbMode};
        let content = engine.content();
        let mut out = BTreeSet::new();
        let mut live: BTreeMap<CardId, Vec<story_core::EntityId>> = BTreeMap::new();
        for (&eid, ent) in &state.entities {
            live.entry(ent.prototype.clone()).or_default().push(eid);
        }
        for (card, ids) in &live {
            out.insert(Fact::CardLive(card.clone()));
            for eid in ids {
                let ent = &state.entities[eid];
                // Key presence grounds the read: the heuristic's landmark
                // for "this mechanism has been engaged". Gating on the
                // value makes h_add forget the landmark exactly once a
                // drain clamps the value back to 0, and the ordering
                // collapses to near-breadth-first (an 8x expansion
                // regression). Ordering-only; never affects soundness.
                for (key, _) in &ent.aspects {
                    out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                }
                for (key, decl) in &content.aspects {
                    if decl.default >= 1 && !ent.aspects.contains_key(key) {
                        out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                    }
                }
            }
        }
        // Zone facts resolve through the kernel's scope face (zone scopes
        // include nested subtrees).
        for (card, zone) in &self.zone_facts {
            let scope = Scope::Zone(zone.clone());
            if engine.scope_entities(&scope).iter().any(|e| {
                state
                    .entities
                    .get(e)
                    .is_some_and(|ent| &ent.prototype == card)
            }) {
                out.insert(Fact::CardInZone(card.clone(), zone.clone()));
            }
        }
        for (l, &val) in &state.legacy {
            let default = content
                .legacies
                .get(l.as_str())
                .map(|d| d.default)
                .unwrap_or(0);
            if val != default || val >= 1 {
                out.insert(Fact::LegacyCanChange(l.clone()));
            }
        }
        for sit in state.situations.values() {
            out.insert(Fact::RecipeCanOpen(sit.recipe.clone()));
        }
        // Committable and autonomous verbs ground their open affordances
        // when the recipe's run cap has not been exhausted.
        for verb in content.verbs.values() {
            if !matches!(verb.mode, VerbMode::Committable | VerbMode::Autonomous) {
                continue;
            }
            for rec in verb.recipes.values() {
                let exhausted = rec
                    .max
                    .is_some_and(|m| state.recipe_runs.get(&rec.id).copied().unwrap_or(0) >= m);
                if !exhausted {
                    out.insert(Fact::RecipeCanOpen(rec.id.clone()));
                }
            }
        }
        out
    }

    /// The backward cone of the goal facts (§4 output 3): every fact some
    /// chain of producer requirements reaches them through.
    pub fn backward_cone(&self, goals: &BTreeSet<Fact>) -> BTreeSet<Fact> {
        // No CardHasAspect → AspectCanIncrease bridging here: bridging pulls
        // every verb that can touch the key into the cone, which erases the
        // irrelevant-verb demotion and collapses the search ordering. The
        // cone keeps the strict fact-to-fact closure; relevant_verbs does
        // its own bridging where verb-level routing is the point.
        let mut cone = goals.clone();
        loop {
            let mut changed = false;
            for edge in &self.edges {
                if edge.produces.iter().any(|p| cone.contains(p)) {
                    for r in &edge.requires {
                        changed |= cone.insert(r.clone());
                    }
                }
            }
            if !changed {
                return cone;
            }
        }
    }

    /// The verbs whose transitive effects can influence a fact of the cone
    /// or reach an ending (§4 output 3): verbs outside the set are
    /// deprioritized in the search, never pruned. Autonomous verbs always
    /// qualify — time itself drives them, and their endings are active
    /// threats.
    pub fn relevant_verbs(&self, content: &Content, cone: &BTreeSet<Fact>) -> BTreeSet<VerbId> {
        let mut relevant = BTreeSet::new();
        // The forward influence closure: facts a verb's recipes produce,
        // transitively enabled edge by edge.
        let influence = |seeds: &BTreeSet<Fact>| -> BTreeSet<Fact> {
            let mut set = seeds.clone();
            loop {
                let mut changed = false;
                for f in set.clone() {
                    if let Fact::AspectCanIncrease {
                        card: Some(card),
                        key,
                        ..
                    } = f
                    {
                        changed |= set.insert(Fact::CardHasAspect(card, key));
                    }
                }
                for edge in &self.edges {
                    // The boot edges ground the state's affordances for
                    // every verb alike — they say nothing about this
                    // verb's own influence, so they stay out of the
                    // closure.
                    if matches!(edge.site, Site::BootState | Site::Boot(_)) {
                        continue;
                    }
                    if edge.requires.iter().all(|r| set.contains(r)) {
                        for p in &edge.produces {
                            changed |= set.insert(p.clone());
                        }
                    }
                }
                if !changed {
                    return set;
                }
            }
        };
        let mut targets: BTreeSet<Fact> = cone.clone();
        // Active threats: every ending the content can reach is an
        // influence target (an ending-bearing autonomous recipe must never
        // be deprioritized).
        for edge in &self.edges {
            for p in &edge.produces {
                if matches!(p, Fact::EndingReached(_)) {
                    targets.insert(p.clone());
                }
            }
        }
        for verb in content.verbs.values() {
            let mut seeds: BTreeSet<Fact> = BTreeSet::new();
            for rec in verb.recipes.values() {
                for edge in &self.edges {
                    let hit = match &edge.site {
                        Site::Recipe(id) | Site::RecipeAbort(id) => id == &rec.id,
                        _ => false,
                    };
                    if hit {
                        seeds.extend(edge.produces.iter().cloned());
                    }
                }
            }
            if seeds.is_empty() {
                // A verb with no extracted edge (opaque or slot-less) stays
                // relevant: Tier-1 silence is not evidence of deadness.
                relevant.insert(verb.id.clone());
                continue;
            }
            if verb.mode == story_content::ir::VerbMode::Autonomous {
                relevant.insert(verb.id.clone());
                continue;
            }
            let closure = influence(&seeds);
            if closure.iter().any(|f| targets.contains(f)) {
                relevant.insert(verb.id.clone());
            }
        }
        relevant
    }
}

/// The goal facts of a query predicate (the heuristic's and the pruning's
/// input): a conservative extraction of necessary facts over the goal's
/// structure. Conjunctive branches union; disjunctive branches intersect;
/// negated branches and upper bounds contribute nothing.
pub fn goal_facts(goal: &crate::query::GoalPredicate) -> BTreeSet<Fact> {
    use crate::query::{EventMatcher, GoalPredicate};
    use story_content::ir::{Expr, LogicOp, Scope};
    fn positive_read(e: &Expr, n: i64, out: &mut BTreeSet<Fact>) {
        if n < 1 {
            return;
        }
        match e {
            Expr::NamePath { card, key } => {
                out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
            }
            Expr::Legacy(key) => {
                out.insert(Fact::LegacyCanChange(key.clone()));
            }
            _ => {}
        }
    }
    fn expr_facts(e: &Expr) -> BTreeSet<Fact> {
        let mut out = BTreeSet::new();
        match e {
            Expr::NamePath { card, key } => {
                out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
            }
            Expr::Legacy(key) => {
                out.insert(Fact::LegacyCanChange(key.clone()));
            }
            Expr::Alive(card) => {
                out.insert(Fact::CardLive(card.clone()));
            }
            Expr::Cmp { op, left, right } => match (op, left.as_ref(), right.as_ref()) {
                (CmpOp::Ge | CmpOp::Eq, _, Expr::Lit(n)) => positive_read(left, *n, &mut out),
                (CmpOp::Gt, _, Expr::Lit(n)) => positive_read(left, n.saturating_add(1), &mut out),
                (CmpOp::Le | CmpOp::Eq, Expr::Lit(n), _) => positive_read(right, *n, &mut out),
                (CmpOp::Lt, Expr::Lit(n), _) => positive_read(right, n.saturating_add(1), &mut out),
                _ => {}
            },
            Expr::Logic {
                op: LogicOp::And,
                left,
                right,
            } => {
                out.extend(expr_facts(left));
                out.extend(expr_facts(right));
            }
            Expr::Logic {
                op: LogicOp::Or,
                left,
                right,
            } => {
                let l = expr_facts(left);
                let r = expr_facts(right);
                out.extend(l.intersection(&r).cloned());
            }
            Expr::Not(_) => {}
            _ => {}
        }
        out
    }
    let mut out = BTreeSet::new();
    match goal {
        GoalPredicate::Expr(e) => out.extend(expr_facts(e)),
        GoalPredicate::CardMatch {
            scope,
            prototype: Some(p),
            terms,
            min_count,
            ..
        } if *min_count >= 1 => {
            out.insert(Fact::CardLive(p.clone()));
            if let Scope::Zone(z) = scope {
                out.insert(Fact::CardInZone(p.clone(), z.clone()));
            }
            for t in terms {
                if let QueryTerm::Aspect(a) = t {
                    let low = match a.op {
                        None => 1,
                        Some(CmpOp::Ge | CmpOp::Eq) => a.value,
                        Some(CmpOp::Gt) => a.value.saturating_add(1),
                        _ => 0,
                    };
                    if low >= 1 {
                        out.insert(Fact::CardHasAspect(p.clone(), a.key.clone()));
                    }
                }
            }
        }
        GoalPredicate::Ending(Some(id)) => {
            out.insert(Fact::EndingReached(id.clone()));
        }
        GoalPredicate::Ending(None) => {}
        GoalPredicate::SituationActive {
            recipe: Some(r), ..
        } => {
            out.insert(Fact::RecipeCanOpen(r.clone()));
        }
        GoalPredicate::TransitionEvent(EventMatcher::SituationCompleted { recipe }) => {
            out.insert(Fact::RecipeCanComplete(recipe.clone()));
        }
        GoalPredicate::TransitionEvent(EventMatcher::Continued { recipe }) => {
            out.insert(Fact::RecipeCanOpen(recipe.clone()));
        }
        GoalPredicate::PartialState(ps) => {
            for (card, key, val) in &ps.aspects {
                if *val >= 1 {
                    out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                }
            }
            for (scope, proto, terms) in &ps.cards {
                if let Some(p) = proto {
                    out.insert(Fact::CardLive(p.clone()));
                    if let Scope::Zone(z) = scope {
                        out.insert(Fact::CardInZone(p.clone(), z.clone()));
                    }
                    for t in terms {
                        if let QueryTerm::Aspect(a) = t {
                            let low = match a.op {
                                None => 1,
                                Some(CmpOp::Ge | CmpOp::Eq) => a.value,
                                Some(CmpOp::Gt) => a.value.saturating_add(1),
                                _ => 0,
                            };
                            if low >= 1 {
                                out.insert(Fact::CardHasAspect(p.clone(), a.key.clone()));
                            }
                        }
                    }
                }
            }
            for (lkey, val) in &ps.legacy {
                if *val >= 1 {
                    out.insert(Fact::LegacyCanChange(lkey.clone()));
                }
            }
        }
        GoalPredicate::And(ps) => {
            for p in ps {
                out.extend(goal_facts(p));
            }
        }
        GoalPredicate::Or(ps) => {
            let mut iter = ps.iter().map(goal_facts);
            if let Some(first) = iter.next() {
                out = iter.fold(first, |acc, next| {
                    acc.intersection(&next).cloned().collect()
                });
            }
        }
        GoalPredicate::Not(_) => {}
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The mint template of a card: the aliased card's body when the declaration
/// carries `mint <card>` (the kernel's `Engine::template`, statically).
fn template_of<'a>(content: &'a Content, card: &CardId) -> Option<&'a CardDecl> {
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
