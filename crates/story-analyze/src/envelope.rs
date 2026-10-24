//! The explored envelope (story-analyze spec §5): a first-class coverage
//! certificate describing the exact manifold of the state space explored
//! during search.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::AspectKey;
use story_core::RecipeId;
use story_core::{CardId, EndingId, Time, VerbId};

/// A quantified certificate of the state-space region explored by the analyzer.
#[derive(Clone, Debug, PartialEq)]
pub struct ExploredEnvelope {
    /// Total state expansions evaluated during exploration.
    pub states_evaluated: usize,
    /// Distinct canonical states identified through symmetry dedup.
    pub unique_canonical_states: usize,
    /// Earliest and latest clock values encountered: [now_min, now_max].
    pub time_span: (Time, Time),

    /// Set of verbs committed or bound during search.
    pub verbs_used: BTreeSet<VerbId>,
    /// Recipes whose instances opened.
    pub recipes_initiated: BTreeSet<RecipeId>,
    /// Recipes that reached completion and fired effects.
    pub recipes_completed: BTreeSet<RecipeId>,
    /// Ratio of distinct completed recipes over declared recipes in Content.
    pub recipe_coverage_ratio: f64,

    /// Observed [min, max] values for tracked card aspects.
    pub aspect_ranges: BTreeMap<(CardId, AspectKey), (i64, i64)>,
    /// All terminal endings triggered within the envelope.
    pub endings_encountered: BTreeSet<EndingId>,

    /// Count of Tier-1 causal landmarks (facts) grounded within the envelope.
    pub causal_landmarks_reached: usize,
    pub total_causal_landmarks: usize,

    /// Number of seed rollout or trace trajectories guiding the search.
    pub seed_trajectories: usize,
    /// Neighborhood radius around seed trajectories.
    pub exhaustive_radius: usize,
    /// Unexpanded transitions at the boundary of the envelope.
    pub frontier_boundary_edges: usize,
}

impl ExploredEnvelope {
    /// Construct an empty envelope for degenerate/trivial outcomes.
    pub fn empty() -> ExploredEnvelope {
        ExploredEnvelope {
            states_evaluated: 0,
            unique_canonical_states: 0,
            time_span: (0, 0),
            verbs_used: BTreeSet::new(),
            recipes_initiated: BTreeSet::new(),
            recipes_completed: BTreeSet::new(),
            recipe_coverage_ratio: 0.0,
            aspect_ranges: BTreeMap::new(),
            endings_encountered: BTreeSet::new(),
            causal_landmarks_reached: 0,
            total_causal_landmarks: 0,
            seed_trajectories: 0,
            exhaustive_radius: 0,
            frontier_boundary_edges: 0,
        }
    }
}
