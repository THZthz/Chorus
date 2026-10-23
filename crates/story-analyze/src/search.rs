//! Tier 2 search (story-analyze spec §5): the `ReachabilityEngine`. A
//! best-first exploration over real kernel transitions with canonical-key
//! dedup (§5.3), the objective's dominance rules (§5.5), the Tier-1
//! `h_add` heuristic ordering the queue (never pruning), and replay-verified
//! trace coalescing (§5.4). The same machinery, with canonical dedup alone,
//! builds the full explored graph the classification and audit engines
//! consume (§7.1).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use story_content::Content;
use story_core::{RecipeId, State, Time};
use story_content::runtime::RuntimeDiagnostic;
use story_engine::{ChanceOutcome, ChancePoint, EffectRegistry, Engine, };

use crate::causal::{self, CausalGraph, MatchOracle};
use crate::canonical::CanonicalStateKey;
use crate::query::{GoalCtx, GoalPredicate, ReachQuery, RngMode, SearchObjective};
use crate::replay::{Replayer, ReplayError};
use crate::transitions::{
    GeneratorConfig, PlayerStep, TransitionGenerator, TransitionRecord, WitnessPath,
};

// ---------------------------------------------------------------------------
// The analyzer
// ---------------------------------------------------------------------------

/// The two-tier analysis entry point: Tier 1 (`CausalGraph`) for provenance,
/// heuristics, and dead-mechanism pruning; Tier 2 (the transition generator
/// plus the search) for everything state-dependent. One booted engine backs
/// both tiers; the search swaps states in through `Engine::state_mut`.
pub struct Analyzer {
    content: Arc<Content>,
    registry: EffectRegistry,
    seed: u64,
    legacy_in: Option<BTreeMap<String, i64>>,
    causal: CausalGraph,
    /// The content-wide mutation oracle, built once at boot: canonical keys
    /// and the symmetry classifier read it per state, and rebuilding it per
    /// state would walk every effect in the content each time.
    oracle: MatchOracle,
    /// The scratch engine: the search's evaluation face and the generator's
    /// template. Its state is the state under evaluation.
    engine: Engine,
    start: State,
    /// Whether the Tier-1 `h_add` heuristic orders the queue (§5.5): on by
    /// default (guided best-first — fast, not provably optimal); off makes
    /// the search Dijkstra over the objective's cost vector, and the first
    /// witness optimal.
    heuristic: bool,
}

impl Analyzer {
    /// Boot the analyzer from compiled content (the same boot the game host
    /// runs; warnings pass through).
    pub fn boot(
        content: Arc<Content>,
        registry: EffectRegistry,
        seed: u64,
        legacy_in: Option<BTreeMap<String, i64>>,
    ) -> (Analyzer, Vec<RuntimeDiagnostic>) {
        let (engine, warnings) =
            Engine::boot(content.clone(), registry.clone(), seed, legacy_in.clone());
        let causal = CausalGraph::build(&content);
        let oracle = MatchOracle::new(&content);
        let start = engine.state().clone();
        (
            Analyzer {
                content,
                registry,
                seed,
                legacy_in,
                causal,
                oracle,
                engine,
                start,
                heuristic: true,
            },
            warnings,
        )
    }

    /// Analyze from a different start state (e.g. a depleted late-game
    /// state for point-of-no-return classification, §10).
    pub fn with_start_state(mut self, state: State) -> Analyzer {
        self.start = state;
        self
    }

    /// Turn the queue-ordering heuristic off (Dijkstra over the objective's
    /// cost vector; the first witness is then optimal, §5.5).
    pub fn without_heuristic(mut self) -> Analyzer {
        self.heuristic = false;
        self
    }

    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    /// The content-wide mutation oracle, held for the canonical-key and
    /// symmetry fast paths.
    pub(crate) fn oracle(&self) -> &MatchOracle {
        &self.oracle
    }

    pub fn causal(&self) -> &CausalGraph {
        &self.causal
    }

