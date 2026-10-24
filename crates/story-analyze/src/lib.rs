//! Design-time reachability and quality analysis over compiled Chorus²
//! content (story-analyze spec): a two-tier pipeline that answers "what
//! reaches what" without modifying gameplay semantics.
//!
//! Tier 1 ([`causal::CausalGraph`]) is a static producer/consumer hypergraph
//! over `Arc<Content>` for provenance, plausibility pre-checks, search
//! heuristics, and dead-mechanism pruning. Tier 2 is the explicit-state
//! search: [`transitions`] drives a real kernel engine through the §2.2
//! surface (commit/bind/advance plus the forkable settlement face),
//! [`symmetry`] and [`canonical`] reduce interchangeable entities and merge
//! convergent paths, [`stochastic`] enumerates chance-point branches with
//! exact probabilities, and [`search`] runs the bounded, best-first
//! exploration. Four analysis engines build on the same machinery:
//! [`classify`] (dead ends, softlocks, points of no return), [`mdp`]
//! (optimal play, guarantees, expected value), [`audit`] (shadowing, dead
//! content, reachable faults), and [`pareto`] (trade-off envelopes,
//! counterfactual cuts). Every witness is replay-verifiable through
//! [`replay`] — the oracle re-executes the step list through a fresh kernel
//! engine and asserts the goal state and event stream (kernel invariant T6).
//!
//! The analysis is a tool-layer consumer: `core <- content <- engine <-
//! analyze`, and the kernel never depends on it (spec.md §3). Analyzer
//! certainty is bounded by its bounds (`max_states`, `max_time`); claims
//! over the explored space are conditional on them (§1.2).

pub mod audit;
pub mod canonical;
pub mod causal;
pub mod classify;
pub mod envelope;
pub mod mdp;
pub mod pareto;
pub mod query;
pub mod replay;
pub mod sample;
mod scarcity;
pub mod search;
pub mod slice;
pub mod stochastic;
pub mod symmetry;
pub mod trace;
pub mod transitions;

#[cfg(feature = "lang")]
pub mod lang;

// ---------------------------------------------------------------------------
// The public API surface (story-analyze spec §7/§8)
// ---------------------------------------------------------------------------

pub use audit::{AuditReport, Evidence, FaultEvent, FaultWitness, Shadowing, Unreachable};
pub use canonical::CanonicalStateKey;
pub use causal::{CausalGraph, Fact, Plausibility, Provenance, Site};
pub use classify::{ClassifyConfig, Classification};
pub use envelope::ExploredEnvelope;
pub use mdp::{MdpConfig, MdpOutcome, MdpValues};
pub use pareto::{Direction, Objective, ParetoConfig, ParetoPoint, ParetoReport};
pub use query::{
    EventMatcher, GoalPredicate, ReachQuery, RngMode, SearchObjective, StatePattern,
};
pub use replay::{Replayer, ReplayError};
pub use sample::{RolloutConfig, RolloutSampler};
pub use search::{Analyzer, ExploredGraph, ExploreConfig, GraphEdge, GraphNode, SolveOutcome};
pub use slice::{
    apply_assumption, compose_witnesses, slice_content, verify_composition, Assumption,
    CompositionReport, CompositionViolation, InterfaceBoundary, SliceContract, SliceTarget,
    SlicedAnalyzer,
};
pub use stochastic::{OutcomeClass, RollConsumers, Settlement};
pub use symmetry::{symmetry_classes, EntityEquivSig};
pub use trace::{PlayTrace, TraceStep};
pub use transitions::{PlayerStep, TransitionGenerator, TransitionRecord, WitnessPath};
