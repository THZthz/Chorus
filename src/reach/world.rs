//! The analysis host: the WorldFacts
//! interface, `compute_world_facts`' pre-fixpoint half, and the identity-cache
//! design — all world facts are pure functions of (content, initial), so one
//! `Analyzer<'c>` holding `(content, initial)` suffices, with a
//! lazily-computed [`WorldFacts`]. The world-static predicate CLASSIFIERS
//! (`pred_static`/`expr_static`) and EVALUATORS
//! (`eval_static`/`path_open_first_failure`/`writable_paths`) live here — the
//! shape tests
//! and the evaluators must agree variant for variant, so they share one
//! home; census.rs imports pred_static for its site tagging
//! (runtime_gated / path_non_static).

use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};

use crate::content::{
    for_each_authored_outcome_infallible, CompiledContent, CompiledTrigger, CompiledVerb,
    PrototypeDef,
};
use crate::core::clock::{MIN_PER_TICK, TICKS_PER_DAY};
use crate::core::dict::Dictionary;
use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{match_query, CmpOp, Query, QueryTerm};
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, Outcome};
use crate::core::types::{aspect_get, Aspects, Entity, State};
use crate::error::{ChorusError, Result};

use super::census::{
    collect_sites, object_always, site_mints, trigger_owner, Census, RawSite, SiteKind, TrigIndex,
};

/// An `i64::MAX` stand-in for an unbounded freeze distance: every
/// observed value is a first-crossing distance (1..=1440), so `i64::MAX`
/// preserves every comparison direction (`fc < freeze_dist` always true when
/// unfrozen, `fc >= freeze_dist` always false).
pub(crate) const INFINITY: i64 = i64::MAX;

/// The per-fact weight cap of the card world: `min(count, 32)`. The
/// fixpoint's count queries and the post-fixpoint universe flatten MUST
/// agree on it (a fact's world weight and its universe presence are two
/// views of the same fact).
pub(crate) const FLATTEN_WEIGHT_CAP: i64 = 32;

/// The window length a recipe-owned site completes under (§6.4): the
/// site's OWN recipe's `dur` override, else the verb's `duration` default.
/// Sites are keyed (verb index, recipe index) by the census, so the recipe
/// is always known exactly — the analyzer never has to guess which recipe a
/// commit selects here.
pub(crate) fn effective_duration(v: &CompiledVerb, ri: Option<usize>) -> i64 {
    match ri {
        Some(i) => v.recipes.get(i).and_then(|r| r.dur).unwrap_or(v.duration),
        None => v.duration,
    }
}

// ---------------------------------------------------------------------------
// world-static predicate evaluation
// ---------------------------------------------------------------------------

/// A value expression is world-static when it reads no runtime state. The
/// §6.3 sum scopes join the static class: `total(table, …)` is a pure world
/// read, and `total(bound, …)` is decidable from the analyzer's window
/// model (the [`BoundView`]) — a missing model reads Unknown, never an
/// internal throw. `count(held)` joins the same way (§6.7's held slot, §13.1):
/// the analyzer models the hold as a player step it can arrange, so the
/// gate decides over the held slot's fit — any other role's count reads
/// runtime bindings and stays non-static.
fn expr_static(v: &ValExpr) -> bool {
    match v {
        ValExpr::Int { .. }
        | ValExpr::CountQuery { .. }
        | ValExpr::TotalTable { .. }
        | ValExpr::TotalBound { .. } => true,
        // §6.7's held slot: the analyzer models the hold as arrangeable, so
        // the gate decides over the held slot's fit (§13.1). Any other
        // role's count reads runtime bindings and stays non-static.
        ValExpr::Count { role } => role == "held",
        _ => false,
    }
}

/// A predicate is world-static when every operand is. The
/// `Rel`/`Every`/`Any` arms read runtime role state; `Exists` /
/// `ExistsQuery` are plain world reads (answerable from the card world
/// alone, evaluated by [`eval_static`]); the §6.3 sum scopes are static
/// too — `total(table, …)` over the card world, `total(bound, …)` over the
/// caller's window model (`None` reads Unknown, which only demotes).
pub(crate) fn pred_static(p: &Pred) -> bool {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => pred_static(a) && pred_static(b),
        Pred::Not(a) => pred_static(a),
        Pred::Cmp { l, r, .. } => expr_static(l) && expr_static(r),
        Pred::Exists { .. } | Pred::ExistsQuery { .. } => true,
        Pred::Rel { .. } | Pred::Every { .. } | Pred::Any { .. } => false,
    }
}

/// The fixpoint's view of the available world: distinct
/// facts with count-capped weights. Query evaluation over weights is EXACTLY
/// the evaluation over the flattened entity array — flatten pushes
/// [`FLATTEN_WEIGHT_CAP`] instances per fact at most.
#[derive(Debug, Clone)]
pub(crate) struct CardWorld {
    /// the flat_cache version stamp (see [`WorldFacts::push`])
    pub ver: u64,
    pub by_name: IndexMap<String, i64>,
    pub distinct: Vec<DistEntry>,
}

/// One distinct fact: the entity descriptor `e`, its
/// capped weight `w`, and whether the fact's source repeats unconditionally
/// (`CardFact::unbounded` — the weight is then a floor, not a ceiling).
#[derive(Debug, Clone)]
pub(crate) struct DistEntry {
    pub e: Entity,
    pub w: i64,
    pub unbounded: bool,
}

/// The window model the `total(bound, aspect:k)` scope sums, one entry per
/// DECLARED slot of the owning verb: the descriptors the slot could bind
/// (its query's fit over the available world, stackable facts expanded to
/// their capped copy counts), the slot's `min` and its `max` (`None` =
/// unbounded). The engine's cross-role rule is the model's too: a card may
/// fill two roles, so the slots draw from the pool independently while
/// staying distinct WITHIN a slot (`validate_slots`' placed-set).
pub(crate) struct BoundView<'a> {
    pub slots: Vec<BoundSlot<'a>>,
}

/// One declared slot's bindable face (see [`BoundView`]). `role` is the
/// lowercase slot role — the held slot's face is what a `count(held)` gate
/// reads (§13.1's arrangeable-hold model).
pub(crate) struct BoundSlot<'a> {
    pub role: String,
    pub cards: Vec<&'a Entity>,
    pub min: i64,
    pub max: Option<i64>,
}