    pub fn start(&self) -> &State {
        &self.start
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn registry(&self) -> &EffectRegistry {
        &self.registry
    }

    pub fn legacy_in(&self) -> Option<BTreeMap<String, i64>> {
        self.legacy_in.clone()
    }

    pub fn replayer(&self) -> Replayer {
        Replayer::new(
            self.content.clone(),
            self.registry.clone(),
            self.seed,
            self.legacy_in.clone(),
        )
    }

    /// A template clone of the analyzer's engine with the start state
    /// installed (the generator engines clone from it).
    pub fn engine_with_start(&self) -> Engine {
        let mut e = self.engine.clone();
        *e.state_mut() = self.start.clone();
        e
    }

    /// Install a state in the scratch engine for evaluation.
    fn scratch(&mut self, state: &State) -> GoalCtx<'_> {
        *self.engine.state_mut() = state.clone();
        GoalCtx {
            engine: &self.engine,
            events: &[],
        }
    }

    fn generator(&self, query: &ReachQuery) -> TransitionGenerator {
        let config = GeneratorConfig {
            seed: self.seed,
            legacy_in: self.legacy_in.clone(),
            rng_mode: query.rng_mode,
            allow_mid_run_bind: query.allow_mid_run_bind,
            max_time: query.max_time,
            invariant: query.invariant.clone(),
            query_thresholds: query.clock_thresholds(),
        };
        TransitionGenerator::new(self.content.clone(), self.engine.clone(), config)
    }

