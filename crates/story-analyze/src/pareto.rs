//! The `ParetoAndCutAnalyzer` (story-analyze spec §7.4): trade-off
//! envelopes over objective vectors, mandatory sets, and counterfactual
//! mechanism cuts verified by re-exploration on neutralized content.

use std::collections::{BTreeSet};
use std::sync::Arc;

use story_content::ir::Expr;
use story_core::{CardId, RecipeId};

use crate::query::{GoalCtx, GoalPredicate};
use crate::search::{Analyzer, ExploreConfig};
use crate::transitions::{PlayerStep, WitnessPath};

/// The direction of one objective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Maximize,
    Minimize,
}

/// One objective expression with its direction (e.g. maximize
/// `count(anywhere, name:token)`, minimize actions).
#[derive(Clone, Debug)]
pub struct Objective {
    pub expr: Expr,
    pub direction: Direction,
}

/// One Pareto-undominated point, backed by its witness.
#[derive(Clone, Debug)]
pub struct ParetoPoint {
    pub values: Vec<f64>,
    pub witness: WitnessPath,
}

/// The analysis result (§7.4).
#[derive(Clone, Debug, Default)]
pub struct ParetoReport {
    /// The Pareto-undominated set at the goal, each point backed by a
    /// replay-verified witness.
    pub frontier: Vec<ParetoPoint>,
    /// The intersection over the frontier witnesses: every strategy needs
    /// them.
    pub mandatory_recipes: BTreeSet<RecipeId>,
    /// The intersection over the frontier witnesses of the card prototypes
    /// the strategy binds (commit or mid-run bind).
    pub mandatory_cards: BTreeSet<CardId>,
    /// Minimal hitting sets over the frontier witnesses' recipe sets.
    pub minimal_recipe_cuts: Vec<BTreeSet<RecipeId>>,
    /// The cuts whose counterfactual re-exploration confirmed no
    /// alternative path exists within the bounds.
    pub verified_cuts: Vec<BTreeSet<RecipeId>>,
    /// True when bounds or budgets truncated any part of the analysis.
    pub truncated: bool,
}

/// The analyzer configuration.
#[derive(Clone)]
pub struct ParetoConfig {
    pub goal: GoalPredicate,
    pub objectives: Vec<Objective>,
    pub explore: ExploreConfig,
    /// The hitting-set candidate budget; beyond it the cut analysis is
    /// skipped (the frontier stands, `truncated` set).
    pub max_cut_candidates: usize,
}

impl ParetoConfig {
    pub fn new(goal: GoalPredicate, objectives: Vec<Objective>) -> ParetoConfig {
        ParetoConfig {
            goal,
            objectives,
            explore: ExploreConfig::default(),
            max_cut_candidates: 12,
        }
    }
}

