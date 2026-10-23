//! The scarcity bound, the numeric half of the feasibility gate: for goals
//! that provably need a unique card's aspect to reach a threshold, bound the
//! achievable value from a state by `current + Σ bounded gains` and prune
//! when the bound falls short of the threshold.
//!
//! Soundness rests on one verified direction: gains are over-approximated
//! and decreases are ignored. The kernel lets the player defer gains to the
//! last feasible moment (a commit spends no time; its effects land at
//! completion; only records overrunning the horizon are discarded), so any
//! drain schedule can be defeated by late scheduling and must not be
//! subtracted. Recipe `when` gates are likewise ignored (a false gate can
//! open later). Every unrecognized gain shape reads as unbounded, which only
//! ever disables the gate.
//!
//! Deliberately out of scope, each with a reason: drain accounting (see
//! above), cross-aspect gain chains beyond depth one (a bounded gain whose
//! consumed resource is itself recipe-mintable or deck-dealt reads as
//! unbounded), and deck cursor modeling (remaining permutation deals are
//! unbounded here).

use std::collections::BTreeSet;

use story_content::ir::{
    self, AspectKey, ArithOp, CmpOp, EffectNode, Expr, LogicOp, SetOp, Target,
};
use story_content::Content;
use story_core::{CardId, State};

/// One provable goal threshold: the unique card's key must reach `n`.
#[derive(Clone, Debug)]
struct Threshold {
    card: CardId,
    key: AspectKey,
    n: i64,
}

/// One term of a bounded gain sum.
#[derive(Clone, Debug)]
enum Term {
    /// `g` per use; the use count is capped by the recipe's `max` and by the
    /// remaining stock of the (non-mintable, non-dealt) consumed aspects.
    Literal {
        g: i64,
        max_uses: Option<u64>,
        per_use: Vec<(AspectKey, i64)>,
    },
    /// The term equals the current summed stock of these aspect values (the
    /// slot-total form: what the slots burn is what the recipe adds).
    Stock(Vec<AspectKey>),
}

/// A bounded gain route: the sum of its terms. Empty means the recipe adds
/// nothing recognizable.
type Route = Vec<Term>;

/// The numeric scarcity model for one goal: the thresholds the goal needs
/// and the bounded gain routes into each. Empty when the goal has no
/// modelable thresholds (the gate then never fires).
pub(crate) struct Scarcity {
    thresholds: Vec<Threshold>,
    /// Gain routes per threshold (target card, target key).
    gains: Vec<(CardId, AspectKey, Vec<Route>)>,
}

impl Scarcity {
    /// Build the model for a goal over compiled content. `recipe_mintable`
    /// is the causal graph's set of recipe-producible cards: gains coupled
    /// to a mintable resource read as unbounded.
    pub(crate) fn build(
        goal: &crate::query::GoalPredicate,
        content: &Content,
        recipe_mintable: &BTreeSet<CardId>,
    ) -> Scarcity {
        let thresholds = match goal {
            crate::query::GoalPredicate::Expr(e) => expr_thresholds(e),
            crate::query::GoalPredicate::Ending(Some(id)) => ending_thresholds(id, content),
            _ => Vec::new(),
        };
        let mut gains = Vec::new();
        for t in &thresholds {
            let routes = gain_routes(t, content, recipe_mintable);
            gains.push((t.card.clone(), t.key.clone(), routes));
        }
        if std::env::var("STORY_DEBUG_PRUNE").is_ok() {
            eprintln!("DBG scarcity thresholds: {thresholds:?}");
            for (c, k, r) in &gains {
                eprintln!("DBG scarcity gains for {c:?}.{k}: {r:?}");
            }
        }
        if std::env::var("STORY_DEBUG_PRUNE").is_ok() {
            eprintln!("DBG scarcity thresholds: {thresholds:?}");
            eprintln!("DBG scarcity gains: {gains:?}");
        }
        Scarcity { thresholds, gains }
    }