/// Evaluate a static value expression against the available world, as an
/// interval: `(lo, hi)` with `hi: None` = unbounded above. The lo face is
/// exactly the plain capped-weight sum over matching facts); `hi` lifts off only when an UNBOUNDED fact matches the query —
/// its source repeats unconditionally, so the world count is a floor, never
/// a ceiling. Bounded operands carry `hi = Some(lo)`.
///
/// The §6.3 sum scopes weight the same enumerations by the descriptors'
/// LEVEL (missing → 0, the engine's house rule): `total(table, …)` over the
/// distinct facts, `total(bound, …)` over the [`BoundView`]. An unbounded
/// matching fact with a POSITIVE level lifts the ceiling (more copies only
/// raise the sum); a negative level sinks it instead — that face unbounds
/// the interval both ways (the full-open pair decides Unknown under every
/// op), which only ever widens toward candidate.
fn eval_expr_interval(
    v: &ValExpr,
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> Result<(i64, Option<i64>)> {
    match v {
        ValExpr::Int { v } => Ok((*v, Some(*v))),
        // §6.7's held slot, the arrangeable face (§13.1): the hold is a
        // player step the analyzer can arrange, like a commit — a fitting
        // descriptor in the available world reads the count as exactly 1
        // (the hold arranged; a require-held gate then decides True,
        // provable), none reads exactly 0 (the gate decides False — the
        // hold alone cannot satisfy it: never claim Proven off an
        // unarrangeable hold). No window model at all (a chain-pinned verb's
        // merged-gate walk, a trigger's empty bindings) reads the full-open
        // pair — Unknown under every op, the widening-only invariant.
        ValExpr::Count { role } if role == "held" => match bound {
            None => Ok((i64::MIN, None)),
            Some(b) => {
                let fits =
                    b.slots.iter().find(|s| s.role == "held").is_some_and(|s| !s.cards.is_empty());
                Ok(if fits { (1, Some(1)) } else { (0, Some(0)) })
            }
        },
        ValExpr::CountQuery { query } => {
            let mut sum: i64 = 0;
            let mut unbounded = false;
            for d in &world.distinct {
                if match_query(&d.e, query) {
                    sum += d.w;
                    unbounded |= d.unbounded;
                }
            }
            Ok((sum, if unbounded { None } else { Some(sum) }))
        }
        ValExpr::TotalTable { path } => {
            let mut sum: i64 = 0;
            let mut lift = false;
            let mut sink = false;
            for d in &world.distinct {
                let level = aspect_get(&d.e, path).unwrap_or(0);
                sum = sum.saturating_add(d.w.saturating_mul(level));
                if d.unbounded {
                    lift |= level > 0;
                    sink |= level < 0;
                }
            }
            if sink {
                return Ok((i64::MIN, None));
            }
            Ok((sum, if lift { None } else { Some(sum) }))
        }
        ValExpr::TotalBound { path } => match bound {
            // no window model: the full-open pair — Unknown under every op
            None => Ok((i64::MIN, None)),
            Some(b) => {
                // the achievable window sums, per slot: the FLOOR binds the
                // slot's min cards at their SMALLEST levels, the ceiling its
                // max cards at their LARGEST (the window may always bind
                // fewer — a comparison must hold at the floor to prove and
                // at no corner to demote). An unbounded-max slot with a
                // positive level opens the ceiling (arbitrary many positive
                // copies) and with a negative one sinks the floor — either
                // way the interval goes full-open, Unknown under every op,
                // which only ever widens toward candidate.
                let mut lo: i64 = 0;
                let mut hi: i64 = 0;
                let mut full_open = false;
                for slot in &b.slots {
                    let mut levels: Vec<i64> =
                        slot.cards.iter().map(|e| aspect_get(e, path).unwrap_or(0)).collect();
                    levels.sort_unstable();
                    // slots parse non-negative; the clamp is a defensive
                    // coercion (a negative min casts huge as usize)
                    let min = slot.min.max(0) as usize;
                    lo = lo.saturating_add(levels.iter().take(min).sum::<i64>());
                    match slot.max {
                        None => {
                            // an unbounded-max slot: any non-zero level drives
                            // the sum without bound (positive copies raise
                            // it, negative ones lower it) — full-open
                            if levels.iter().any(|&l| l != 0) {
                                full_open = true;
                            }
                        }
                        Some(mx) => {
                            let k = std::cmp::min(mx.max(0) as usize, levels.len());
                            hi = hi.saturating_add(levels[levels.len() - k..].iter().sum::<i64>());
                            // an extendable slot (more cards may join than
                            // the min fills) whose smallest level is negative
                            // drags the floor below the min-filling sum
                            if mx > slot.min && levels.first().is_some_and(|&l| l < 0) {
                                full_open = true;
                            }
                        }
                    }
                }
                if full_open {
                    Ok((i64::MIN, None))
                } else {
                    Ok((lo, Some(hi)))
                }
            }
        },
        // preserved as Err (catchable upstream)
        _ => Err(ChorusError::new("internal: eval_expr_interval over a non-static operand")),
    }
}

/// The three-valued static-truth verdict: `True`/`False` are exact over the
/// card world; `Unknown` is the honest third value for a count-form
/// comparison whose matching pool is UNBOUNDED and whose verdict the
/// interval pair cannot decide — the world sum there is a floor, never a
/// ceiling, so the comparison decides only when every reachable pool pair
/// agrees (the Cmp arm's corner rule). Kleene composition lifts the
/// three-valued count arms through NOT/AND/OR unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticTruth {
    True,
    False,
    Unknown,
}

impl StaticTruth {
    /// Kleene negation — NOT is exact over the two exact values and keeps
    /// an Unknown Unknown.
    fn not(self) -> Self {
        match self {
            StaticTruth::True => StaticTruth::False,
            StaticTruth::False => StaticTruth::True,
            StaticTruth::Unknown => StaticTruth::Unknown,
        }
    }

    /// Kleene conjunction — False decides, True requires both.
    fn and(self, b: Self) -> Self {
        match (self, b) {
            (StaticTruth::False, _) | (_, StaticTruth::False) => StaticTruth::False,
            (StaticTruth::True, StaticTruth::True) => StaticTruth::True,
            _ => StaticTruth::Unknown,
        }
    }

    /// Kleene disjunction — True decides, False requires both.
    fn or(self, b: Self) -> Self {
        match (self, b) {
            (StaticTruth::True, _) | (_, StaticTruth::True) => StaticTruth::True,
            (StaticTruth::False, StaticTruth::False) => StaticTruth::False,
            _ => StaticTruth::Unknown,
        }
    }
}

