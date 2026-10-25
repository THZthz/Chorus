//! The explored graph: nodes, edges, and witness reconstruction.

use story_core::{RecipeId, State, Time};
use story_engine::Event;

use crate::query::RngMode;
use crate::transitions::{PlayerStep, TransitionRecord, WitnessPath};

/// One node of the explored graph.
#[derive(Clone, Debug)]
pub struct GraphNode {
    pub state: State,
}

/// One explored edge: a transition record from `from` to `to` (branching
/// modes yield one edge per outcome branch; the record carries its exact
/// probability). The record's end state is not stored — for the discovering
/// edge it is exactly `nodes[to]`'s state, and every other edge's end state
/// is canonical-equal to its destination node's; `as_record` reassembles a
/// full record against a node state.
#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub step: PlayerStep,
    pub outcomes: Vec<story_engine::ChanceOutcome>,
    pub probability: f64,
    pub due_moments: Vec<Time>,
    pub events: Vec<Event>,
    pub time_before: Time,
    pub time_after: Time,
    pub recipe: Option<RecipeId>,
}

impl GraphEdge {
    /// The full transition record against the edge's end state — the
    /// destination node's state for the discovering edge, a canonical-equal
    /// state for any other edge.
    pub fn as_record(&self, state_after: State) -> TransitionRecord {
        TransitionRecord {
            step: self.step.clone(),
            outcomes: self.outcomes.clone(),
            probability: self.probability,
            due_moments: self.due_moments.clone(),
            events: self.events.clone(),
            time_before: self.time_before,
            time_after: self.time_after,
            state_after,
            recipe: self.recipe.clone(),
        }
    }
}

/// The full explored graph (§7.1): the classifier's, auditor's, and Pareto
/// engine's input. `truncated` marks a bounds-conditional exploration.
#[derive(Clone, Debug)]
pub struct ExploredGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub start: usize,
    pub truncated: bool,
    pub budget_depleted: bool,
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
    /// Each record's end state is the node it arrived at, so the rebuild
    /// reads the states off the node chain.
    pub fn witness_to(&self, node: usize, start_state: &State) -> Option<WitnessPath> {
        if node >= self.nodes.len() {
            return None;
        }
        let mut records = Vec::new();
        let mut cur = Some(node);
        while let Some(i) = cur {
            match (self.parents[i], self.parent_edge[i]) {
                (Some(p), Some(e)) => {
                    records.push(self.edges[e].as_record(self.nodes[i].state.clone()));
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