    /// Solve one reach query (§5.5): best-first search over the transition
    /// space with the objective's dominance rules; every returned witness
    /// has passed the replay oracle (§5.6). Existential under every RNG
    /// mode — guarantee queries go through the AND-OR solver (`mdp`).
    pub fn solve(&mut self, query: &ReachQuery) -> SolveOutcome {
        let _span = tracing::info_span!("solve").entered();
        let mut gen = self.generator(query);
        let goal_facts = causal::goal_facts(&query.goal);
        let cone = self.causal.backward_cone(&goal_facts);
        let relevant = self.causal.relevant_verbs(&self.content, &cone);

        // The start node; the invariant must hold there too.
        if let Some(inv) = &query.invariant {
            let start = self.start.clone();
            let ctx = self.scratch(&start);
            if !inv.holds(&ctx) {
                return SolveOutcome::empty();
            }
        }
        let mut arena: Vec<ArenaNode> = vec![ArenaNode {
            state: self.start.clone(),
            parent: None,
            record: None,
            actions: 0,
            depth: 0,
            sig: Vec::new(),
        }];

        let diverse = matches!(query.objective, SearchObjective::DiverseWays { .. });
        let max_ways = match &query.objective {
            SearchObjective::DiverseWays { max_ways } => *max_ways,
            _ => 1,
        };

        let mut visited: BTreeMap<CanonicalStateKey, Vec<i64>> = BTreeMap::new();
        let mut sig_visited: BTreeSet<(CanonicalStateKey, Vec<Option<RecipeId>>)> = BTreeSet::new();
        let mut queue: std::collections::BinaryHeap<Reverse<Key>> =
            std::collections::BinaryHeap::new();
        queue.push(Reverse(Key {
            parts: vec![0, 0, 0],
            seq: 0,
        }));
        let mut expansions = 0usize;
        let mut truncated = false;
        let mut witnesses: Vec<WitnessPath> = Vec::new();
        let mut witnessed_sigs: BTreeSet<Vec<Option<RecipeId>>> = BTreeSet::new();
        let mut dropped = 0usize;
        let replayer = self.replayer();

        while let Some(Reverse(k)) = queue.pop() {
            let idx = k.seq;
            expansions += 1;
            if expansions > query.max_states {
                truncated = true;
                break;
            }
            let node = &arena[idx];
            let node_state = node.state.clone();
            // Goal on pop: with the heuristic off this is the optimal
            // witness for the objective's cost vector (§5.5).
            {
                *self.engine.state_mut() = node_state.clone();
                let events = node.record.as_ref().map(|r| r.events.clone()).unwrap_or_default();
                let ctx = GoalCtx {
                    engine: &self.engine,
                    events: &events,
                };
                if query.goal.holds(&ctx) {
                    // DiverseWays counts structurally distinct recipe
                    // sequences: one witness per signature (§5.5).
                    if diverse && !witnessed_sigs.insert(arena[idx].sig.clone()) {
                        continue;
                    }
                    let records = path(&arena, idx);
                    // Adversarial witnesses carry no cumulative probability
                    // (§6); the other modes record the exact product.
                    let probability = if matches!(query.rng_mode, RngMode::Adversarial) {
                        None
                    } else {
                        Some(records.iter().map(|r| r.probability).product::<f64>())
                    };
                    let witness = self.coalesce(WitnessPath {
                        start: self.start.clone(),
                        records,
                        actions: node.actions,
                        depth: node.depth,
                        probability,
                    });
                    match replayer.replay(&witness, Some(&query.goal), query.invariant.as_ref()) {
                        Ok(_) => {
                            witnesses.push(witness);
                            if witnesses.len() >= max_ways {
                                break;
                            }
                        }
                        Err(ReplayError::GoalUnmet) => {}
                        // A replay failure is an analyzer bug; the witness
                        // is dropped and counted (§5.6: every reported
                        // witness goes through the oracle).
                        Err(_) => dropped += 1,
                    }
                    continue;
                }
            }
            if node_state.ended.is_some() {
                continue; // terminal states have no outgoing transitions (T7)
            }
            for step in gen.steps(&node_state) {
                // Dead-mechanism deprioritization (§4 output 3): steps of
                // irrelevant verbs generate after relevant ones; insertion
                // order is the priority queue's final tie-break, so
                // deprioritization never prunes.
                let step_is_core = match &step {
                    PlayerStep::Commit { verb, .. } => relevant.contains(verb),
                    _ => true,
                };
                let order = if step_is_core { 0 } else { 1 };
                let records = gen.execute(&node_state, &step);
                for record in records {
                    let state_after = record.state_after.clone();
                    if let Some(horizon) = query.max_time {
                        if state_after.now > horizon {
                            continue; // advance steps beyond the horizon are discarded (§5.5)
                        }
                    }
                    // The invariant gates every successor (§3.2).
                    if let Some(inv) = &query.invariant {
                        *self.engine.state_mut() = state_after.clone();
                        let ctx = GoalCtx {
                            engine: &self.engine,
                            events: &record.events,
                        };
                        if !inv.holds(&ctx) {
                            continue;
                        }
                    }
                    let key = CanonicalStateKey::of_with(&state_after, &self.content, &self.oracle);
                    let now = state_after.now;
                    let actions = arena[idx].actions + record.actions();
                    let depth = arena[idx].depth + record.depth();
                    // Visited rules (§5.5): the objective's cost vector is
                    // the dominance test; DiverseWays augments the key with
                    // the recipe-trace signature instead.
                    if diverse {
                        let mut sig = arena[idx].sig.clone();
                        if let Some(r) = &record.recipe {
                            sig.push(Some(r.clone()));
                        }
                        if !sig_visited.insert((key.clone(), sig.clone())) {
                            continue;
                        }
                        let child = arena.len();
                        arena.push(ArenaNode {
                            state: state_after,
                            parent: Some(idx),
                            record: Some(record),
                            actions,
                            depth,
                            sig,
                        });
                        queue.push(Reverse(Key {
                            parts: priority(&query.objective, actions, depth, now, 0),
                            seq: child,
                        }));
                        continue;
                    }
                    let cost = match &query.objective {
                        SearchObjective::FewestActions => {
                            vec![actions as i64, state_after.now]
                        }
                        SearchObjective::EarliestTime => {
                            vec![state_after.now, actions as i64]
                        }
                        SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
                            vec![depth as i64]
                        }
                    };
                    let dominated = visited.get(&key).is_some_and(|best| {
                        best.iter().zip(&cost).all(|(b, c)| b <= c)
                    });
                    if dominated {
                        continue;
                    }
                    let best = visited.entry(key.clone()).or_default();
                    for (b, c) in best.iter_mut().zip(&cost) {
                        *b = (*b).min(*c);
                    }
                    let h = if self.heuristic {
                        self.h_of(&state_after, &goal_facts)
                    } else {
                        0
                    };
                    let child = arena.len();
                    arena.push(ArenaNode {
                        state: state_after.clone(),
                        parent: Some(idx),
                        record: Some(record),
                        actions,
                        depth,
                        sig: Vec::new(),
                    });
                    queue.push(Reverse(Key {
                        parts: priority(
                            &query.objective,
                            actions,
                            depth,
                            state_after.now,
                            h + order,
                        ),
                        seq: child,
                    }));
                }
            }
        }