/// Evaluate a static predicate as a [`StaticTruth`]. The `Exists` /
/// `ExistsQuery` arms and every bounded count-form comparison are exact
/// two-valued (a plain bool comparison); Kleene composition lifts the
/// three-valued count and sum arms through NOT/AND/OR unchanged. `bound`
/// is the window model the `total(bound, …)` scope sums — `None` reads that
/// scope Unknown (which only ever demotes).
pub(crate) fn eval_static_truth(
    p: &Pred,
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> Result<StaticTruth> {
    match p {
        Pred::And(a, b) => {
            let va = eval_static_truth(a, world, bound)?;
            if va == StaticTruth::False {
                return Ok(StaticTruth::False);
            }
            Ok(va.and(eval_static_truth(b, world, bound)?))
        }
        Pred::Or(a, b) => {
            let va = eval_static_truth(a, world, bound)?;
            if va == StaticTruth::True {
                return Ok(StaticTruth::True);
            }
            Ok(va.or(eval_static_truth(b, world, bound)?))
        }
        Pred::Not(a) => Ok(eval_static_truth(a, world, bound)?.not()),
        Pred::Cmp { l, op, r } => cmp_interval_truth(l, op, r, world, bound),
        Pred::Exists { name } => Ok(if world.by_name.contains_key(name) {
            StaticTruth::True
        } else {
            StaticTruth::False
        }),
        Pred::ExistsQuery { query } => {
            Ok(if world.distinct.iter().any(|d| match_query(&d.e, query)) {
                StaticTruth::True
            } else {
                StaticTruth::False
            })
        }
        // preserved as Err (catchable upstream)
        _ => Err(ChorusError::new("internal: eval_static_truth over a non-static predicate")),
    }
}

/// Evaluate a static predicate against the available world. The bool face
/// of [`eval_static_truth`]: Unknown reads false — every caller demotes on
/// false, so Unknown keeps the site unproven (the honest candidate), never
/// flips a verdict upward.
pub(crate) fn eval_static(
    p: &Pred,
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> Result<bool> {
    Ok(eval_static_truth(p, world, bound)? == StaticTruth::True)
}

/// The count/sum-form comparison leaf, decided over the OPERAND INTERVALS:
/// an unbounded matching pool opens its side's ceiling (the world sum there
/// is a floor, never a ceiling). The verdict is the interval decision —
/// True when the comparison holds at the WORST corner pair (the floors, so
/// growth never un-holds it), False when it holds at NO pair (even the
/// ceiling corners fail — a pool can only grow, so a comparison false at
/// the floor of an upward-closed pool stays false), Unknown otherwise. The
/// invariant over open pools: Unknown may only widen toward "candidate",
/// never produce "proven" on its own. Bounded operands are single-point
/// intervals, where this is exactly the capped-weight comparison. Shared by
/// [`eval_static_truth`] and [`ambient_gate_truth`].
fn cmp_interval_truth(
    l: &ValExpr,
    op: &CmpOp,
    r: &ValExpr,
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> Result<StaticTruth> {
    let (lv, lhi) = eval_expr_interval(l, world, bound)?;
    let (rv, rhi) = eval_expr_interval(r, world, bound)?;
    // the open faces: an open ceiling is the floor's infinity
    // stand-in (world counts never reach i64::MAX, so every
    // comparison direction survives the substitution)
    let l_lo = lv;
    let l_hi = lhi.unwrap_or(INFINITY);
    let r_lo = rv;
    let r_hi = rhi.unwrap_or(INFINITY);
    // the monotone ops decide on the corner pairs; the point ops
    // (= and !=) are not monotone — they decide only on the
    // interval SHAPES (equality needs two exact points; interval
    // disjointness is the everywhere-face of both)
    let exact_pair = l_lo == l_hi && r_lo == r_hi;
    let disjoint = l_hi < r_lo || r_hi < l_lo;
    let everywhere = match op {
        CmpOp::Ge => l_lo >= r_hi,
        CmpOp::Gt => l_lo > r_hi,
        CmpOp::Le => l_hi <= r_lo,
        CmpOp::Lt => l_hi < r_lo,
        CmpOp::Eq => exact_pair && l_lo == r_lo,
        CmpOp::Ne => disjoint,
    };
    let nowhere = match op {
        CmpOp::Ge => l_hi < r_lo,
        CmpOp::Gt => l_hi <= r_lo,
        CmpOp::Le => l_lo > r_hi,
        CmpOp::Lt => l_lo >= r_hi,
        CmpOp::Eq => disjoint,
        CmpOp::Ne => exact_pair && l_lo == r_lo,
    };
    Ok(if everywhere {
        StaticTruth::True
    } else if nowhere {
        StaticTruth::False
    } else {
        StaticTruth::Unknown
    })
}

/// §6.9: one operand's EMPTY-BINDINGS face — the exact value an ambient
/// walk's comparison operand reads over the empty scope (`select_recipe`'s
/// bindings are empty there). The role forms are exact: a count or total
/// over a role reduces over no cards and reads 0, and `total(bound, …)`
/// sums no window and reads 0. Everything else reaching this arm (a role
/// path, the dice operand) resolves nothing and reads None — the
/// comparison then reads false, exactly the runtime lenient face
/// (`eval_pred`'s Cmp: either side None → false). The static world forms
/// never reach this arm (they take the interval path).
fn ambient_operand(x: &ValExpr) -> Option<i64> {
    match x {
        ValExpr::Int { v } => Some(*v),
        ValExpr::Count { .. } | ValExpr::Total { .. } | ValExpr::TotalBound { .. } => Some(0),
        _ => None,
    }
}

/// §6.9: the ambient gate's three-valued truth — the §6.4 walk an ambient
/// sweep runs, over EMPTY bindings, mirrored onto the analyzer's card
/// world. The role forms decide EXACTLY (the empty scope: `rel` pairs no
/// cards, `any` sees none and reads false, `every` is vacuously true, a
/// role path resolves to None so its comparison reads false, and a
/// count/total over a role reads 0 — each the runtime face of
/// `eval_pred` over empty bindings); the world forms keep the interval
/// truth of [`eval_static_truth`] (a mixed comparison — a world form
/// against a role form — reads its role side unresolved and decides False,
/// the sound under-claim); Kleene composition lifts. Unknown may only
/// widen toward candidate (the completing invariant) — a decided FALSE is
/// the recipe that never opens, and the ladder demotes it exactly like a
/// player recipe's non-static-true gate.
pub(crate) fn ambient_gate_truth(
    p: &Pred,
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> StaticTruth {
    match p {
        Pred::And(a, b) => {
            ambient_gate_truth(a, world, bound).and(ambient_gate_truth(b, world, bound))
        }
        Pred::Or(a, b) => {
            ambient_gate_truth(a, world, bound).or(ambient_gate_truth(b, world, bound))
        }
        Pred::Not(a) => ambient_gate_truth(a, world, bound).not(),
        Pred::Rel { .. } | Pred::Any { .. } => StaticTruth::False,
        Pred::Every { .. } => StaticTruth::True,
        Pred::Exists { .. } | Pred::ExistsQuery { .. } => {
            eval_static_truth(p, world, bound).unwrap_or(StaticTruth::False)
        }
        Pred::Cmp { l, op, r } => {
            if expr_static(l) && expr_static(r) {
                return cmp_interval_truth(l, op, r, world, bound).unwrap_or(StaticTruth::False);
            }
            match (ambient_operand(l), ambient_operand(r)) {
                (Some(a), Some(b)) => {
                    if op.num_cmp(a, b) {
                        StaticTruth::True
                    } else {
                        StaticTruth::False
                    }
                }
                _ => StaticTruth::False,
            }
        }
    }
}

/// The path-open walk, reporting its evidence: the FIRST path
/// condition that is not a static truth (non-static, or static and false) —
/// `Some(p)` is the demotion verdict, `None` exactly when every path
/// condition is a static truth. The fixpoint's ladder hands the predicate
/// to the design-time renderer, so the PathClosed blocker and the demotion
/// verdict come from this ONE pass; there is no second evaluation to drift
/// away from.
pub(crate) fn path_open_first_failure<'p>(
    path: &'p [Pred],
    world: &CardWorld,
    bound: Option<&BoundView<'_>>,
) -> Option<&'p Pred> {
    for p in path {
        if pred_static(p) && eval_static_ok(p, world, bound) {
            continue;
        }
        return Some(p);
    }
    None
}

/// `eval_static` with the internal-throw guard: the non-static operands sit
/// behind `pred_static` guards and are unreachable; debug-trap if a bug ever
/// reaches one.
pub(crate) fn eval_static_ok(p: &Pred, world: &CardWorld, bound: Option<&BoundView<'_>>) -> bool {
    match eval_static(p, world, bound) {
        Ok(b) => b,
        Err(e) => {
            debug_assert!(false, "internal: {e}");
            false
        }
    }
}

/// The writable paths a query's aspect/state thresholds
/// demand, in SET-key form ('aspect:x' / 'state.x'). The UNION across
/// alternation groups (§4.5) — this feeds the possibility analysis, where
/// any single group's demand being pumpable matters.
pub(crate) fn writable_paths(query: &Query) -> Vec<String> {
    let mut out = Vec::new();
    for group in query {
        out.extend(writable_paths_of(group));
    }
    out
}

/// The writable paths ONE conjunction group demands.
pub(crate) fn writable_paths_of(group: &[QueryTerm]) -> Vec<String> {
    group
        .iter()
        .filter_map(|t| match t {
            QueryTerm::Aspect { neg: false, path, .. } => Some(format!("aspect:{path}")),
            QueryTerm::State { neg: false, key, .. } => Some(format!("state.{key}")),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the structured committability proof
// ---------------------------------------------------------------------------

/// A slot filler: the `name` and `how` it was obtained.
#[derive(Debug, Clone)]
pub(crate) struct SlotFiller {
    pub name: String,
    pub how: String,
}

/// One filled slot of a verb's committability proof.
#[derive(Debug, Clone)]
pub(crate) struct SlotProof {
    pub role: String,
    pub min: i64,
    pub fillers: Vec<SlotFiller>,
}

// ---------------------------------------------------------------------------
// the available-card fact
// ---------------------------------------------------------------------------

/// One available card: a static descriptor + how many
/// live copies it can contribute.
#[derive(Debug, Clone)]
pub(crate) struct CardFact {
    pub entity: Entity,
    pub count: i64,
    pub how: String,
    /// The source repeats UNCONDITIONALLY — a stackable mint behind a bare
    /// (no `when`, no IF conditions) list owned by an uncapped recipe or a
    /// recurring trigger, or an endless reset/defaultcard draw behind such
    /// a bare uncapped list — so `count` is a floor, not a ceiling: any
    /// number of copies is arrangeable. The bounded sources stay exact: a
    /// `max` cap, a once trigger's single firing and a REPLACE's consumed
    /// target all bound their count. Only the count-form world gates read
    /// this (the [`CardWorld`] weight stays the capped flatten; an
    /// unbounded matching fact lifts the count interval's ceiling off).
    pub unbounded: bool,
}

// ---------------------------------------------------------------------------
// WorldFacts
// ---------------------------------------------------------------------------

/// Everything target-independent the per-card analysis reads. Deterministic
/// in (content, initial) — the analyzer never
/// mutates either. Identity keys are indices: `committable` /
/// `fillers` are keyed by the content.verbs insertion index, `proven` /
/// `mint_sources` by the census site arena index; every insertion-ordered
/// Map/Set is an IndexMap/IndexSet (avail push order → `slot_support`
/// candidate order → first-fit proofs; proven order = proof order → chain
/// first-hit and paths order).
pub(crate) struct WorldFacts<'c> {
    pub content: &'c CompiledContent,
    pub initial: &'c State,
    pub trig_idx: TrigIndex<'c>,
    /// the site census (arena + live/refused index lists)
    pub census: Census<'c>,
    /// available cards by name, fact lists in push order
    pub avail: IndexMap<String, Vec<CardFact>>,
    pub avail_ver: u64,
    /// verbs owning a recipe some authored `RUN`/`FORK` targets (the §6.7
    /// chain edges — [`chain_pinned_verbs`]). Their `when` gates are
    /// re-evaluated at play over MERGED bindings no declared-slot model
    /// bounds, so [`WorldFacts::bound_view`] reads no window model for them.
    pub chain_pinned: IndexSet<usize>,
    /// the flat_cache — exact mechanics preserved including its
    /// version stamp; `Rc` because every [`WorldFacts::cards_of`] call
    /// hands the SAME immutable snapshot to the whole round
    pub flat_cache: Option<Rc<CardWorld>>,
    /// one representative per name (descriptors; later-wins over
    /// State.entities order — same-name stackable mints in mid-game States)
    pub init_names: IndexMap<String, &'c Entity>,
    /// trigger owner → its non-stackable mint names (hoisted for `site_proven`'s
    /// trigger-vs-recipe rule, 81915)
    pub trigger_mint_names: IndexMap<String, IndexSet<String>>,
    pub set_writes: IndexSet<String>,
    /// trigger owners with a deterministic refusal at their slot
    pub risky_owners: IndexSet<String>,
    pub frozen: bool,
    /// the freeze boundary in FIRST-CROSSING distance units; [`INFINITY`]
    /// when unfrozen
    pub freeze_dist: i64,
    /// the `--by` bound: the AUTHORED absolute minute (engine clock, §7)
    /// the analysis window closes at; `None` on the unbounded default face.
    /// Every time-keyed rule reads the horizon it induces
    /// ([`WorldFacts::horizon`]) — never this field alone.
    pub by: Option<i64>,
    /// owners of ONCE slot triggers already fired in the replayed log —
    /// they never fire again, so their sites demote (AlreadyFired).
    /// Empty on the default face (script states carry no fired set).
    pub fired_once_owners: IndexSet<String>,
    /// owners whose every draft is refused at completion
    pub dirty_owners: IndexSet<String>,
    /// world-level notes, converging on the named card's report (the
    /// §13.1 revision: a note naming a card rides only on THAT name's
    /// report; name-free notes ride on every report — the filter is
    /// `paths::world_notes_for`, applied per analyze_card)
    pub notes: Vec<String>,
    /// verb index → its structured committability proof (proof order)
    pub committable: IndexMap<usize, Vec<SlotProof>>,
    /// site arena index → support lines (proof order)
    pub proven: IndexMap<usize, Vec<String>>,
    /// bare name → PROVEN sites minting it, in proof order (`mint_sources`)
    pub mint_sources: IndexMap<String, Vec<usize>>,
    /// verb index → names its committability proof filled slots with
    pub fillers: IndexMap<usize, IndexSet<String>>,
    /// the universe of cards that can ever exist (static descriptors)
    pub universe: Vec<Entity>,
    /// an authored aspects= pair we cannot parse — cannot bound
    pub opaque_spawn_aspects: bool,
    pub derive_paths: IndexSet<String>,
    /// verbs still uncommittable at fixpoint whose min>0 slots can never be
    /// filled (by verb index)
    pub dead_verbs: IndexSet<usize>,
    /// deck id → capacity from proven sites
    pub deck_cap: IndexMap<String, i64>,
}

impl<'c> WorldFacts<'c> {
    /// The EFFECTIVE analysis horizon in first-crossing distance units:
    /// [`INFINITY`] unbounded, otherwise `min(freeze_dist, by_dist + 1)` —
    /// the freeze boundary and the `--by` window take the tighter, and
    /// `by_dist + 1` turns "fired by minute M" into the same strict
    /// `first_cross < horizon` comparison the freeze model already speaks
    /// (`first_cross <= M − now` ⇔ `first_cross < M − now + 1`). The
    /// default face passes `by: None`, so the horizon IS the freeze
    /// distance and every rule below reads it byte-identically to the
    /// pre-`--by` code.
    pub(crate) fn horizon(&self) -> i64 {
        match self.by {
            None => self.freeze_dist,
            Some(m) => self.freeze_dist.min(m.saturating_sub(self.initial.now).saturating_add(1)),
        }
    }

    /// Minutes until the FIRST crossing of slot s (a
    /// start-slot trigger's first crossing is the next cycle — boundary ==
    /// now is already spent). The `d === 0 → 1440` branch is part of the
    /// semantics.
    pub(crate) fn first_cross(&self, slot: i64) -> i64 {
        let d = (slot * MIN_PER_TICK - self.initial.now).rem_euclid(TICKS_PER_DAY * MIN_PER_TICK);
        if d == 0 {
            TICKS_PER_DAY * MIN_PER_TICK
        } else {
            d
        }
    }

    /// The declared prototype for a name, if any.
    pub(crate) fn proto_of(&self, name: &str) -> Option<&'c PrototypeDef> {
        self.content.prototypes.get(name)
    }

    /// Synthesize a static entity descriptor (id
    /// `~<name>`; `born_action` 0; `dead_action` None).
    pub(crate) fn synth(
        &self,
        name: &str,
        kind: &str,
        proto: Option<&PrototypeDef>,
        aspects_override: Option<Aspects>,
    ) -> Entity {
        Entity {
            id: format!("~{name}"),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: aspects_override
                .or_else(|| proto.and_then(|p| p.aspects.clone()))
                .unwrap_or_default(),
            attrs: proto.and_then(|p| p.attrs.clone()).unwrap_or_default(),
            born_action: Some(0),
            dead_action: None,
            stackable: proto.is_some_and(|p| p.stackable),
            // §4.2: the same resolved-name lookup the engine's mint sites run
            desc: self.content.mint_desc(name).map(str::to_string),
        }
    }

    /// `cards_of`, over the flat_cache. Exact
    /// mechanics: [`WorldFacts::push`] bumps `avail_ver` without touching the
    /// cache's `ver`, so every push invalidates and the next call rebuilds from
    /// `avail` (insertion order preserved — the cache never reads warm after a
    /// push). The snapshot is handed out as an `Rc`: it is never mutated
    /// through the handle.
    pub(crate) fn cards_of(&mut self) -> Rc<CardWorld> {
        if let Some(cache) = &self.flat_cache {
            if cache.ver == self.avail_ver {
                return Rc::clone(cache);
            }
        }
        let cache = Rc::new(self.build_world());
        self.flat_cache = Some(Rc::clone(&cache));
        cache
    }

    /// The cache rebuild `cards_of` performs, factored out: the read-only
    /// form the post-fixpoint diagnostics use (`&self` cannot install the
    /// rebuilt snapshot). The build is a pure function of `avail`, so this
    /// is observationally identical to a warm `cards_of` after the fixpoint
    /// settles.
    pub(crate) fn build_world(&self) -> CardWorld {
        let mut by_name: IndexMap<String, i64> = IndexMap::new();
        let mut distinct: Vec<DistEntry> = Vec::new();
        for list in self.avail.values() {
            for f in list {
                let unique = self.content.mint_is_unique(&f.entity.name);
                // §4.2 unique: at most ONE living copy of a unique name can
                // ever exist (the mint face skips while one lives) — the
                // pooled total clamps at 1 and only the FIRST fact becomes
                // the distinct instance (no slot can ever bind two), whatever
                // the sources' own counts say
                if unique && by_name.contains_key(&f.entity.name) {
                    continue;
                }
                let w = std::cmp::min(f.count, FLATTEN_WEIGHT_CAP);
                let w = if unique { w.min(1) } else { w };
                by_name.insert(
                    f.entity.name.clone(),
                    by_name.get(&f.entity.name).copied().unwrap_or(0) + w,
                );
                distinct.push(DistEntry { e: f.entity.clone(), w, unbounded: f.unbounded });
            }
        }
        CardWorld { ver: self.avail_ver, by_name, distinct }
    }

    /// Add one available-card fact under its name (the `push` mechanics).
    /// A cached-CardWorld patch here would be DEAD WORK — the same statement
    /// sequence bumps `avail_ver`, so the very next [`WorldFacts::cards_of`]
    /// sees `cache.ver != avail_ver` and rebuilds from `avail` before any
    /// reader could observe the patch. Dropping it is output-identical; the
    /// ver stamp (the observable part) stays.
    pub(crate) fn push(&mut self, name: String, fact: CardFact) {
        match self.avail.get_mut(&name) {
            Some(list) => list.push(fact),
            None => {
                self.avail.insert(name, vec![fact]);
            }
        }
        self.avail_ver += 1;
    }
}

// ---------------------------------------------------------------------------
// Analyzer — the per-(content, initial) host; per-fact caches are unnecessary
// ---------------------------------------------------------------------------

/// The per-(content, initial) analysis host. Deterministic in its inputs; the
/// world facts are computed once on first use and shared by every subsequent
/// [`Analyzer::analyze_card`] call. The
/// caller must not mutate content or initial between calls.
///
/// The dictionary rides along for the load-time grammars the analyzer
/// re-parses (a `--demand` §4.5 query validates its `kind:` terms against
/// it, §4.1) — runtime analysis itself stays vocabulary-free.
///
/// The two bounded-time faces ride the HOST, not `CardOptions`: one
/// Analyzer owns exactly one world, so the bound cannot drift between
/// cards of one run (the CLI sets it once, before the first analysis).
/// `by` is the `--by` absolute minute; `replayed` marks a `--from-state`
/// initial world (the fidelity note). Both default to the unbounded
/// script face.
pub struct Analyzer<'c> {
    pub(crate) content: &'c CompiledContent,
    pub(crate) initial: &'c State,
    pub(crate) dict: &'c Dictionary,
    pub(crate) by: Option<i64>,
    pub(crate) replayed: bool,
    world: OnceLock<WorldFacts<'c>>,
}

impl<'c> Analyzer<'c> {
    #[must_use]
    pub fn new(content: &'c CompiledContent, initial: &'c State, dict: &'c Dictionary) -> Self {
        Analyzer { content, initial, dict, by: None, replayed: false, world: OnceLock::new() }
    }

    /// The `--by <minute>` face: bound every time-keyed judgment at the
    /// authored absolute minute. Must be set before the first analysis
    /// (the world facts are cached once).
    #[must_use]
    pub fn by(mut self, minute: i64) -> Self {
        self.by = Some(minute);
        self
    }

    /// The `--from-state` face: the initial world is a log replay — the
    /// fidelity note rides on every report. Must be set before the first
    /// analysis.
    #[must_use]
    pub fn replayed(mut self) -> Self {
        self.replayed = true;
        self
    }

    /// The world facts (compute once per (content, initial) pair, under
    /// this Analyzer's bound).
    pub(crate) fn facts(&self) -> &WorldFacts<'c> {
        let (by, replayed) = (self.by, self.replayed);
        self.world.get_or_init(|| compute_world_facts(self.content, self.initial, by, replayed))
    }
}

// ---------------------------------------------------------------------------
// compute_world_facts — the pre-fixpoint half
// ---------------------------------------------------------------------------

/// The §6.7 chain edges as a verb set: every verb owning a recipe that some
/// authored `RUN`/`FORK` targets (recipe bodies and trigger bodies alike, IF
/// branches included — candidates resolve against the GLOBAL recipe-id
/// table). A chain-pinned candidate's `when` is re-evaluated at play over
/// MERGED bindings — the parent window's committed roles stand under roles
/// the candidate does not declare, and chain-synthesized follow-ups skip
/// `validate_slots` — so no declared-slot model bounds the gate's domain.
fn chain_pinned_verbs(content: &CompiledContent) -> IndexSet<usize> {
    let mut pinned: IndexSet<usize> = IndexSet::new();
    let mut scan = |out: &[Outcome]| {
        for_each_authored_outcome_infallible(out, &mut |o| {
            if o.op != "RUN" && o.op != "FORK" {
                return;
            }
            for a in &o.args {
                if let Arg::Ref { r#ref: Ref::Name { name, .. } } = a {
                    let Some(vn) = content.recipe_verbs.get(name.as_str()) else {
                        continue;
                    };
                    if let Some((vi, _, _)) = content.verbs.get_full(vn.as_str()) {
                        pinned.insert(vi);
                    }
                }
            }
        });
    };
    for (_, v) in &content.verbs {
        for r in &v.recipes {
            scan(&r.out);
        }
    }
    for t in &content.triggers {
        scan(t.out());
    }
    pinned
}

pub(crate) fn compute_world_facts<'c>(
    content: &'c CompiledContent,
    initial: &'c State,
    by: Option<i64>,
    replayed: bool,
) -> WorldFacts<'c> {
    let trig_idx = TrigIndex::build(content);

    // the midgame face's spent-once set: once slot triggers the replayed
    // log already fired never fire again (§6.6 — the fold marks fired from
    // each EVENT row's trigger id, the same raw id the census keys). The
    // default face reads an empty fired set, so this stays empty there.
    let fired_once_owners: IndexSet<String> = content
        .triggers
        .iter()
        .filter_map(|t| match t {
            CompiledTrigger::Slot { id, once: true, .. } if initial.fired.contains(id.as_str()) => {
                Some(trigger_owner(id))
            }
            _ => None,
        })
        .collect();

    // initial living entities, grouped by name; the
    // LAST entity with a name wins the representative slot (insertion keeps
    // the original insertion position — IndexMap::insert likewise).
    let mut init_names: IndexMap<String, &'c Entity> = IndexMap::new();
    let mut init_count: IndexMap<String, i64> = IndexMap::new();
    for e in initial.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        init_names.insert(e.name.clone(), e);
        *init_count.entry(e.name.clone()).or_insert(0) += 1;
    }

    let mut wf = WorldFacts {
        content,
        initial,
        trig_idx,
        census: Census::default(),
        avail: IndexMap::new(),
        avail_ver: 0,
        chain_pinned: chain_pinned_verbs(content),
        flat_cache: None,
        init_names,
        trigger_mint_names: IndexMap::new(),
        set_writes: IndexSet::new(),
        risky_owners: IndexSet::new(),
        frozen: false,
        freeze_dist: INFINITY,
        by,
        fired_once_owners,
        dirty_owners: IndexSet::new(),
        notes: Vec::new(),
        committable: IndexMap::new(),
        proven: IndexMap::new(),
        mint_sources: IndexMap::new(),
        fillers: IndexMap::new(),
        universe: Vec::new(),
        opaque_spawn_aspects: false,
        derive_paths: IndexSet::new(),
        dead_verbs: IndexSet::new(),
        deck_cap: IndexMap::new(),
    };

    // one avail fact per representative name
    let pushes: Vec<(String, CardFact)> = wf
        .init_names
        .iter()
        .map(|(name, e)| {
            (
                name.clone(),
                CardFact {
                    entity: (*e).clone(),
                    count: init_count.get(name).copied().unwrap_or(1),
                    how: format!("initial entity {name} ({})", e.kind),
                    unbounded: false,
                },
            )
        })
        .collect();
    for (name, fact) in pushes {
        wf.push(name, fact);
    }

    // the census (collision_names seeded with the
    // initial non-stackable names inside)
    let census = collect_sites(content, &wf.init_names, &wf.trig_idx);
    wf.census = census;
    // the SET-written paths
    wf.set_writes = wf.census.set_sites.iter().map(|s| s.key.clone()).collect();

    // the risky-trigger demotions (list-level mint
    // risk, same-slot pair rule, cross-slot timing, freeze line)
    risky_block(&mut wf);

    // dirty_owners
    for &i in wf.census.live.iter().chain(&wf.census.refused) {
        let s = &wf.census.sites[i];
        if s.dup_risk {
            wf.dirty_owners.insert(s.owner.clone());
        }
    }
    // the freeze line: FIRST-CROSSING distance units,
    // NOT cyclic order (seed 65415 shape)
    let risky_triggers: Vec<&CompiledTrigger> = wf
        .content
        .triggers
        .iter()
        .filter(|t| {
            matches!(t, CompiledTrigger::Slot { .. })
                && wf.risky_owners.contains(&trigger_owner(t.id()))
        })
        .collect();
    wf.frozen = !risky_triggers.is_empty();
    if wf.frozen {
        debug_assert!(!risky_triggers.is_empty(), "frozen ⇒ at least one risky trigger");
        wf.freeze_dist = risky_triggers
            .iter()
            .map(|t| match t {
                CompiledTrigger::Slot { slot, .. } => wf.first_cross(*slot),
                CompiledTrigger::Expire { .. } | CompiledTrigger::Complete { .. } => {
                    unreachable!("risky_triggers filters to slot triggers")
                }
            })
            .min()
            .unwrap_or(INFINITY);
    }
    // the refused note
    if !wf.census.refused.is_empty() {
        wf.notes.push(format!(
            "{} producer site(s) sit behind an authored REFUSE — they can never apply and are excluded",
            wf.census.refused.len()
        ));
    }

    // the midgame face's fidelity note (name-free ⇒ global — it rides on
    // every report): what the replay models beyond the script face, and
    // the state fields read by the engine but deliberately unmodeled.
    if replayed {
        let cursors: Vec<(String, i64)> = initial
            .decks
            .iter()
            .filter(|(id, _)| content.decks.get(*id).is_some_and(|d| !d.reset))
            .map(|(id, c)| (id.clone(), *c))
            .collect();
        wf.notes.push(format!(
            "midgame analysis: the initial world is replayed from a log — modeled: the replayed entities and clock (first crossings re-anchor to minute {}), {} already-fired once trigger(s) (they never fire again), {} non-reset deck cursor(s) (draws serve from the cursor forward); read but not modeled: open windows (the lock table), the executed-recipe map, the rng counter",
            initial.now,
            wf.fired_once_owners.len(),
            cursors.len(),
        ));
    }

    // the fixpoint (naive round-robin)
    super::fixpoint::run_fixpoint(&mut wf);

    // universe, dead verbs, deck caps
    super::paths::build_post_fixpoint(&mut wf);

    wf
}