    /// Whether the model can prune at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.thresholds.is_empty()
    }

    /// The inert model: no thresholds, no gains, no pruning.
    pub(crate) fn empty() -> Scarcity {
        Scarcity {
            thresholds: Vec::new(),
            gains: Vec::new(),
        }
    }

    /// True when some threshold provably cannot be reached from `state`:
    /// the current value plus every bounded gain route still falls short.
    pub(crate) fn dead(&self, state: &State, content: &Content) -> bool {
        for (t, (card, key, routes)) in self.thresholds.iter().zip(&self.gains) {
            debug_assert_eq!(&t.card, card, "thresholds and gains stay aligned");
            let v = state
                .entities
                .values()
                .find(|e| &e.prototype == card)
                .and_then(|e| e.aspects.get(key))
                .copied()
                .unwrap_or_else(|| {
                    content
                        .aspects
                        .get(key)
                        .map(|a| a.default)
                        .unwrap_or(0)
                });
            if v >= t.n {
                continue;
            }
            let mut reachable = v;
            let mut bounded = true;
            'routes: for route in routes {
                for term in route {
                    match term {
                        Term::Stock(keys) => {
                            reachable += keys
                                .iter()
                                .map(|k| {
                                    state
                                        .entities
                                        .values()
                                        .map(|e| e.aspects.get(k).copied().unwrap_or(0))
                                        .sum::<i64>()
                                })
                                .sum::<i64>();
                        }
                        Term::Literal {
                            g,
                            max_uses,
                            per_use,
                        } => {
                            let mut uses = max_uses.map(|u| u as i64);
                            for (k, per) in per_use {
                                let stock: i64 = state
                                    .entities
                                    .values()
                                    .map(|e| e.aspects.get(k).copied().unwrap_or(0))
                                    .sum();
                                let from_stock = if *per > 0 { stock / per } else { i64::MAX };
                                uses = Some(uses.map_or(from_stock, |u| u.min(from_stock)));
                            }
                            match uses {
                                Some(u) => reachable += g.saturating_mul(u),
                                // No cap and no consumed resource: repeatable.
                                None => {
                                    bounded = false;
                                    break 'routes;
                                }
                            }
                        }
                    }
                }
            }
            if bounded && reachable < t.n {
                return true;
            }
        }
        false
    }
}

/// Lower-bound unique-card reads of an expression: `c.k >= n` contributes
/// `(c, k, n)`, `> n` contributes `n + 1`. Conjunctions recurse; `not`,
/// disjunctions, and aggregate scopes contribute nothing (a missed threshold
/// only ever disables the gate).
fn expr_thresholds(e: &Expr) -> Vec<Threshold> {
    let mut out = Vec::new();
    walk_thresholds(e, &mut out);
    out
}

fn walk_thresholds(e: &Expr, out: &mut Vec<Threshold>) {
    match e {
        Expr::Cmp { op, left, right } => {
            if let (Expr::NamePath { card, key }, Expr::Lit(n)) = (left.as_ref(), right.as_ref()) {
                let n = if matches!(op, CmpOp::Gt) { n + 1 } else { *n };
                out.push(Threshold {
                    card: card.clone(),
                    key: key.clone(),
                    n,
                });
            }
        }
        Expr::Logic {
            op: LogicOp::And,
            left,
            right,
        } => {
            walk_thresholds(left, out);
            walk_thresholds(right, out);
        }
        _ => {}
    }
}

/// Thresholds of an ending: the unique-card lower-bound reads in the `when`
/// and the effect-tree branch conditions of every recipe that fires the
/// ending.
fn ending_thresholds(id: &story_core::EndingId, content: &Content) -> Vec<Threshold> {
    let mut out = Vec::new();
    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if !recipe_reaches_ending(&rec.effects, id) {
                continue;
            }
            if let Some(w) = &rec.when {
                walk_thresholds(w, &mut out);
            }
            walk_branch_conds(&rec.effects, &mut out);
        }
    }
    out
}

fn recipe_reaches_ending(effects: &[EffectNode], id: &story_core::EndingId) -> bool {
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        match e {
            EffectNode::Ending { id: end } if end == id => return true,
            EffectNode::Branch { then, els, .. } => stack.extend(then.iter().chain(els.iter())),
            _ => {}
        }
    }
    false
}

fn walk_branch_conds(effects: &[EffectNode], out: &mut Vec<Threshold>) {
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        if let EffectNode::Branch { cond, then, els } = e {
            walk_thresholds(cond, out);
            stack.extend(then.iter().chain(els.iter()));
        }
    }
}

