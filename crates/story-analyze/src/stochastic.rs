//! Stochastic settlement (story-analyze spec §6): the chance-point forking
//! driver. Branching is implemented by re-executing one player step through
//! the kernel's forkable settlement face with every planned outcome class
//! supplied through the hook — the analyzer only chooses outcomes at chance
//! points and the kernel clones `State` per branch; the settlement loop,
//! books, adoption, budgets, and per-unit rollback stay kernel code.
//!
//! Roll classes partition the exact discrete distribution by the thresholds
//! at which the binding is actually consumed in the enclosing effect list
//! (Branch conditions; an arithmetic consumer forces per-distinct-total
//! classes; a binding with no state-affecting consumer does not branch).
//! Draw classes enumerate the remaining undrawn entry multiset before
//! fallback/wrap as a multivariate hypergeometric draw. Every forked draw
//! and roll advances the kernel's cursors uniformly, so the canonical key
//! keeps `decks` and `rng_counter` across branches.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use story_content::ir::{CmpOp, DestroyCount, EffectNode, Expr};
use story_content::Content;
use story_core::{CardId, DeckId, State};
use story_content::runtime::RuntimeDiagnostic;
use story_engine::{ChanceOutcome, ChancePoint, CommitAction, Engine, };

use crate::query::RngMode;
use crate::transitions::PlayerStep;

/// The fork hook the forking settlement face consults at every chance
/// point.
type ForkHook<'a> = &'a mut dyn FnMut(&ChancePoint) -> Option<ChanceOutcome>;

/// The enumeration caps of the forking driver (§5.5's budget discipline):
/// chance classes per point, final settlements and processed plans per
/// action. Reaching a cap degrades the enumeration (some branches of one
/// action collapse), never its soundness — every emitted branch is exact.
pub const MAX_CLASSES_PER_POINT: usize = 128;
pub const MAX_SETTLEMENTS_PER_ACTION: usize = 256;
pub const MAX_PLANS_PER_ACTION: usize = 1024;

/// One settled branch of one player step: the chance outcomes taken (in
/// firing order), the exact branch probability, and the kernel's result.
#[derive(Clone, Debug)]
pub struct Settlement {
    pub outcomes: Vec<ChanceOutcome>,
    pub probability: f64,
    pub result: Result<(Vec<story_engine::Event>, State), RuntimeDiagnostic>,
}

/// One planned answer: the outcome the hook supplies and the exact
/// probability of the class it represents.
type PlanEntry = (ChanceOutcome, f64);

/// The consumers of each roll binding, extracted statically from every
/// effect list in the content (§6).
#[derive(Debug, Default)]
pub struct RollConsumers {
    /// Bind name -> the cut points its Branch comparisons induce.
    pub thresholds: BTreeMap<String, Vec<i64>>,
    /// Bind names with an arithmetic consumer (`set`/`legacy`/operand
    /// reads): per-distinct-total classes.
    pub arithmetic: BTreeSet<String>,
}

impl RollConsumers {
    /// Extract the consumers of every roll binding in the content.
    pub fn of_content(content: &Content) -> RollConsumers {
        let mut out = RollConsumers::default();
        let mut lists: Vec<&[EffectNode]> = Vec::new();
        for verb in content.verbs.values() {
            for rec in verb.recipes.values() {
                lists.push(&rec.effects);
                if let Some(book) = &rec.on_complete {
                    for r in &book.rules {
                        lists.push(&r.effects);
                    }
                }
            }
        }
        for card in content.cards.values() {
            for (_, book) in &card.on_complete {
                for r in &book.rules {
                    lists.push(&r.effects);
                }
            }
            if let Some(book) = &card.on_expire {
                for r in &book.rules {
                    lists.push(&r.effects);
                }
            }
        }
        for book in content.events.values() {
            for r in &book.rules {
                lists.push(&r.effects);
            }
        }
        for list in lists {
            let mut bind = None;
            walk_list(list, &mut bind, &mut out);
        }
        for v in out.thresholds.values_mut() {
            v.sort_unstable();
        }
        out
    }
}

