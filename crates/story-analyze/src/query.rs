//! The query model (story-analyze spec §3): goal predicates over states and
//! transition edges, reachability queries with objectives and RNG modes, and
//! their evaluation through the kernel's context-free evaluation face. The
//! module also extracts the static `clock` thresholds of a predicate — part
//! of the transition generator's critical-moment set (§5.4).

use std::collections::BTreeSet;

use story_content::ir::{Expr, QueryTerm, Scope};
use story_core::{CardId, EndingId, EntityId, RecipeId, SlotName, State, Time, VerbId};

/// Why an open was skipped — mirrored for the matcher (the engine's own enum
/// stays the source of truth in event payloads).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartSkipReasonMatcher {
    InstanceCap,
    MaterializationFailed,
    Any,
}

/// A transition-edge condition (story-analyze spec §3.1): matched against the
/// event stream of one transition record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventMatcher {
    SituationCompleted { recipe: RecipeId },
    Continued { recipe: RecipeId },
    Bound { slot: SlotName, card: Option<CardId> },
    StartSkipped { reason: Option<StartSkipReasonMatcher> },
    NarrationContains(String),
    ResolutionFailed,
}

/// A partial map over addressable facts; facts it omits are wildcards
/// (story-analyze spec §3.1): aspect values on named (unique) cards,
/// card-presence patterns, legacy values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatePattern {
    pub aspects: Vec<(CardId, String, i64)>,
    pub cards: Vec<(Scope, Option<CardId>, Vec<QueryTerm>)>,
    pub legacy: Vec<(String, i64)>,
}

/// A goal predicate (story-analyze spec §3.1). `Expr` and `QueryTerm`
/// predicates evaluate through the kernel face with T1 totality and T9
/// context-free scope semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoalPredicate {
    /// A compiled `.sw` boolean expression, e.g. `beacon.heat >= 6`.
    Expr(Expr),
    /// Live entities satisfying all terms, the scope (zone scopes include
    /// nested subtrees), a minimum stack count, and an optional minimum
    /// remaining lifetime before `expires_at`.
    CardMatch {
        scope: Scope,
        prototype: Option<CardId>,
        kind: Option<String>,
        terms: Vec<QueryTerm>,
        min_count: u32,
        min_ttl: Option<Time>,
    },
    /// The story has ended (the given ending, or any ending when `None`).
    Ending(Option<EndingId>),
    /// A live situation of the verb / recipe.
    SituationActive {
        verb: Option<VerbId>,
        recipe: Option<RecipeId>,
    },
    /// A transition-edge condition over one step's event stream.
    TransitionEvent(EventMatcher),
    /// Specified aspects / cards / legacies must match.
    PartialState(StatePattern),
    And(Vec<GoalPredicate>),
    Or(Vec<GoalPredicate>),
    Not(Box<GoalPredicate>),
}

/// The search objective (story-analyze spec §3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchObjective {
    /// Fewest commits + binds; tie-break earliest clock.
    FewestActions,
    /// Earliest goal clock; tie-break fewest actions.
    EarliestTime,
    /// Fewest transitions.
    ShortestDepth,
    /// Up to `max_ways` structurally distinct recipe sequences.
    DiverseWays { max_ways: usize },
}

/// The branching behavior of the search (story-analyze spec §3.2, §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RngMode {
    /// The kernel's deterministic path at every chance point.
    Deterministic,
    /// Enumerate chance outcomes with exact probabilities; witnesses record
    /// cumulative path probability.
    OptimisticBranching,
    /// The same enumeration; the AND-OR solver takes the positive-probability
    /// minimum over chance outcomes, and witnesses carry no probability.
    Adversarial,
}

impl RngMode {
    /// Whether chance points fork into enumerated outcomes.
    pub fn branches(self) -> bool {
        matches!(self, RngMode::OptimisticBranching | RngMode::Adversarial)
    }
}

/// A reachability query (story-analyze spec §3.2).
#[derive(Clone, Debug)]
pub struct ReachQuery {
    pub goal: GoalPredicate,
    /// Must hold on every state along the path.
    pub invariant: Option<GoalPredicate>,
    pub objective: SearchObjective,
    pub rng_mode: RngMode,
    /// Clock horizon.
    pub max_time: Option<Time>,
    /// Expansion bound.
    pub max_states: usize,
    /// `false` emits no Bind transitions (§5.1): queries over fixed mid-run
    /// binding timelines turn it off.
    pub allow_mid_run_bind: bool,
    /// Heuristic feasibility pruning (off by default): branches whose goal
    /// facts cannot be grounded even in the causal graph's delete relaxation
    /// are not expanded. The relaxation misses decrease-chain routes, so a
    /// pruned run's "no witness" is weaker than exhaustive unreachability —
    /// the caller must say so. Witnesses stay replay-verified.
    pub prune: bool,
}