/// The risky block: list-level mint risk against the
/// INITIAL world and CROSS-LIST trigger timing. Mutates census sites
/// (`dup_risk`) through arena indices (the shared per-list site objects,
/// flattened into the arena). The stages run in exactly this order and each
/// owns one rule set:
///
/// 1. [`RiskySets::build`] — the per-owner grouping and mint sets
/// 2. [`initial_collision_risky`] — initial-world mint collisions (seeds 40/43)
/// 3. [`slot_pair_risky`] — same-slot mint pairs + the event-pass kill
/// 4. [`cross_slot_risky`] — later-first-crossing demotions (seed 7475)
/// 5. [`gated_arm_demotions`] — roll/role-gated mint arms (seed 686)
/// 6. [`throw_freeze_set`] — deterministic-throw freeze (seeds 2876, 10388)
/// 7. [`first_freeze_at`] — the freeze boundary (seed 65415 shape)
/// 8. [`boundary_demotions`] — pass-mates of a freezing trigger
/// 9. [`demote_risky_owners`] — the final demotions + `trigger_mint_names`
fn risky_block(wf: &mut WorldFacts) {
    let mut sets = RiskySets::build(wf);
    initial_collision_risky(wf, &mut sets);
    slot_pair_risky(wf, &mut sets);
    cross_slot_risky(wf, &mut sets);
    gated_arm_demotions(wf, &sets);
    let throw_frozen = throw_freeze_set(wf, &sets);
    let freeze_at = first_freeze_at(wf, &sets, &throw_frozen);
    boundary_demotions(wf, &sets, &throw_frozen, freeze_at);
    demote_risky_owners(wf, &sets, &throw_frozen);
}

