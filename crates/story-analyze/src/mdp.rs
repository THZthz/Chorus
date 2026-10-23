//! The `StochasticMdpSolver` (story-analyze spec §7.2): optimal play,
//! guarantees, and expected value by backward induction over the
//! (now, actions) DAG. Decision nodes are the player's `Commit`/`Bind`/
//! `Advance` choices; chance nodes are the §6 forks fused into the
//! enclosing action's settlement. Every edge strictly increases
//! `(now, actions)` lexicographically, so the graph is a DAG under that
//! labeling (canonical-key revisits at higher action counts are not cycles)
//! and backward induction is exact — no value-iteration convergence.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use story_content::Content;
use story_core::{State, Time};

use crate::canonical::CanonicalStateKey;
use crate::Analyzer;
use crate::query::{GoalCtx, GoalPredicate, RngMode};
use crate::transitions::{GeneratorConfig, TransitionGenerator};

/// The solver configuration: the exploration budgets of the game-graph
/// construction (§1.2 — analyzer certainty is bounded by its bounds).
#[derive(Clone)]
pub struct MdpConfig {
    pub goal: GoalPredicate,
    pub invariant: Option<GoalPredicate>,
    pub rng_mode: RngMode,
    pub max_time: Option<Time>,
    pub max_actions: usize,
    pub max_nodes: usize,
    /// The total graph-depth budget (commits plus advances along one
    /// branch). It bounds the induction recursion as well — advances raise
    /// `now` without raising `actions`, so the (now, actions) labeling
    /// alone does not bound the graph without a horizon.
    pub max_depth: usize,
    pub allow_mid_run_bind: bool,
    /// Heuristic feasibility pruning (off by default): states whose goal
    /// facts cannot be grounded even in the causal graph's delete relaxation
    /// score 0 — the same conservative direction as a budget cut, and the
    /// outcome reports incomplete. Skipped for host-opaque content.
    pub prune: bool,
}

impl MdpConfig {
    pub fn new(goal: GoalPredicate) -> MdpConfig {
        MdpConfig {
            goal,
            invariant: None,
            rng_mode: RngMode::OptimisticBranching,
            max_time: None,
            max_actions: 24,
            max_nodes: 20_000,
            max_depth: 192,
            allow_mid_run_bind: true,
            prune: false,
        }
    }
}

/// The three metrics of one state (§7.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MdpValues {
    /// Maximum goal probability under optimal play (expectimax).
    pub v_max: f64,
    /// Worst-case guarantee: `max_a min_{P>0} V(s')`. `1.0` means a 100%
    /// RNG-proof strategy exists.
    pub v_minimax: f64,
    /// Uniform/random-play baseline (the forgiveness measure).
    pub v_unif: f64,
    /// Expected action count under the optimal (V-max) policy.
    pub expected_actions: f64,
}

/// The solver result at the start state, plus its bounds flags.
#[derive(Clone, Debug)]
pub struct MdpOutcome {
    pub start: MdpValues,
    pub nodes: usize,
    /// False when the node budget or the action/horizon budget truncated
    /// the game graph: the values are then bounds-conditional (truncated
    /// leaves score 0, the conservative direction for win probability).
    pub complete: bool,
}

#[derive(Clone, Default)]
struct Vals {
    max: f64,
    minimax: f64,
    unif: f64,
    actions: f64,
}

