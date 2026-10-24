//! Full transition-space exploration (§7.1) and its configuration.

use std::collections::{BTreeMap, BTreeSet};

use story_core::{CardId, Time};

use super::graph::{ExploredGraph, GraphEdge, GraphNode};
use super::Analyzer;
use crate::canonical::CanonicalStateKey;
use crate::causal;
use crate::query::{GoalCtx, GoalPredicate, ReachQuery, RngMode, SearchObjective};

impl Analyzer {
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
            prune: false,
            allowed_verbs: config.allowed_verbs.clone(),
            seed_samples: 0,
            seed_trace: None,
            tube_radius: None,
            envelope: false,
            quiescence_pruning: config.quiescence_pruning,
        };
        // Feasibility pruning expands only toward a target: without
        // `stop_at` there is nothing to be "impossible" for, so the flag is
        // ignored (the classification and audit consumers keep their full
        // edge sets by default).
        let prune_facts = if config.prune && !self.causal.host_opaque() {
            config.stop_at.as_ref().map(causal::goal_facts)
        } else {
            None
        };
        let recipe_mintable = if config.prune && !self.causal.host_opaque() {
            self.causal.recipe_mintable_cards()
        } else {
            BTreeSet::new()
        };
        let goal_cards: Vec<CardId> = prune_facts
            .as_ref()
            .map(|facts| {
                facts
                    .iter()
                    .filter_map(|f| match f {
                        causal::Fact::CardLive(c)
                        | causal::Fact::CardInZone(c, _)
                        | causal::Fact::CardHasAspect(c, _) => Some(c.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let scarcity = if config.prune && !self.causal.host_opaque() {
            config
                .stop_at
                .as_ref()
                .map(|g| crate::scarcity::Scarcity::build(g, &self.content, &recipe_mintable))
        } else {
            None
        };
        let mut gen = self.generator(&query);
        let start_key = CanonicalStateKey::of_with(&self.start, &self.content, &self.symmetry);
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
                    if let Some(g) = &config.stop_at {
                        let mut gate_fired = !goal_cards.is_empty()
                            && goal_cards.iter().any(|c| {
                                !record
                                    .state_after
                                    .entities
                                    .values()
                                    .any(|e| &e.prototype == c)
                                    && !recipe_mintable.contains(c)
                            });
                        if !gate_fired {
                            if let Some(s) = &scarcity {
                                if !s.is_empty()
                                    && s.dead(&record.state_after, &self.content, config.max_time)
                                {
                                    gate_fired = true;
                                }
                            }
                        }
                        if !gate_fired {
                            if let Some(facts) = &prune_facts {
                                *self.engine.state_mut() = record.state_after.clone();
                                let raw =
                                    self.causal.h_add(&record.state_after, &self.engine, facts);
                                if raw >= u32::MAX / 4 {
                                    gate_fired = true;
                                }
                            }
                        }
                        if gate_fired {
                            *self.engine.state_mut() = record.state_after.clone();
                            let ctx = GoalCtx {
                                engine: &self.engine,
                                events: &record.events,
                            };
                            if !g.holds(&ctx) {
                                continue;
                            }
                        }
                    }
                    let key = CanonicalStateKey::of_with(
                        &record.state_after,
                        &self.content,
                        &self.symmetry,
                    );
                    let to = match index.get(&key) {
                        Some(&t) => t,
                        None => {
                            let t = nodes.len();
                            // The edge record keeps no state: the discovering
                            // edge's end state IS the node's, and the witness
                            // rebuild reads it from the node (§7.1).
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
                        step: record.step,
                        outcomes: record.outcomes,
                        probability: record.probability,
                        due_moments: record.due_moments,
                        events: record.events,
                        time_before: record.time_before,
                        time_after: record.time_after,
                        recipe: record.recipe,
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
/// alone by default, so backward closure sees the full explored edge set.
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
    /// Heuristic feasibility pruning toward `stop_at` (ignored without it):
    /// branches whose goal facts cannot be grounded even in the delete
    /// relaxation are not expanded, so the explored graph loses edges and
    /// every downstream finding becomes an under-approximation.
    pub prune: bool,
    /// Verbs permitted in this exploration; if `Some`, commits and binds
    /// outside this set are skipped.
    pub allowed_verbs: Option<BTreeSet<story_core::VerbId>>,
    /// Suppress advance steps when the state is completely quiescent.
    pub quiescence_pruning: bool,
}

impl Default for ExploreConfig {
    fn default() -> ExploreConfig {
        ExploreConfig {
            rng_mode: RngMode::Deterministic,
            max_states: 100_000,
            max_time: Some(crate::query::DEFAULT_MAX_TIME),
            invariant: None,
            stop_at: None,
            allow_mid_run_bind: true,
            prune: false,
            allowed_verbs: None,
            quiescence_pruning: true,
        }
    }
}