/// The owner's site indices, empty when the owner has none.
fn idxs<'m>(m: &'m IndexMap<String, Vec<usize>>, k: &str) -> &'m [usize] {
    m.get(k).map_or(&[], Vec::as_slice)
}

/// Collision-filtered mint names of a site list.
fn mint_set_of(census: &Census, sites: &[usize]) -> IndexSet<String> {
    let mut names = IndexSet::new();
    for &i in sites {
        let s = &census.sites[i];
        if s.kind == SiteKind::Draw && s.deck.is_some() {
            for (n, _) in site_mints(s) {
                if census.collision_names.contains(n) {
                    names.insert(n.to_string());
                }
            }
        } else if census.collision_names.contains(&s.name) {
            names.insert(s.name.clone());
        }
    }
    names
}

/// The per-owner bookkeeping the risky stages share: the live ∪ refused site
/// indices grouped by owner, each owner's collision-filtered mint set (built
/// once — the pair stages consult it repeatedly), and the running risky flags.
struct RiskySets {
    by_list: IndexMap<String, Vec<usize>>,
    owner_mints: IndexMap<String, IndexSet<String>>,
    risky: IndexMap<String, bool>,
}

impl RiskySets {
    fn build(wf: &WorldFacts) -> Self {
        // by_list: live ∪ refused grouped by owner
        let mut by_list: IndexMap<String, Vec<usize>> = IndexMap::new();
        for &i in wf.census.live.iter().chain(&wf.census.refused) {
            let owner = wf.census.sites[i].owner.clone();
            by_list.entry(owner).or_default().push(i);
        }
        let mut owner_mints: IndexMap<String, IndexSet<String>> = IndexMap::new();
        for (owner, sites) in &by_list {
            owner_mints.insert(owner.clone(), mint_set_of(&wf.census, sites));
        }
        // every trigger owner is a query candidate of the stages below;
        // owners without sites read as the empty set
        for t in &wf.content.triggers {
            owner_mints.entry(trigger_owner(t.id())).or_default();
        }
        RiskySets { by_list, owner_mints, risky: IndexMap::new() }
    }