        SolveOutcome {
            witnesses,
            truncated,
            expansions,
            witnesses_dropped_by_the_oracle: dropped,
        }
    }

    /// The `h_add` value of the goal facts at a state (§4 output 4); a
    /// saturated estimate caps the penalty so the queue keeps a sensible
    /// order.
    fn h_of(&mut self, state: &State, goal_facts: &BTreeSet<causal::Fact>) -> i64 {
        *self.engine.state_mut() = state.clone();
        i64::from(self.causal.h_add(state, &self.engine, goal_facts).min(4096))
    }

    /// Replay-verified coalescing of consecutive Advance records (§5.4):
    /// two records merge only if executing `advance(dt1 + dt2)` from the
    /// first record's entry state reproduces a canonical key equal to the
    /// second record's resulting state (and the same events).
    fn coalesce(&self, mut witness: WitnessPath) -> WitnessPath {
        let mut i = 0;
        while i + 1 < witness.records.len() {
            let (d1, d2) = match (&witness.records[i].step, &witness.records[i + 1].step) {
                (PlayerStep::Advance { dt: d1 }, PlayerStep::Advance { dt: d2 }) => (*d1, *d2),
                _ => {
                    i += 1;
                    continue;
                }
            };
            let a = witness.records[i].clone();
            let b = witness.records[i + 1].clone();
            let before = if i == 0 {
                witness.start.clone()
            } else {
                witness.records[i - 1].state_after.clone()
            };
            let mut outcomes = a.outcomes.clone();
            outcomes.extend(b.outcomes.iter().cloned());
            let mut engine = self.engine.clone();
            *engine.state_mut() = before;
            let run = if outcomes.is_empty() {
                engine.advance(d1 + d2)
            } else {
                let mut cursor = 0usize;
                let mut hook = |_: &ChancePoint| -> Option<ChanceOutcome> {
                    let o = outcomes.get(cursor).cloned();
                    if o.is_some() {
                        cursor += 1;
                    }
                    o
                };
                engine.advance_forked(d1 + d2, &mut hook)
            };
            let merged_ok = match run {
                Err(_) => false,
                Ok(events) => {
                    let key = CanonicalStateKey::of_with(engine.state(), &self.content, &self.oracle);
                    let want = CanonicalStateKey::of_with(&b.state_after, &self.content, &self.oracle);
                    events == b.events && key == want
                }
            };
            if merged_ok {
                let mut due = a.due_moments.clone();
                due.extend(b.due_moments.iter().copied());
                due.sort_unstable();
                due.dedup();
                witness.records[i] = TransitionRecord {
                    step: PlayerStep::Advance { dt: d1 + d2 },
                    outcomes,
                    probability: a.probability * b.probability,
                    due_moments: due,
                    events: b.events,
                    time_before: a.time_before,
                    time_after: b.time_after,
                    state_after: engine.state().clone(),
                    recipe: None,
                };
                witness.records.remove(i + 1);
                // The merged record may merge again with its successor.
            } else {
                i += 1;
            }
        }
        // The cumulative probability is value-preserving under coalescing
        // (the merged record's branch probability is the pair's product).
        witness.depth = witness.records.len();
        witness
    }

    /// Explore the full transition space with canonical-key dedup alone
    /// (§7.1): every explored edge is kept, so backward closure sees the
    /// complete edge set the bounds admit. `stop_at` ends expansion at goal
    /// states (classification keeps it `None`); `invariant` gates every
    /// successor.
    pub fn explore(&mut self, config: &ExploreConfig) -> ExploredGraph {
        let _span = tracing::info_span!("explore").entered();
        let query = ReachQuery {
            goal: GoalPredicate::Ending(None), // never satisfied at a live state; unused
            invariant: config.invariant.clone(),
            objective: SearchObjective::ShortestDepth,
            rng_mode: config.rng_mode,
            max_time: config.max_time,
            max_states: config.max_states,
            allow_mid_run_bind: config.allow_mid_run_bind,
        };
        let mut gen = self.generator(&query);
        let start_key = CanonicalStateKey::of_with(&self.start, &self.content, &self.oracle);
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut index: BTreeMap<CanonicalStateKey, usize> = BTreeMap::new();
        let mut parents: Vec<Option<usize>> = Vec::new(); // first-discovery parent, for witnesses
        let mut parent_edge: Vec<Option<usize>> = Vec::new(); // the edge that discovered the node

        let start = nodes.len();
        nodes.push(GraphNode {
            state: self.start.clone(),
        });
        parents.push(None);
        parent_edge.push(None);
        index.insert(start_key, start);

        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        let mut truncated = false;
        let mut expansions = 0usize;

        while let Some(idx) = queue.pop_front() {
            expansions += 1;
            if expansions > config.max_states {
                truncated = true;
                break;
            }
            let node_state = nodes[idx].state.clone();
            if let Some(stop) = &config.stop_at {
                *self.engine.state_mut() = node_state.clone();
                let ctx = GoalCtx {
                    engine: &self.engine,
                    events: &[],
                };
                if stop.holds(&ctx) {
                    continue;
                }
            }
            if node_state.ended.is_some() {
                continue;
            }
            // The step list is collected first so the borrow of `nodes`
            // ends before successors push into it.
            let steps = gen.steps(&node_state);
            for step in steps {
                let records = gen.execute(&node_state, &step);
                for record in records {
                    if let Some(horizon) = config.max_time {
                        if record.state_after.now > horizon {
                            continue;
                        }
                    }
                    if let Some(inv) = &config.invariant {
                        *self.engine.state_mut() = record.state_after.clone();
                        let ctx = GoalCtx {
                            engine: &self.engine,
                            events: &record.events,
                        };
                        if !inv.holds(&ctx) {
                            continue;
                        }
                    }
                    let key = CanonicalStateKey::of_with(
                        &record.state_after,
                        &self.content,
                        &self.oracle,
                    );
                    let to = match index.get(&key) {
                        Some(&t) => t,
                        None => {
                            let t = nodes.len();
                            // The edge record keeps its own state_after (the
                            // witness rebuild walks it), so the node holds a
                            // clone.
                            nodes.push(GraphNode {
                                state: record.state_after.clone(),
                            });
                            parents.push(None);
                            parent_edge.push(None);
                            index.insert(key, t);
                            queue.push_back(t);
                            t
                        }
                    };
                    // The first edge to discover a node is its witness
                    // parent (a shortest-depth path in BFS order).
                    if parents[to].is_none() && to != start {
                        parents[to] = Some(idx);
                    }
                    let edge = edges.len();
                    edges.push(GraphEdge {
                        from: idx,
                        to,
                        record,
                    });
                    if parent_edge[to].is_none() && to != start {
                        parent_edge[to] = Some(edge);
                    }
                }
            }
        }

        ExploredGraph {
            nodes,
            edges,
            start,
            truncated,
            expansions,
            rng_mode: config.rng_mode,
            parents,
            parent_edge,
        }
    }
}