/// Solve the game from the analyzer's start state.
pub fn solve(analyzer: &Analyzer, config: &MdpConfig) -> MdpOutcome {
    let _span = tracing::info_span!("mdp_solve").entered();
    let content: Arc<Content> = analyzer.content().clone();
    let gen_config = GeneratorConfig {
        seed: analyzer.seed(),
        legacy_in: analyzer.legacy_in(),
        rng_mode: config.rng_mode,
        allow_mid_run_bind: config.allow_mid_run_bind,
        max_time: config.max_time,
        invariant: config.invariant.clone(),
        query_thresholds: {
            // The goal's and invariant's clock thresholds join Tcrit so no
            // jump lands past a moment the goal or invariant can turn on.
            let mut t = BTreeSet::new();
            crate::query::collect_pred_thresholds_pub(&config.goal, &mut t);
            if let Some(inv) = &config.invariant {
                crate::query::collect_pred_thresholds_pub(inv, &mut t);
            }
            t
        },
    };
    let mut gen =
        TransitionGenerator::new(content.clone(), analyzer.engine_with_start(), gen_config);
    let mut memo: BTreeMap<(CanonicalStateKey, usize), Option<Vals>> = BTreeMap::new();
    let start = analyzer.start().clone();
    let mut counter = NodeCounter {
        count: 0,
        limit: config.max_nodes,
    };
    let vals = {
        let goal_facts = crate::causal::goal_facts(&config.goal);
        let scarcity = if config.prune && !analyzer.host_opaque() {
            let mintable = analyzer.causal().recipe_mintable_cards();
            crate::scarcity::Scarcity::build(&config.goal, &content, &mintable)
        } else {
            crate::scarcity::Scarcity::empty()
        };
        let mut ctx = Ctx {
            gen: &mut gen,
            content: &content,
            symmetry: analyzer.symmetry(),
            causal: analyzer.causal(),
            goal_facts,
            scarcity,
            config,
            memo: &mut memo,
            counter: &mut counter,
        };
        eval(&mut ctx, &start, 0, 0)
    };
    // `count == limit` means some subtree was cut by the budget.
    let complete = vals.is_some() && counter.count < counter.limit;
    MdpOutcome {
        start: vals.map_or_else(MdpValues::default, MdpValues::from),
        nodes: counter.count,
        complete,
    }
}

struct NodeCounter {
    count: usize,
    limit: usize,
}

/// The recursion context of one induction, bundling the eval parameters.
struct Ctx<'a> {
    gen: &'a mut TransitionGenerator,
    content: &'a Arc<Content>,
    symmetry: &'a crate::symmetry::SymmetryCache,
    causal: &'a crate::causal::CausalGraph,
    /// The goal's relaxed-grounding facts, computed once for the prune gate.
    goal_facts: std::collections::BTreeSet<crate::causal::Fact>,
    scarcity: crate::scarcity::Scarcity,
    config: &'a MdpConfig,
    memo: &'a mut BTreeMap<(CanonicalStateKey, usize), Option<Vals>>,
    counter: &'a mut NodeCounter,
}

