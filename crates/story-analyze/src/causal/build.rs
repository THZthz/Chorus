//! Hyperedge extraction: the per-site builder passes over compiled content.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{
    Book, CardDecl, CmpOp, DestroyCount, EffectNode, Expr, HandleName, Into, RecipeDecl, SetOp,
    Target, ZoneName,
};
use story_content::Content;
use story_core::{CardId, RecipeId};

use super::fact::{Fact, Hyperedge, Site};
use super::oracle::MatchOracle;
use super::{is_static_count, template_of, CausalGraph};

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

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

pub(super) struct Builder<'a> {
    content: &'a Content,
    oracle: MatchOracle,
    edges: Vec<Hyperedge>,
}

impl<'a> Builder<'a> {
    pub(super) fn new(content: &'a Content) -> Builder<'a> {
        Builder {
            oracle: MatchOracle::new(content),
            content,
            edges: Vec::new(),
        }
    }

    pub(super) fn finish(self) -> CausalGraph {
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
    pub(super) fn boot_state(&mut self) {
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
    pub(super) fn boot_cards(&mut self) {
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
    pub(super) fn decks(&mut self) {
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
    pub(super) fn recipes(&mut self) {
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
    pub(super) fn card_books(&mut self) {
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
    pub(super) fn expiry_books(&mut self) {
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
    pub(super) fn event_books(&mut self) {
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