/// The exploration configuration of `explore` (§7.1): canonical-key dedup
/// alone, so backward closure sees the full explored edge set.
#[derive(Clone)]
pub struct ExploreConfig {
    pub rng_mode: RngMode,
    pub max_states: usize,
    pub max_time: Option<Time>,
    pub invariant: Option<GoalPredicate>,
    /// Stop expanding at states satisfying the predicate (witness-set
    /// collection for the Pareto engine keeps exploring past the goal).
    pub stop_at: Option<GoalPredicate>,
    pub allow_mid_run_bind: bool,
}

impl Default for ExploreConfig {
    fn default() -> ExploreConfig {
        ExploreConfig {
            rng_mode: RngMode::Deterministic,
            max_states: 100_000,
            max_time: None,
            invariant: None,
            stop_at: None,
            allow_mid_run_bind: true,
        }
    }
}

/// One node of the explored graph.
#[derive(Clone, Debug)]
pub struct GraphNode {
    pub state: State,
}

/// One explored edge: a transition record from `from` to `to` (branching
/// modes yield one edge per outcome branch; the record carries its exact
/// probability).
#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub record: TransitionRecord,
}

/// The full explored graph (§7.1): the classifier's, auditor's, and Pareto
/// engine's input. `truncated` marks a bounds-conditional exploration.
#[derive(Clone, Debug)]
pub struct ExploredGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub start: usize,
    pub truncated: bool,
    pub expansions: usize,
    /// The RNG mode the exploration ran under: the witnesses it rebuilds
    /// carry cumulative probability in every mode but `Adversarial` (§6).
    pub rng_mode: RngMode,
    /// First-discovery parents (BFS tree), for witness reconstruction.
    pub parents: Vec<Option<usize>>,
    pub parent_edge: Vec<Option<usize>>,
}