/// Run the analysis from the analyzer's start state.
pub fn analyze(analyzer: &Analyzer, config: &ParetoConfig) -> ParetoReport {
    let mut report = ParetoReport::default();

    // The explored space with the goal states as the frontier's leaves.
    let explore = ExploreConfig {
        stop_at: Some(config.goal.clone()),
        ..config.explore.clone()
    };
    let mut explorer = fresh(analyzer);
    let graph = explorer.explore(&explore);
    report.truncated |= graph.truncated;

    // Objective vectors at the goal nodes.
    let mut candidates: Vec<(usize, Vec<f64>)> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        if let Some(values) = objectives_at(analyzer, &node.state, config) {
            candidates.push((i, values));
        }
    }

    // Pareto-undominated filter (maximize/minimize per objective).
    let component = |x: f64, y: f64, d: Direction| match d {
        Direction::Maximize => x >= y,
        Direction::Minimize => x <= y,
    };
    let strict = |x: f64, y: f64, d: Direction| match d {
        Direction::Maximize => x > y,
        Direction::Minimize => x < y,
    };
    let dominates = |a: &[f64], b: &[f64]| -> bool {
        a.iter()
            .zip(b)
            .zip(&config.objectives)
            .all(|((&x, &y), o)| component(x, y, o.direction))
            && a.iter()
                .zip(b)
                .zip(&config.objectives)
                .any(|((&x, &y), o)| strict(x, y, o.direction))
    };
    // One point per distinct vector: equal vectors are the same trade-off.
    let mut kept: Vec<(usize, Vec<f64>)> = Vec::new();
    for (i, v) in &candidates {
        if candidates.iter().any(|(_, w)| dominates(w, v)) {
            continue;
        }
        if kept.iter().any(|(_, w)| w == v) {
            continue;
        }
        kept.push((*i, v.clone()));
    }

    // Witnesses: the BFS-tree path to each frontier node, replay-verified.
    let replayer = analyzer.replayer();
    for (i, values) in kept {
        // witness_to carries the graph's mode-appropriate cumulative
        // probability (None under `Adversarial`, §6).
        let Some(witness) = graph.witness_to(i, analyzer.start()) else {
            continue;
        };
        match replayer.replay(&witness, Some(&config.goal), config.explore.invariant.as_ref()) {
            Ok(_) => report.frontier.push(ParetoPoint { values, witness }),
            Err(_) => report.truncated = true,
        }
    }

    // The mandatory sets: intersections over the frontier witnesses.
    let sets: Vec<BTreeSet<RecipeId>> = report
        .frontier
        .iter()
        .map(|p| recipes_of(&p.witness))
        .collect();
    let card_sets: Vec<BTreeSet<CardId>> = report
        .frontier
        .iter()
        .map(|p| cards_of(&p.witness))
        .collect();
    report.mandatory_recipes = intersect_all(&sets);
    report.mandatory_cards = intersect_all(&card_sets);

    // Minimal hitting sets over the witness recipe sets, each verified by
    // counterfactual re-exploration on a neutralized clone of the content.
    let universe: BTreeSet<RecipeId> = sets.iter().flatten().cloned().collect();
    if sets.is_empty() {
        return report;
    }
    if universe.len() > config.max_cut_candidates {
        report.truncated = true;
        return report;
    }
    report.minimal_recipe_cuts = hitting_sets(&sets, &universe);
    for cut in &report.minimal_recipe_cuts {
        match verify_cut(analyzer, config, cut) {
            Some(true) => report.verified_cuts.push(cut.clone()),
            _ => report.truncated = true,
        }
    }
    report
}

/// A fresh analyzer over the same content, seed, and start state.
fn fresh(analyzer: &Analyzer) -> Analyzer {
    Analyzer::boot(
        analyzer.content().clone(),
        analyzer.registry().clone(),
        analyzer.seed(),
        analyzer.legacy_in(),
    )
    .0
    .with_start_state(analyzer.start().clone())
}

/// The objective vector at a state, or `None` when the goal does not hold.
fn objectives_at(analyzer: &Analyzer, state: &story_core::State, config: &ParetoConfig) -> Option<Vec<f64>> {
    let mut engine = analyzer.engine_with_start();
    *engine.state_mut() = state.clone();
    let ctx = GoalCtx {
        engine: &engine,
        events: &[],
    };
    if !config.goal.holds(&ctx) {
        return None;
    }
    Some(
        config
            .objectives
            .iter()
            .map(|o| engine.eval_expr(&o.expr) as f64)
            .collect(),
    )
}

fn recipes_of(witness: &WitnessPath) -> BTreeSet<RecipeId> {
    witness
        .records
        .iter()
        .filter_map(|r| r.recipe.clone())
        .collect()
}

