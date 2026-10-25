//! Tier 2 search (story-analyze spec §5): the `ReachabilityEngine`. A
//! best-first exploration over real kernel transitions with canonical-key
//! dedup (§5.3), the objective's dominance rules (§5.5), the Tier-1
//! `h_add` heuristic ordering the queue (never pruning), and replay-verified
//! trace coalescing (§5.4). The same machinery, with canonical dedup alone,
//! builds the full explored graph the classification and audit engines
//! consume (§7.1).

use std::collections::BTreeMap;
use std::sync::Arc;

use story_content::runtime::RuntimeDiagnostic;
use story_content::Content;
use story_core::State;
use story_engine::{EffectRegistry, Engine};

use crate::causal::CausalGraph;
use crate::query::{GoalCtx, ReachQuery};
use crate::replay::Replayer;
use crate::symmetry::SymmetryCache;
use crate::transitions::{GeneratorConfig, TransitionGenerator};

mod explore;
mod frontier;
mod graph;
mod solve;

pub use explore::ExploreConfig;
pub use graph::{ExploredGraph, GraphEdge, GraphNode};
pub use solve::SolveOutcome;

// ---------------------------------------------------------------------------
// The analyzer
// ---------------------------------------------------------------------------

/// The two-tier analysis entry point: Tier 1 (`CausalGraph`) for provenance,
/// heuristics, and dead-mechanism pruning; Tier 2 (the transition generator
/// plus the search) for everything state-dependent. One booted engine backs
/// both tiers; the search swaps states in through `Engine::state_mut`.
#[derive(Clone)]
pub struct Analyzer {
    content: Arc<Content>,
    registry: EffectRegistry,
    seed: u64,
    legacy_in: Option<BTreeMap<String, i64>>,
    causal: Arc<CausalGraph>,
    /// The content-derived symmetry cache (match oracle plus candidacy
    /// surfaces), built once at boot: canonical keys and the symmetry
    /// classifier read it per state, and rebuilding it per state would walk
    /// every effect in the content each time.
    symmetry: Arc<SymmetryCache>,
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
        let causal = Arc::new(CausalGraph::build(&content));
        let symmetry = Arc::new(SymmetryCache::new(&content));
        let start = engine.state().clone();
        (
            Analyzer {
                content,
                registry,
                seed,
                legacy_in,
                causal,
                symmetry,
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
        *self.engine.state_mut() = state.clone();
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

    /// The content-derived symmetry cache, held for the canonical-key and
    /// symmetry fast paths.
    pub(crate) fn symmetry(&self) -> &SymmetryCache {
        &self.symmetry
    }

    /// The Tier-1 causal graph; the heuristic-feasibility gate reads it per
    /// state.
    pub fn causal(&self) -> &CausalGraph {
        &self.causal
    }

    /// Whether the content registers host ops: the causal graph is then
    /// incomplete and heuristic pruning must stay off.
    pub fn host_opaque(&self) -> bool {
        self.causal.host_opaque()
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
        Replayer::with_symmetry(
            self.content.clone(),
            self.registry.clone(),
            self.seed,
            self.legacy_in.clone(),
            self.symmetry.clone(),
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
    pub(super) fn scratch(&mut self, state: &State) -> GoalCtx<'_> {
        *self.engine.state_mut() = state.clone();
        GoalCtx {
            engine: &self.engine,
            events: &[],
        }
    }

    pub(super) fn generator(&self, query: &ReachQuery) -> TransitionGenerator {
        let config = GeneratorConfig {
            seed: self.seed,
            legacy_in: self.legacy_in.clone(),
            rng_mode: query.rng_mode,
            allow_mid_run_bind: query.allow_mid_run_bind,
            max_time: query.max_time,
            invariant: query.invariant.clone(),
            query_thresholds: query.clock_thresholds(),
            allowed_verbs: query.allowed_verbs.clone(),
            quiescence_pruning: query.quiescence_pruning,
        };
        TransitionGenerator::with_symmetry(
            self.content.clone(),
            self.engine_with_start(),
            config,
            self.symmetry.clone(),
        )
    }
}