impl ReachQuery {
    /// A query with the spec's default bounds: 100k expansions, no clock
    /// horizon, mid-run binding allowed.
    pub fn new(goal: GoalPredicate) -> ReachQuery {
        ReachQuery {
            goal,
            invariant: None,
            objective: SearchObjective::FewestActions,
            rng_mode: RngMode::Deterministic,
            max_time: None,
            max_states: 100_000,
            allow_mid_run_bind: true,
            prune: false,
        }
    }

    /// The engine-reported clock thresholds of the goal and the invariant
    /// (§5.4): part of the transition generator's critical-moment set.
    pub fn clock_thresholds(&self) -> BTreeSet<Time> {
        predicate_clock_thresholds(&self.goal).into_iter().chain(
            self.invariant
                .as_ref()
                .map(predicate_clock_thresholds)
                .unwrap_or_default(),
        )
        .collect()
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// The evaluation context: the engine installs the state under test (the
/// analyzer evaluates through `Engine::with_state`), and the events of the
/// transition edge under test back the `TransitionEvent` matcher.
pub struct GoalCtx<'a> {
    pub engine: &'a story_engine::Engine,
    pub events: &'a [story_engine::Event],
}

impl GoalPredicate {
    /// Whether the predicate holds at the context's state (and, for
    /// transition-edge conditions, the context's event stream).
    pub fn holds(&self, ctx: &GoalCtx<'_>) -> bool {
        match self {
            GoalPredicate::Expr(e) => ctx.engine.eval_cond(e),
            GoalPredicate::CardMatch {
                scope,
                prototype,
                kind,
                terms,
                min_count,
                min_ttl,
            } => {
                let now = ctx.engine.state().now;
                let matches = |e: &EntityId| {
                    let ent = &ctx.engine.state().entities[e];
                    if let Some(p) = prototype {
                        if &ent.prototype != p {
                            return false;
                        }
                    }
                    if let Some(k) = kind {
                        if ent.kind.as_deref() != Some(k.as_str()) {
                            return false;
                        }
                    }
                    if !ctx.engine.matches(*e, terms) {
                        return false;
                    }
                    if let Some(ttl) = min_ttl {
                        match ent.expires_at {
                            Some(at) if at.saturating_sub(now) >= *ttl => {}
                            _ => return false,
                        }
                    }
                    true
                };
                ctx.engine
                    .scope_entities(scope)
                    .iter()
                    .filter(|e| matches(e))
                    .count()
                    >= *min_count as usize
            }
            GoalPredicate::Ending(id) => match id {
                Some(want) => ctx.engine.state().ended.as_ref() == Some(want),
                None => ctx.engine.state().ended.is_some(),
            },
            GoalPredicate::SituationActive { verb, recipe } => ctx
                .engine
                .state()
                .situations
                .values()
                .any(|s| {
                    verb.as_ref().is_none_or(|v| &s.verb == v)
                        && recipe.as_ref().is_none_or(|r| &s.recipe == r)
                }),
            GoalPredicate::TransitionEvent(m) => match_events(m, ctx),
            GoalPredicate::PartialState(p) => holds_pattern(p, ctx),
            GoalPredicate::And(ps) => ps.iter().all(|p| p.holds(ctx)),
            GoalPredicate::Or(ps) => ps.iter().any(|p| p.holds(ctx)),
            GoalPredicate::Not(p) => !p.holds(ctx),
        }
    }
}

fn match_events(m: &EventMatcher, ctx: &GoalCtx<'_>) -> bool {
    use story_engine::{Event, StartSkipReason};
    ctx.events.iter().any(|ev| match (m, ev) {
        (
            EventMatcher::SituationCompleted { recipe },
            Event::SituationCompleted { recipe: r, .. },
        ) => r == recipe,
        (EventMatcher::Continued { recipe }, Event::Continued { recipe: r, .. }) => r == recipe,
        (EventMatcher::Bound { slot, card }, Event::Bound { slot: s, entity, .. }) => {
            s == slot
                && card.as_ref().is_none_or(|c| {
                    ctx.engine
                        .state()
                        .entities
                        .get(entity)
                        .is_some_and(|ent| &ent.prototype == c)
                })
        }
        (
            EventMatcher::StartSkipped { reason },
            Event::StartSkipped { reason: r, .. },
        ) => match reason {
            None => true,
            Some(StartSkipReasonMatcher::Any) => true,
            Some(StartSkipReasonMatcher::InstanceCap) => *r == StartSkipReason::InstanceCap,
            Some(StartSkipReasonMatcher::MaterializationFailed) => {
                *r == StartSkipReason::MaterializationFailed
            }
        },
        (EventMatcher::NarrationContains(needle), Event::Narration { text }) => {
            text.contains(needle.as_str())
        }
        (EventMatcher::ResolutionFailed, Event::ResolutionFailed { .. }) => true,
        _ => false,
    })
}

fn holds_pattern(p: &StatePattern, ctx: &GoalCtx<'_>) -> bool {
    let state: &State = ctx.engine.state();
    for (card, key, value) in &p.aspects {
        let Some(ent) = named_entity(ctx.engine, card) else {
            return false;
        };
        let actual = ent.aspects.get(key).copied().unwrap_or_else(|| {
            ctx.engine
                .content()
                .aspects
                .get(key)
                .map(|d| d.default)
                .unwrap_or(0)
        });
        if actual != *value {
            return false;
        }
    }
    for (scope, prototype, terms) in &p.cards {
        let ok = ctx.engine.scope_entities(scope).iter().any(|e| {
            let ent = &state.entities[e];
            prototype.as_ref().is_none_or(|p| &ent.prototype == p)
                && ctx.engine.matches(*e, terms)
        });
        if !ok {
            return false;
        }
    }
    p.legacy
        .iter()
        .all(|(k, v)| state.legacy.get(k) == Some(v))
}

/// The live entity of a unique card name: the first live entity whose name
/// matches (unique cards mint one copy; the analyzer reads names, not ids).
fn named_entity<'a>(
    engine: &'a story_engine::Engine,
    card: &CardId,
) -> Option<&'a story_core::Entity> {
    engine
        .state()
        .entities
        .values()
        .find(|e| e.name == card.as_str())
}