/// Sequential walk of one effect list: the last roll binding persists into
/// nested branches (`Expr::Roll` reads the last binding of the list);
/// Narrate interpolation is not a state-affecting consumer (§6).
fn walk_list(nodes: &[EffectNode], bind: &mut Option<String>, out: &mut RollConsumers) {
    for n in nodes {
        match n {
            EffectNode::Roll { bind: b, .. } => *bind = Some(b.clone()),
            EffectNode::Branch { cond, then, els } => {
                if let Some(b) = bind {
                    scan_cond(cond, b, out);
                }
                walk_list(then, bind, out);
                walk_list(els, bind, out);
            }
            EffectNode::Narrate { .. } => {}
            // A roll-fed destroy count consumes the binding: per-distinct-
            // total classes (`DestroyCount::AtMost(expr)`; `All` reads
            // nothing).
            EffectNode::Destroy { count, .. } => {
                if let (Some(b), DestroyCount::AtMost(expr)) = (bind.as_deref(), count) {
                    if reads_roll(expr) {
                        out.arithmetic.insert(b.to_string());
                    }
                }
            }
            EffectNode::Consume { count, .. }
            | EffectNode::Swap { count, .. }
            | EffectNode::Spawn { count, .. }
            | EffectNode::Start { count, .. }
            | EffectNode::Draw { count, .. } => {
                if let Some(b) = bind {
                    if reads_roll(count) {
                        out.arithmetic.insert(b.clone());
                    }
                }
            }
            EffectNode::Set { expr, .. } | EffectNode::Legacy { expr, .. } => {
                if let Some(b) = bind {
                    if reads_roll(expr) {
                        out.arithmetic.insert(b.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

fn scan_cond(e: &Expr, bind: &str, out: &mut RollConsumers) {
    match e {
        Expr::Cmp { op, left, right } => {
            if let Expr::Lit(t) = right.as_ref() {
                if matches!(**left, Expr::Roll) {
                    out.thresholds
                        .entry(bind.to_string())
                        .or_default()
                        .push(cut_of(*op, *t));
                    return;
                }
            }
            if let Expr::Lit(t) = left.as_ref() {
                if matches!(**right, Expr::Roll) {
                    out.thresholds
                        .entry(bind.to_string())
                        .or_default()
                        .push(cut_of(mirror(*op), *t));
                }
            }
        }
        Expr::Arith { left, right, .. } | Expr::Logic { left, right, .. } => {
            if reads_roll(e) {
                out.arithmetic.insert(bind.to_string());
            }
            scan_cond(left, bind, out);
            scan_cond(right, bind, out);
        }
        Expr::Not(inner) => scan_cond(inner, bind, out),
        _ => {}
    }
}

/// Whether the expression reads the roll binding anywhere.
fn reads_roll(e: &Expr) -> bool {
    match e {
        Expr::Roll => true,
        Expr::Arith { left, right, .. }
        | Expr::Cmp { left, right, .. }
        | Expr::Logic { left, right, .. } => reads_roll(left) || reads_roll(right),
        Expr::Not(inner) => reads_roll(inner),
        _ => false,
    }
}

/// A `roll >= t` comparison cuts the distribution at t; `>` at t+1; an
/// equality cuts at t.
fn cut_of(op: CmpOp, t: i64) -> i64 {
    match op {
        CmpOp::Ge | CmpOp::Lt => t,
        CmpOp::Gt | CmpOp::Le => t.saturating_add(1),
        CmpOp::Eq | CmpOp::Ne => t,
    }
}

fn mirror(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Le,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Eq | CmpOp::Ne => CmpOp::Eq,
    }
}

/// One outcome class of a chance point: the representative the hook
/// supplies, and the class's exact probability.
#[derive(Clone, Debug)]
pub struct OutcomeClass {
    pub outcome: ChanceOutcome,
    pub probability: f64,
}

/// Every outcome class of one chance point. `answered_draws` is the total
/// count of the draws this settlement already answered — deck cursors
/// advance uniformly across branches (§2.2), so the remaining window is the
/// start-state cursor plus that total.
pub fn outcome_classes(
    content: &Content,
    consumers: &RollConsumers,
    seed: u64,
    state: &State,
    point: &ChancePoint,
    answered_draws: u32,
) -> Vec<OutcomeClass> {
    match point {
        ChancePoint::Roll {
            count,
            sides,
            bonus,
            bind,
        } => roll_classes(*count, *sides, *bonus, bind, consumers),
        ChancePoint::Draw { deck, count, .. } => {
            draw_classes(content, seed, state, deck, *count, answered_draws)
        }
    }
}

/// Roll classes (§6): the exact distribution over `count`d`sides` + bonus,
/// partitioned by the consumption thresholds; per-distinct-total classes
/// under an arithmetic consumer; a single representative with no
/// state-affecting consumer.
fn roll_classes(
    count: u32,
    sides: u32,
    bonus: i64,
    bind: &str,
    consumers: &RollConsumers,
) -> Vec<OutcomeClass> {
    let dist = dice_distribution(count, sides);
    if dist.is_empty() {
        return Vec::new();
    }
    let min = count as i64 + bonus;
    let max = count as i64 * sides as i64 + bonus;
    if consumers.arithmetic.contains(bind) {
        let out: Vec<OutcomeClass> = dist
            .iter()
            .enumerate()
            .filter(|(_, p)| **p > 0.0)
            .map(|(i, p)| OutcomeClass {
                outcome: ChanceOutcome::Roll(min + i as i64),
                probability: *p,
            })
            .collect();
        if out.len() > MAX_CLASSES_PER_POINT {
            return single_class(min, max);
        }
        return out;
    }
    let cuts = consumers
        .thresholds
        .get(bind)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    if cuts.is_empty() {
        // No state-affecting consumer: one representative outcome (§6).
        return vec![OutcomeClass {
            outcome: ChanceOutcome::Roll(midpoint(min, max)),
            probability: 1.0,
        }];
    }
    // Partition [min, max] by the cuts; each class's probability is the sum
    // of its totals' exact probabilities.
    let mut edges = vec![min];
    edges.extend(cuts.iter().copied().filter(|&b| b > min && b <= max));
    edges.sort_unstable();
    edges.dedup();
    let mut out = Vec::new();
    for i in 0..edges.len() {
        let lo = edges[i];
        let hi = if i + 1 < edges.len() { edges[i + 1] - 1 } else { max };
        if lo > hi {
            continue;
        }
        let p: f64 = (lo..=hi)
            .filter_map(|t| dist.get((t - min) as usize).copied())
            .sum();
        if p > 0.0 {
            out.push(OutcomeClass {
                outcome: ChanceOutcome::Roll(lo),
                probability: p,
            });
        }
    }
    if out.len() > MAX_CLASSES_PER_POINT {
        return single_class(min, max);
    }
    out
}

fn single_class(min: i64, max: i64) -> Vec<OutcomeClass> {
    vec![OutcomeClass {
        outcome: ChanceOutcome::Roll(midpoint(min, max)),
        probability: 1.0,
    }]
}

fn midpoint(min: i64, max: i64) -> i64 {
    min.saturating_add((max - min).saturating_div(2))
}

/// The exact distribution of the sum of `count` uniform dice with `sides`
/// faces: exact integer counts over the denominator `sides^count`. Empty
/// for pathological operands, where the driver degrades to one class.
fn dice_distribution(count: u32, sides: u32) -> Vec<f64> {
    if count == 0 || sides == 0 {
        return Vec::new();
    }
    let span = (count as u64).saturating_mul(sides as u64);
    if span > 4096 {
        return Vec::new();
    }
    let mut weights = vec![1u128];
    for _ in 0..count {
        let mut next = vec![0u128; weights.len() + sides as usize - 1];
        for (i, w) in weights.iter().enumerate() {
            for die in 1..=sides {
                next[i + die as usize - 1] += *w;
            }
        }
        weights = next;
    }
    let total = u128::pow(sides as u128, count);
    weights
        .into_iter()
        .map(|w| w as f64 / total as f64)
        .collect()
}

/// Draw classes (§6): the remaining undrawn entry multiset before
/// fallback/wrap as a multivariate hypergeometric draw over the requested
/// count. One branch per distinct multiset, minted in deck-declaration
/// order (order within a branch is canonical-key invisible up to entity
/// symmetry); the seed-fixed window is one of the classes.
fn draw_classes(
    content: &Content,
    seed: u64,
    state: &State,
    deck: &DeckId,
    count: u32,
    answered_draws: u32,
) -> Vec<OutcomeClass> {
    let Some(decl) = content.decks.get(deck.as_str()) else {
        return Vec::new();
    };
    if count == 0 {
        return Vec::new();
    }
    let perm = deck_permutation(seed, deck.as_str(), &decl.entries);
    let start = state.decks.get(deck).copied().unwrap_or(0);
    let cursor = start.saturating_add(answered_draws as u64);
    if cursor as usize >= perm.len() {
        return Vec::new();
    }
    let remaining = &perm[cursor as usize..];
    if remaining.len() < count as usize {
        // The draw leaves the permutation (fallback/wrap): the chance point
        // does not fire (§2.2); the kernel path is the one settlement.
        return Vec::new();
    }
    let mut pool: Vec<(CardId, u64)> = Vec::new();
    for card in remaining {
        match pool.iter_mut().find(|(c, _)| c == card) {
            Some((_, n)) => *n += 1,
            None => pool.push((card.clone(), 1)),
        }
    }
    let denom = binomial(remaining.len() as u64, count as u64);
    let mut out = Vec::new();
    let mut chosen: Vec<(CardId, u64)> = Vec::new();
    enumerate_multisets(&pool, 0, count as u64, &mut chosen, denom, &mut out);
    out
}

fn enumerate_multisets(
    pool: &[(CardId, u64)],
    idx: usize,
    remaining: u64,
    chosen: &mut Vec<(CardId, u64)>,
    denom: f64,
    out: &mut Vec<OutcomeClass>,
) {
    if out.len() > MAX_CLASSES_PER_POINT {
        return;
    }
    if idx == pool.len() {
        if remaining == 0 {
            let mut cards: Vec<CardId> = Vec::new();
            let mut num = 1f64;
            for (card, m) in chosen.iter() {
                for _ in 0..*m {
                    cards.push(card.clone());
                }
                let available = pool
                    .iter()
                    .find(|(c, _)| c == card)
                    .map(|(_, n)| *n)
                    .unwrap_or(0);
                num *= binomial(available, *m);
            }
            out.push(OutcomeClass {
                outcome: ChanceOutcome::Draw(cards),
                probability: num / denom,
            });
        }
        return;
    }
    let (card, available) = &pool[idx];
    let top = (*available).min(remaining);
    for take in 0..=top {
        if take > 0 {
            chosen.push((card.clone(), take));
        }
        enumerate_multisets(pool, idx + 1, remaining - take, chosen, denom, out);
        if take > 0 {
            chosen.pop();
        }
    }
}

/// The boot-derived deck permutation: the kernel's seeded Fisher-Yates over
/// the declared entries — the same deterministic derivation `boot` runs
/// from the public `story_core` primitives (`fingerprint`, `splitmix64`).
pub fn deck_permutation(seed: u64, deck: &str, entries: &[CardId]) -> Vec<CardId> {
    let mut perm = entries.to_vec();
    let mut ctr = seed ^ story_core::fingerprint::fingerprint(deck);
    for i in (1..perm.len()).rev() {
        ctr = ctr.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let j = (story_core::rng::splitmix64(ctr) % (i as u64 + 1)) as usize;
        perm.swap(i, j);
    }
    perm
}

fn binomial(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut num = 1u128;
    for i in 0..k {
        num = num.saturating_mul((n - i) as u128) / (i + 1) as u128;
    }
    num as f64
}

// ---------------------------------------------------------------------------
// The forking driver
// ---------------------------------------------------------------------------

/// Every settled branch of one player step: one settlement in
/// `Deterministic`; the enumerated outcome classes in the branching modes.
/// Refusals return inside the settlement's `result` (a refused action
/// creates no edge, T3 — the caller drops it).
pub fn settlements(
    content: &Arc<Content>,
    template: &Engine,
    seed: u64,
    state: &State,
    step: &PlayerStep,
    mode: RngMode,
) -> Vec<Settlement> {
    match mode {
        RngMode::Deterministic => vec![run_once(template, state, step, None)],
        RngMode::OptimisticBranching | RngMode::Adversarial => {
            enumerate(content, template, seed, state, step)
        }
    }
}

fn run_once(
    template: &Engine,
    state: &State,
    step: &PlayerStep,
    mut fork: Option<ForkHook<'_>>,
) -> Settlement {
    let mut engine = template.with_state(state.clone());
    let result = match step {
        PlayerStep::Commit { verb, bindings } => {
            let action = CommitAction {
                verb: verb.clone(),
                bindings: bindings.clone(),
            };
            match fork.as_mut() {
                Some(hook) => engine.commit_forked(action, hook),
                None => engine.commit(action),
            }
        }
        PlayerStep::Bind { sit, slot, entity } => engine.bind(*sit, slot.clone(), *entity),
        PlayerStep::Advance { dt } => match fork.as_mut() {
            Some(hook) => engine.advance_forked(*dt, hook),
            None => engine.advance(*dt),
        },
    };
    Settlement {
        outcomes: Vec::new(),
        probability: 1.0,
        result: result.map(|ev| (ev, engine.into_state())),
    }
}

/// The branch-plan executor (§6): iterative plan discovery. A plan is a
/// full assignment for a prefix of the run's chance points; a run answers
/// its plan entry at each chance point, and the first unplanned point is
/// *discovered* — its outcome classes extend the plan queue (the discovery
/// run itself is not a settlement). A run that answers its whole plan
/// without discovering a new point is the one final settlement of that
/// outcome sequence. Branch probabilities are the products of the answered
/// classes' exact probabilities.
fn enumerate(
    content: &Arc<Content>,
    template: &Engine,
    seed: u64,
    state: &State,
    step: &PlayerStep,
) -> Vec<Settlement> {
    let consumers = RollConsumers::of_content(content);
    let mut results: Vec<Settlement> = Vec::new();
    let mut plans: Vec<Vec<PlanEntry>> = vec![Vec::new()];
    let mut processed = 0usize;
    while let Some(plan) = plans.pop() {
        processed += 1;
        if processed > MAX_PLANS_PER_ACTION || results.len() >= MAX_SETTLEMENTS_PER_ACTION {
            break;
        }
        // Discovery happens exactly when the plan is exhausted, so a
        // discovery run's taken prefix equals its plan.
        let base_plan = plan.clone();
        let run = RefCell::new(RunState {
            plan,
            taken: Vec::new(),
            prob: 1.0,
            discovered: None,
            draws_answered: 0,
            consumers: &consumers,
            content,
            seed,
            state,
        });
        let settlement = {
            let mut hook = |point: &ChancePoint| -> Option<ChanceOutcome> {
                let mut rs = run.borrow_mut();
                if let Some(weight) = rs.plan.get(rs.taken.len()).cloned() {
                    rs.taken.push(weight.0.clone());
                    rs.prob *= weight.1;
                    rs.note(&weight.0);
                    return Some(weight.0);
                }
                // Discovery: the first unplanned chance point of this run.
                if rs.discovered.is_none() {
                    rs.discovered = Some(point.clone());
                }
                // Answer with the first class to keep the discovery run
                // deterministic; only final runs settle.
                let first = rs.classes_now(point).into_iter().next()?;
                rs.taken.push(first.outcome.clone());
                rs.note(&first.outcome);
                Some(first.outcome.clone())
            };
            let mut s = run_once(template, state, step, Some(&mut hook));
            let rs = run.into_inner();
            s.outcomes = rs.taken;
            s.probability = rs.prob;
            (s, rs.discovered)
        };
        let (settlement, discovered) = settlement;
        match discovered {
            None => results.push(settlement),
            Some(point) if plans.len() < MAX_PLANS_PER_ACTION => {
                // The extensions replace the discovery answer, so the
                // cursor counts only the plan prefix's draws.
                let answered = base_plan
                    .iter()
                    .filter_map(|e| match &e.0 {
                        ChanceOutcome::Draw(cards) => Some(cards.len() as u32),
                        _ => None,
                    })
                    .sum();
                let classes = outcome_classes(content, &consumers, seed, state, &point, answered);
                // Each class of the discovered point extends the plan with
                // its exact weight. A single-class point re-enqueues
                // unchanged (the queue converges when the run discovers no
                // further point).
                for c in classes {
                    let mut child = base_plan.clone();
                    child.push((c.outcome, c.probability));
                    plans.push(child);
                }
            }
            Some(_) => {}
        }
    }
    results
}

struct RunState<'a> {
    plan: Vec<PlanEntry>,
    taken: Vec<ChanceOutcome>,
    prob: f64,
    discovered: Option<ChancePoint>,
    draws_answered: u32,
    consumers: &'a RollConsumers,
    content: &'a Arc<Content>,
    seed: u64,
    state: &'a State,
}

impl<'a> RunState<'a> {
    /// The outcome classes of a chance point at this moment of the run: the
    /// deck cursor accounts for the draws already answered.
    fn classes_now(&self, point: &ChancePoint) -> Vec<OutcomeClass> {
        outcome_classes(
            self.content,
            self.consumers,
            self.seed,
            self.state,
            point,
            self.draws_answered,
        )
    }

    /// Record a cursor-relevant answer.
    fn note(&mut self, outcome: &ChanceOutcome) {
        if let ChanceOutcome::Draw(cards) = outcome {
            self.draws_answered += cards.len() as u32;
        }
    }
}