/// The card prototypes the strategy binds (commit bindings and mid-run
/// binds), read off the witness steps against the states they run from.
fn cards_of(witness: &WitnessPath) -> BTreeSet<CardId> {
    let mut out = BTreeSet::new();
    let mut state = witness.start.clone();
    for record in &witness.records {
        match &record.step {
            PlayerStep::Commit { bindings, .. } => {
                for (_, e) in bindings {
                    if let Some(ent) = state.entities.get(e) {
                        out.insert(ent.prototype.clone());
                    }
                }
            }
            PlayerStep::Bind { entity, .. } => {
                if let Some(ent) = state.entities.get(entity) {
                    out.insert(ent.prototype.clone());
                }
            }
            PlayerStep::Advance { .. } => {}
        }
        state = record.state_after.clone();
    }
    out
}

fn intersect_all<T: Ord + Clone>(sets: &[BTreeSet<T>]) -> BTreeSet<T> {
    let mut iter = sets.iter();
    let Some(first) = iter.next() else {
        return BTreeSet::new();
    };
    let mut out = first.clone();
    for s in iter {
        out = out.intersection(s).cloned().collect();
    }
    out
}

/// Exact minimal hitting sets (small universes only): subsets of the
/// universe, in increasing size, hitting every witness set; supersets of
/// found sets are discarded.
fn hitting_sets(
    sets: &[BTreeSet<RecipeId>],
    universe: &BTreeSet<RecipeId>,
) -> Vec<BTreeSet<RecipeId>> {
    let items: Vec<RecipeId> = universe.iter().cloned().collect();
    let n = items.len();
    let mut out: Vec<BTreeSet<RecipeId>> = Vec::new();
    for size in 0..=n {
        for mask in 0u64..(1u64 << n) {
            if mask.count_ones() as usize != size {
                continue;
            }
            let candidate: BTreeSet<RecipeId> = items
                .iter()
                .enumerate()
                .filter(|(i, _)| (mask >> i) & 1 == 1)
                .map(|(_, r)| r.clone())
                .collect();
            if !sets.iter().all(|s| s.iter().any(|r| candidate.contains(r))) {
                continue;
            }
            if out.iter().any(|m| candidate.is_superset(m)) {
                continue;
            }
            out.push(candidate);
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Neutralize the cut (`when = false` on every cut recipe, its `max`
/// counter pre-spent in the start state) on a cloned `Content` and
/// re-explore: `Some(true)` when the goal became unreachable within the
/// bounds, `Some(false)` when an alternative path still exists, `None` when
/// the re-exploration was truncated (counterexample-guided refinement, §7.4).
fn verify_cut(
    analyzer: &Analyzer,
    config: &ParetoConfig,
    cut: &BTreeSet<RecipeId>,
) -> Option<bool> {
    let mut content = analyzer.content().as_ref().clone();
    for verb in content.verbs.values_mut() {
        for rec in verb.recipes.values_mut() {
            if cut.contains(&rec.id) {
                rec.when = Some(Expr::Bool(false));
            }
        }
    }
    let mut start = analyzer.start().clone();
    for r in cut {
        let max = content
            .verbs
            .values()
            .flat_map(|v| v.recipes.values())
            .find(|rec| &rec.id == r)
            .and_then(|rec| rec.max);
        // Pre-spent: `max_left` reads `runs < max`.
        start.recipe_runs.insert(r.clone(), max.unwrap_or(0));
    }
    let mut neutral = Analyzer::boot(
        Arc::new(content),
        // The analyzer's own registry: host-op content neutralizes and
        // re-explores faithfully — a default registry would fail every
        // host-op path at runtime and falsely verify cuts.
        analyzer.registry().clone(),
        analyzer.seed(),
        analyzer.legacy_in(),
    )
    .0
    .with_start_state(start);
    let explore = ExploreConfig {
        stop_at: Some(config.goal.clone()),
        ..config.explore.clone()
    };
    let graph = neutral.explore(&explore);
    if graph.truncated {
        return None;
    }
    let engine = neutral.engine_with_start();
    let unreachable = !graph.nodes.iter().any(|node| {
        let mut e = engine.clone();
        *e.state_mut() = node.state.clone();
        let ctx = GoalCtx {
            engine: &e,
            events: &[],
        };
        config.goal.holds(&ctx)
    });
    Some(unreachable)
}