    /// The owner's collision-filtered mint set. Every queried owner is a
    /// by_list key or a trigger owner, both seeded by [`RiskySets::build`].
    fn mints(&self, owner: &str) -> &IndexSet<String> {
        match self.owner_mints.get(owner) {
            Some(m) => m,
            None => unreachable!("owner '{owner}' was not seeded by RiskySets::build"),
        }
    }
}

/// A list minting the name of a living initial
/// non-stackable card collides on its first commit (seeds 40/43)
fn initial_collision_risky(wf: &WorldFacts, sets: &mut RiskySets) {
    for (owner, _) in &sets.by_list {
        let val =
            sets.mints(owner).iter().any(|n| wf.init_names.get(n).is_some_and(|e| !e.stackable));
        sets.risky.insert(owner.clone(), val);
    }
}

/// Slot triggers by slot, declaration order.
/// Same-slot pairs: a shared mint name makes the
/// pair mutually risky, and one risky trigger kills the whole EVENT PASS
fn slot_pair_risky(wf: &WorldFacts, sets: &mut RiskySets) {
    let mut trig_by_slot: IndexMap<i64, Vec<String>> = IndexMap::new();
    for t in &wf.content.triggers {
        if let CompiledTrigger::Slot { id, slot, .. } = t {
            trig_by_slot.entry(*slot).or_default().push(trigger_owner(id));
        }
    }
    for owners in trig_by_slot.values() {
        for i in 0..owners.len() {
            for j in (i + 1)..owners.len() {
                let (a, b) = (sets.mints(&owners[i]), sets.mints(&owners[j]));
                if a.iter().any(|n| b.contains(n)) {
                    sets.risky.insert(owners[i].clone(), true);
                    sets.risky.insert(owners[j].clone(), true);
                }
            }
        }
        if owners.iter().any(|o| sets.risky.get(o) == Some(&true)) {
            for o in owners {
                sets.risky.insert(o.clone(), true);
            }
        }
    }
}