// ---------------------------------------------------------------------------
// Static clock thresholds (§5.4)
// ---------------------------------------------------------------------------

/// Every `clock` threshold a statically analyzable comparison in the
/// expression pins: `clock >= t` pins t, `clock > t` pins t+1, `clock == t`
/// pins t. Upper-bound comparisons gate negatively — time only moves
/// forward, so they never create a due moment.
pub fn clock_thresholds(e: &Expr, out: &mut BTreeSet<Time>) {
    match e {
        Expr::Cmp { op, left, right } => {
            if matches!(**left, Expr::Clock) {
                if let Expr::Lit(t) = right.as_ref() {
                    insert_threshold(*op, *t, out);
                }
            } else if matches!(**right, Expr::Clock) {
                if let Expr::Lit(t) = left.as_ref() {
                    // Mirror: `t <= clock` reads `clock >= t`.
                    insert_threshold(mirror(*op), *t, out);
                }
            }
        }
        Expr::Arith { left, right, .. } | Expr::Logic { left, right, .. } => {
            clock_thresholds(left, out);
            clock_thresholds(right, out);
        }
        Expr::Not(inner) => clock_thresholds(inner, out),
        _ => {}
    }
}

fn mirror(op: story_content::ir::CmpOp) -> story_content::ir::CmpOp {
    use story_content::ir::CmpOp::*;
    match op {
        Ge => Le,
        Le => Ge,
        Gt => Lt,
        Lt => Gt,
        Eq | Ne => Eq,
    }
}

fn insert_threshold(
    op: story_content::ir::CmpOp,
    t: Time,
    out: &mut BTreeSet<Time>,
) {
    use story_content::ir::CmpOp::*;
    match op {
        Ge => {
            out.insert(t);
        }
        Gt => {
            out.insert(t.saturating_add(1));
        }
        Eq => {
            out.insert(t);
        }
        Le | Lt | Ne => {}
    }
}

/// The static clock thresholds of one goal predicate (§5.4).
pub fn predicate_clock_thresholds(p: &GoalPredicate) -> BTreeSet<Time> {
    let mut out = BTreeSet::new();
    collect_pred_thresholds(p, &mut out);
    out
}

/// The in-place form of [`predicate_clock_thresholds`].
pub fn collect_pred_thresholds_pub(p: &GoalPredicate, out: &mut BTreeSet<Time>) {
    collect_pred_thresholds(p, out)
}

fn collect_pred_thresholds(p: &GoalPredicate, out: &mut BTreeSet<Time>) {
    match p {
        GoalPredicate::Expr(e) => clock_thresholds(e, out),
        GoalPredicate::And(ps) | GoalPredicate::Or(ps) => {
            for p in ps {
                collect_pred_thresholds(p, out);
            }
        }
        GoalPredicate::Not(p) => collect_pred_thresholds(p, out),
        _ => {}
    }
}