impl ExploredGraph {
    /// The outgoing edges of a node.
    pub fn out_edges(&self, node: usize) -> Vec<usize> {
        self.edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.from == node)
            .map(|(i, _)| i)
            .collect()
    }

    /// The witness path to a node: the BFS-tree records from the start.
    pub fn witness_to(&self, node: usize, start_state: &State) -> Option<WitnessPath> {
        if node >= self.nodes.len() {
            return None;
        }
        let mut records = Vec::new();
        let mut cur = Some(node);
        while let Some(i) = cur {
            match (self.parents[i], self.parent_edge[i]) {
                (Some(p), Some(e)) => {
                    records.push(self.edges[e].record.clone());
                    cur = Some(p);
                }
                _ => {
                    cur = None;
                }
            }
        }
        records.reverse();
        // Adversarial witnesses carry no cumulative probability (§6).
        let probability = if matches!(self.rng_mode, RngMode::Adversarial) {
            None
        } else {
            Some(records.iter().map(|r| r.probability).product::<f64>())
        };
        Some(WitnessPath {
            start: start_state.clone(),
            actions: records.iter().map(|r| r.actions()).sum(),
            depth: records.len(),
            probability,
            records,
        })
    }
}

/// The priority parts per objective (§5.5): `(actions + h, clock, depth)`,
/// `(clock, actions + h, depth)`, `(depth + h, actions, clock)`. `h` rides
/// the objective's own component so the guided search stays well ordered.
fn priority(
    objective: &SearchObjective,
    actions: usize,
    depth: usize,
    clock: Time,
    h: i64,
) -> Vec<i64> {
    match objective {
        SearchObjective::FewestActions => vec![actions as i64 + h, clock, depth as i64],
        SearchObjective::EarliestTime => vec![clock, actions as i64 + h, depth as i64],
        SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
            vec![depth as i64 + h, actions as i64, clock]
        }
    }
}

/// The transition records from the start node to `idx`.
fn path(arena: &[ArenaNode], idx: usize) -> Vec<TransitionRecord> {
    let mut records = Vec::new();
    let mut cur = Some(idx);
    while let Some(i) = cur {
        let node = &arena[i];
        if let Some(r) = &node.record {
            records.push(r.clone());
        }
        cur = node.parent;
    }
    records.reverse();
    records
}

/// Lexicographic priority key over i64 parts, insertion order last.
struct Key {
    parts: Vec<i64>,
    seq: usize,
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts && self.seq == other.seq
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.parts.cmp(&other.parts).then(self.seq.cmp(&other.seq))
    }
}

struct ArenaNode {
    state: State,
    parent: Option<usize>,
    record: Option<TransitionRecord>,
    actions: usize,
    depth: usize,
    /// The recipe trace from the start node (the DiverseWays signature).
    sig: Vec<Option<RecipeId>>,
}

// ---------------------------------------------------------------------------
// The outcome type
// ---------------------------------------------------------------------------

/// The result of `solve`. Claims are bounds-conditional (§1.2): `truncated`
/// means the search hit `max_states` before exhausting the space, so "no
/// witness" is conditional on the bounds; an untruncated empty result is a
/// definitive unreachability answer over the explored space.
#[derive(Clone, Debug)]
pub struct SolveOutcome {
    pub witnesses: Vec<WitnessPath>,
    pub truncated: bool,
    pub expansions: usize,
    pub witnesses_dropped_by_the_oracle: usize,
}

impl SolveOutcome {
    fn empty() -> SolveOutcome {
        SolveOutcome {
            witnesses: Vec::new(),
            truncated: false,
            expansions: 0,
            witnesses_dropped_by_the_oracle: 0,
        }
    }

    /// Whether the query is answered definitively within the bounds (a
    /// witness exists, or the space was exhausted without one).
    pub fn conclusive(&self) -> bool {
        !self.truncated
    }
}