/// The bounded gain routes into `(card, key)`: every `set card.key += expr`
/// whose amount and use count the model can bound. `None` marks an
/// unrecognizable shape, which makes the whole threshold unbounded.
fn gain_routes(
    t: &Threshold,
    content: &Content,
    recipe_mintable: &BTreeSet<CardId>,
) -> Vec<Route> {
    let mut routes = Vec::new();
    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            for route in set_gains(&rec.effects, &t.card, &t.key) {
                let Some(mut route) = route else { return Vec::new() };
                // The use-count bounds: the recipe's `max` cap, and the
                // consumed aspects' stock — where a consumed aspect is
                // itself recipe-mintable or deck-dealt, the stock reads as
                // unbounded and the route falls back to the cap alone.
                let mut consumed: Vec<(AspectKey, i64)> = Vec::new();
                if collect_consumed(&rec.effects, &mut consumed) {
                    for sd in &rec.slots {
                        if sd.consuming {
                            for term in &sd.query {
                                if let ir::QueryTerm::Aspect(a) = term {
                                    consumed.push((a.key.clone(), 1));
                                }
                            }
                        }
                    }
                    consumed.retain(|(k, _)| {
                        resource_bounded(k, content, recipe_mintable)
                    });
                }
                for term in &mut route {
                    if let Term::Literal { max_uses, per_use, .. } = term {
                        *max_uses = max_uses.or(rec.max.map(u64::from));
                        if consumed.is_empty() {
                            // Uncapped and unconsumed: the recipe repeats.
                            *max_uses = None;
                        }
                        *per_use = consumed.clone();
                    }
                }
                routes.push(route);
            }
        }
    }
    routes
}

/// The aspect draws of a recipe's `consume` effects as (key, per-use count).
/// `false` marks a non-literal count, which the model reads as unbounded.
fn collect_consumed(effects: &[EffectNode], out: &mut Vec<(AspectKey, i64)>) -> bool {
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    let mut ok = true;
    while let Some(e) = stack.pop() {
        match e {
            EffectNode::Consume { target, count } => {
                let terms = match target {
                    Target::Query { terms, .. } | Target::BareQuery(terms) => terms,
                    _ => continue,
                };
                let n = match count {
                    Expr::Lit(n) if *n > 0 => *n,
                    _ => {
                        ok = false;
                        continue;
                    }
                };
                for term in terms {
                    if let ir::QueryTerm::Aspect(a) = term {
                        out.push((a.key.clone(), n));
                    }
                }
            }
            EffectNode::Branch { then, els, .. } => stack.extend(then.iter().chain(els.iter())),
            _ => {}
        }
    }
    ok
}

/// Whether the resource behind an aspect key is finite: no card carrying the
/// key is recipe-mintable, and no deck deals one.
fn resource_bounded(
    key: &AspectKey,
    content: &Content,
    recipe_mintable: &BTreeSet<CardId>,
) -> bool {
    let carriers: Vec<&CardId> = content
        .cards
        .values()
        .filter(|c| c.aspects.contains_key(key))
        .map(|c| &c.id)
        .collect();
    let dealt: BTreeSet<&CardId> = content
        .decks
        .values()
        .flat_map(|d| d.entries.iter().chain(d.fallback.iter()))
        .collect();
    carriers
        .iter()
        .all(|c| !recipe_mintable.contains(*c) && !dealt.contains(*c))
}

/// The gain routes of one recipe's `set card.key += expr` effects.
/// `None` marks an unrecognizable amount.
fn set_gains(effects: &[EffectNode], card: &CardId, key: &AspectKey) -> Vec<Option<Vec<Term>>> {
    let mut out = Vec::new();
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        match e {
            EffectNode::Set {
                target: Target::Name { card: c, .. },
                key: k,
                op: SetOp::Add,
                expr,
                ..
            } if c == card && k == key => {
                out.push(set_route(expr));
            }
            EffectNode::Branch { then, els, .. } => stack.extend(then.iter().chain(els.iter())),
            _ => {}
        }
    }
    out
}

fn set_route(expr: &Expr) -> Option<Vec<Term>> {
    match expr {
        Expr::Lit(g) => Some(vec![Term::Literal {
            g: *g,
            max_uses: None,
            per_use: Vec::new(),
        }]),
        Expr::Aspect { key, .. } => Some(vec![Term::Stock(vec![key.clone()])]),
        Expr::Arith {
            op: ArithOp::Add,
            left,
            right,
        } => {
            // A sum of terms is the sum of the terms' contributions. Two
            // literal terms merge into one (their use caps intersect); a
            // literal and a stock term stay separate.
            let mut l = set_route(left)?;
            let mut r = set_route(right)?;
            l.append(&mut r);
            Some(l)
        }
        _ => None,
    }
}
