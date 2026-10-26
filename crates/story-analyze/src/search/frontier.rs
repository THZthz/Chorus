//! The search frontier: the priority key, arena nodes, and ordering helpers.

use story_core::{RecipeId, State, Time};

use crate::query::SearchObjective;
use crate::transitions::TransitionRecord;

/// The priority parts per objective (§5.5): `(actions + h, clock, depth)`,
/// `(clock, actions + h, depth)`, `(depth + h, actions, clock)`. `h` rides
/// the objective's own component so the guided search stays well ordered.
pub(super) fn priority(
    objective: &SearchObjective,
    actions: usize,
    depth: usize,
    clock: Time,
    h: i64,
) -> [i64; 3] {
    match objective {
        SearchObjective::FewestActions => [actions as i64 + h, clock, depth as i64],
        SearchObjective::EarliestTime => [clock, actions as i64 + h, depth as i64],
        SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
            [depth as i64 + h, actions as i64, clock]
        }
    }
}

/// The transition records from the start node to `idx`.
pub(super) fn path(arena: &[ArenaNode], idx: usize) -> Vec<TransitionRecord> {
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
pub(super) struct Key {
    pub(super) parts: [i64; 3],
    pub(super) seq: usize,
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

pub(super) struct ArenaNode {
    pub(super) state: Option<State>,
    pub(super) parent: Option<usize>,
    pub(super) record: Option<TransitionRecord>,
    pub(super) actions: usize,
    pub(super) depth: usize,
    /// The recipe trace from the start node (the DiverseWays signature).
    pub(super) sig: Vec<Option<RecipeId>>,
    pub(super) dist_to_spine: usize,
    pub(super) h: i64,
    pub(super) h_computed: bool,
    pub(super) order: i64,
}

impl ArenaNode {
    #[inline]
    pub(super) fn state(&self) -> &State {
        if let Some(r) = &self.record {
            &r.state_after
        } else {
            self.state.as_ref().expect("root state")
        }
    }
}