/// Backward induction by memoized recursion over `(canonical key, actions)`.
/// Every edge strictly increases `(now, actions)`, so the recursion is
/// well-founded: commits and binds raise `actions`, advances raise `now`,
/// and no `(key, actions)` pair recurses into itself. `None` marks a
/// truncated subtree (budget exhausted): it scores 0, the conservative
/// direction, and marks the outcome incomplete.
fn eval(ctx: &mut Ctx<'_>, state: &State, actions: usize, depth: usize) -> Option<Vals> {
    if ctx.counter.count >= ctx.counter.limit
        || actions > ctx.config.max_actions
        || depth > ctx.config.max_depth
    {
        return None;
    }
    if ctx.config.prune && !ctx.causal.host_opaque() {
        ctx.gen.install_state(state);
        let raw = ctx.causal.h_add(state, ctx.gen.engine(), &ctx.goal_facts);
        if raw >= u32::MAX / 4 {
            // A pruned state scores 0, the same conservative direction as a
            // budget cut, and marks the outcome incomplete.
            ctx.counter.count += 1;
            return Some(Vals::default());
        }
        if !ctx.scarcity.is_empty() && ctx.scarcity.dead(state, ctx.content) {
            ctx.counter.count += 1;
            return Some(Vals::default());
        }
    }
    let key = CanonicalStateKey::of_with(state, ctx.content, ctx.symmetry);
    let memo_key = (key, actions);
    if let Some(cached) = ctx.memo.get(&memo_key) {
        return cached.clone();
    }
    // Reserve before descending: a re-entry at the same pair cannot happen
    // (labels strictly increase), so no placeholder cycle handling exists.
    ctx.counter.count += 1;

    let goal_here = eval_goal(ctx.gen, ctx.config, state);
    let vals = if goal_here {
        Some(Vals {
            max: 1.0,
            minimax: 1.0,
            unif: 1.0,
            actions: 0.0,
        })
    } else if state.ended.is_some() {
        Some(Vals::default())
    } else {
        // The three value kinds are tracked independently through the
        // induction (§7.2): `v_max` maximizes the expectation over actions,
        // `v_minimax` maximizes each action's positive-probability minimum,
        // and `v_unif` averages every legal action's expectation (the
        // uniform-play baseline). Only `v_max` (with the action-count
        // tie-break) selects the optimal policy.
        let mut best: Option<Vals> = None;
        let mut best_minimax = 0.0f64;
        let mut unif_sum = 0.0f64;
        let mut unif_actions = 0usize;
        let steps = ctx.gen.steps(state);
        if steps.is_empty() {
            // A live state with no legal step: the run is stuck here
            // (bounds-conditional when the generator was budget-capped).
            best = Some(Vals::default());
        }
        for step in steps {
            let records = ctx.gen.execute(state, &step);
            if records.is_empty() {
                continue; // refused actions create no edge (T3)
            }
            // The chance node fused into this action's settlement: exact
            // class probabilities in both branching modes (§6), so the
            // positive-probability minimum of V_minimax is well defined.
            let mut expected = 0.0;
            let mut worst = f64::INFINITY;
            let mut unif_expected = 0.0;
            let mut act_expected = 0.0;
            let mut total_p = 0.0;
            for record in &records {
                let branch = eval(ctx, &record.state_after, actions + record.actions(), depth + 1);
                let p = record.probability;
                total_p += p;
                let v = branch.as_ref().map_or(0.0, |b| b.max);
                expected += p * v;
                if p > 0.0 {
                    // The minimax minimum runs over branches with positive
                    // probability only (§7.2).
                    worst = worst.min(v);
                }
                let u = branch.as_ref().map_or(0.0, |b| b.unif);
                unif_expected += p * u;
                let a = branch.as_ref().map_or(0.0, |b| b.actions);
                act_expected += p * a;
            }
            if total_p <= 0.0 {
                continue;
            }
            expected /= total_p;
            unif_expected /= total_p;
            act_expected /= total_p;
            best_minimax = best_minimax.max(worst.clamp(0.0, 1.0));
            unif_sum += unif_expected;
            unif_actions += 1;
            let cand = Vals {
                max: expected,
                minimax: 0.0,
                unif: 0.0,
                actions: 1.0 + act_expected,
            };
            best = Some(match best {
                None => cand,
                Some(b) if cand.max > b.max
                    || (cand.max == b.max && cand.actions < b.actions) =>
                {
                    cand
                }
                Some(b) => b,
            });
            if ctx.counter.count >= ctx.counter.limit {
                break;
            }
        }
        // The node's own V_minimax and V_unif: the maximum guarantee over
        // actions, and the uniform average over the enumerated actions.
        if let Some(b) = best.as_mut() {
            b.minimax = best_minimax;
            if unif_actions > 0 {
                b.unif = unif_sum / unif_actions as f64;
            }
        }
        best
    };
    ctx.memo.insert(memo_key, vals.clone());
    vals
}

/// The generator's scratch state, for goal evaluation.
fn eval_goal(gen: &mut TransitionGenerator, config: &MdpConfig, state: &State) -> bool {
    gen.install_state(state);
    let ctx = GoalCtx {
        engine: gen.engine(),
        events: &[],
    };
    config.goal.holds(&ctx)
}

impl From<Vals> for MdpValues {
    fn from(v: Vals) -> MdpValues {
        MdpValues {
            v_max: v.max,
            v_minimax: v.minimax,
            v_unif: v.unif,
            expected_actions: v.actions,
        }
    }
}
