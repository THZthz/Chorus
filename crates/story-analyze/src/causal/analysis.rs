//! Search support (story-analyze spec §4 outputs 3 and 4): the delete-relaxation heuristic, grounded state facts, backward cones, and verb relevance.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{CmpOp, QueryTerm};
use story_content::Content;
use story_core::{CardId, VerbId};

use super::fact::{Fact, Site};
use super::{template_of, CausalGraph};

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