/// Across DIFFERENT slots the LATER first crossing
/// demotes (fuzz seed 7475). trig_flat's sort is stability-irrelevant:
/// equal first-cross pairs are skipped below.
fn cross_slot_risky(wf: &WorldFacts, sets: &mut RiskySets) {
    let mut trig_flat: Vec<(String, i64)> = wf
        .content
        .triggers
        .iter()
        .filter_map(|t| match t {
            CompiledTrigger::Slot { id, slot, .. } => {
                Some((trigger_owner(id), wf.first_cross(*slot)))
            }
            _ => None,
        })
        .collect();
    trig_flat.sort_by_key(|x| x.1);
    for i in 0..trig_flat.len() {
        for j in (i + 1)..trig_flat.len() {
            if trig_flat[i].1 == trig_flat[j].1 {
                continue; // same slot — handled above (both risky)
            }
            let (a, b) = (sets.mints(&trig_flat[i].0), sets.mints(&trig_flat[j].0));
            if a.iter().any(|n| b.contains(n)) {
                sets.risky.insert(trig_flat[j].0.clone(), true);
            }
        }
    }
}

/// The gated-branch escape set: every non-stackable
/// mint name anywhere (initial copies included). Then the
/// roll/role-gated mint arms join the risky set
/// when their mint name is ALSO minted elsewhere (fuzz seed 686).
fn gated_arm_demotions(wf: &mut WorldFacts, sets: &RiskySets) {
    let mut other_mints: IndexSet<String> = IndexSet::new();
    for e in wf.init_names.values() {
        if !e.stackable {
            other_mints.insert(e.name.clone());
        }
    }
    for sites in sets.by_list.values() {
        for &i in sites {
            let s = &wf.census.sites[i];
            if s.derives && object_always(wf.content, s.verb) {
                continue; // derived mints carry a different name
            }
            if !wf.census.collision_names.contains(&s.name) {
                continue;
            }
            other_mints.insert(s.name.clone());
        }
    }
    for (owner_o, sites_o) in &sets.by_list {
        let gated: Vec<String> = sites_o
            .iter()
            .filter_map(|&i| {
                let s = &wf.census.sites[i];
                (s.runtime_gated && wf.census.collision_names.contains(&s.name))
                    .then(|| s.name.clone())
            })
            .collect();
        if gated.is_empty() {
            continue;
        }
        let elsewhere = other_mints.iter().any(|n| {
            if gated.contains(n) {
                // also minted by a DIFFERENT list?
                sets.by_list.iter().any(|(o2, ss2)| {
                    o2 != owner_o
                        && ss2.iter().any(|&i2| {
                            let s2 = &wf.census.sites[i2];
                            s2.name == *n && !(s2.runtime_gated && gated.contains(&s2.name))
                        })
                })
            } else {
                true
            }
        });
        if elsewhere {
            for &i in sites_o {
                wf.census.sites[i].dup_risk = true;
            }
        }
    }
}

/// Whether the owner's list is demoted outright: flagged risky, or carrying
/// any site already marked `dup_risk`.
fn list_blocked(wf: &WorldFacts, sets: &RiskySets, owner: &str) -> bool {
    if sets.risky.get(owner) == Some(&true) {
        return true;
    }
    idxs(&sets.by_list, owner).iter().any(|&i| wf.census.sites[i].dup_risk)
}

/// The owner's first firing is blocked outright (list-blocked or
/// deterministically throw-frozen).
fn first_firing_throws(
    wf: &WorldFacts,
    sets: &RiskySets,
    throw_frozen: &IndexSet<String>,
    owner: &str,
) -> bool {
    list_blocked(wf, sets, owner) || throw_frozen.contains(owner)
}

/// Deterministic-throw freeze (seeds 2876, 10388): slot triggers whose
/// REPLACE target dies in-list or by an earlier-determined other owner.
fn throw_freeze_set(wf: &WorldFacts, sets: &RiskySets) -> IndexSet<String> {
    let mut throw_frozen: IndexSet<String> = IndexSet::new();
    let decl_of = |owner: &str| -> i64 { wf.trig_idx.get(owner).map_or(-1, |(_, d)| d) };
    let first_cross_of = |owner: &str| -> Option<i64> {
        match wf.trig_idx.get(owner) {
            Some((CompiledTrigger::Slot { slot, .. }, _)) => Some(wf.first_cross(*slot)),
            _ => None, // recipes commit at player-chosen times
        }
    };
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, .. } = t else {
            continue;
        };
        let owner = trigger_owner(id);
        let my_first = wf.first_cross(*slot);
        let my_decl = decl_of(&owner);
        let replaces: Vec<&RawSite> = idxs(&sets.by_list, &owner)
            .iter()
            .filter_map(|&i| {
                let s = &wf.census.sites[i];
                (s.kind == SiteKind::Replace && s.replace_target.is_some()).then_some(s)
            })
            .collect();
        if replaces.is_empty() {
            continue;
        }
        let self_killed = replaces.iter().any(|s| s.target_killed_in_list);
        let cross_killed = replaces.iter().any(|s| {
            wf.census
                .destroyed_by
                .get(s.replace_target.as_deref().unwrap_or(""))
                .is_some_and(|set| set.iter().any(|o| *o != owner))
        });
        if !self_killed && !cross_killed {
            continue;
        }
        let deterministic = self_killed
            || replaces.iter().any(|s| {
                wf.census.destroyed_by.get(s.replace_target.as_deref().unwrap_or("")).is_some_and(
                    |set| {
                        set.iter().any(|o| {
                            if *o == owner {
                                return false;
                            }
                            match first_cross_of(o) {
                                None => false,
                                Some(fc) => {
                                    fc < my_first || (fc == my_first && decl_of(o) < my_decl)
                                }
                            }
                        })
                    },
                )
            });
        if deterministic {
            throw_frozen.insert(owner);
        }
    }
    throw_frozen
}

/// first_freeze: the first crossing of a blocking
/// boundary freezes the clock there forever. Crossings accumulate
/// deduplicated (key==value set semantics); only their min is ever read.
fn first_freeze_at(wf: &WorldFacts, sets: &RiskySets, throw_frozen: &IndexSet<String>) -> i64 {
    let mut first_freeze: Vec<i64> = Vec::new();
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, once, .. } = t else {
            continue;
        };
        let owner = trigger_owner(id);
        let recurring_minter = !once && !sets.mints(&owner).is_empty();
        if list_blocked(wf, sets, &owner) || recurring_minter {
            first_freeze.push(wf.first_cross(*slot));
        }
    }
    for owner in throw_frozen {
        if let Some((CompiledTrigger::Slot { slot, .. }, _)) = wf.trig_idx.get(owner) {
            first_freeze.push(wf.first_cross(*slot));
        }
    }
    first_freeze.iter().copied().min().unwrap_or(INFINITY)
}

/// Same-slot pass-mates of a freezing trigger never
/// persist either; the exemption is EXACT (recurring minter standing ON
/// the boundary slot, clean first firings only)
fn boundary_demotions(
    wf: &mut WorldFacts,
    sets: &RiskySets,
    throw_frozen: &IndexSet<String>,
    freeze_at: i64,
) {
    let boundary_slot: Option<i64> = if freeze_at != INFINITY {
        wf.content.triggers.iter().find_map(|t| match t {
            CompiledTrigger::Slot { slot, .. } if wf.first_cross(*slot) == freeze_at => Some(*slot),
            _ => None,
        })
    } else {
        None
    };
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, once, .. } = t else {
            continue;
        };
        let owner = trigger_owner(id);
        let fc = wf.first_cross(*slot);
        if fc < freeze_at {
            continue;
        }
        let recurring_minter_self = !once && !sets.mints(&owner).is_empty();
        let on_clean_boundary = match boundary_slot {
            Some(bs) => {
                *slot == bs
                    && recurring_minter_self
                    && !wf.content.triggers.iter().any(|o| {
                        matches!(o, CompiledTrigger::Slot { slot: s2, .. } if *s2 == *slot)
                            && first_firing_throws(wf, sets, throw_frozen, &trigger_owner(o.id()))
                    })
            }
            None => false,
        };
        if !on_clean_boundary {
            for &i in idxs(&sets.by_list, &owner) {
                wf.census.sites[i].dup_risk = true;
            }
        }
    }
}

/// Risky owners, deterministic throwers and draft-condemned triggers demote
/// their lists; `trigger_mint_names` is hoisted for `site_proven`'s
/// trigger-vs-recipe rule
fn demote_risky_owners(wf: &mut WorldFacts, sets: &RiskySets, throw_frozen: &IndexSet<String>) {
    // risky owners and deterministic throwers demote
    for (owner, is_risky) in &sets.risky {
        if !*is_risky {
            continue;
        }
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&sets.by_list, owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    for owner in throw_frozen {
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&sets.by_list, owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    // a draft-condemned trigger refuses deterministically
    // at its slot just the same (fuzz seed 65415)
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, .. } = t else {
            continue;
        };
        let owner = trigger_owner(id);
        if !list_blocked(wf, sets, &owner) {
            continue;
        }
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&sets.by_list, &owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    // hoisted for `site_proven`'s trigger-vs-recipe rule
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, .. } = t else {
            continue;
        };
        let owner = trigger_owner(id);
        let mints = sets.mints(&owner).clone();
        wf.trigger_mint_names.insert(owner, mints);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::script::compile_script;
    use crate::core::dict::core_dictionary;
    use crate::core::types::new_state;

    #[test]
    fn first_cross_wraps_at_the_day_boundary_on_the_tick_grid() {
        let dict = core_dictionary();
        let text = "\
script: First Cross Probe
player: MARCH
start: 0:0

entities:
  MARCH is character

triggers:
  t_edge at tick 143 -> NARRATE \"edge\"
  t_wrap at tick 0 -> NARRATE \"wrap\"
";
        let compiled = compile_script(text, &dict).unwrap();
        let state = new_state(1430); // day 0, 23:50 — tick 143 was spent at 1430
        let facts = compute_world_facts(&compiled.content, &state, None, false);
        // the day's own tick 143 lies at now: the next crossing is a full
        // day out (the d == 0 → 1440 branch of the rem_euclid walk)
        assert_eq!(facts.first_cross(143), 1440);
        // tick 0 (00:00) lies 10 minutes ahead, at day 1's first tick
        assert_eq!(facts.first_cross(0), 10);
    }
}
