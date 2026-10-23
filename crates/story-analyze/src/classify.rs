//! The `StateSpaceClassifier` (story-analyze spec §7.1): dead ends,
//! softlocks, and points of no return over an explored transition graph.
//! Classification explores with canonical-key dedup alone — the objective
//! dominance rules of §5.5 are off, because backward closure needs the full
//! explored edge set — and classifies by backward closure and Tarjan SCC.

use std::collections::BTreeSet;

use story_core::EndingId;

use crate::search::{Analyzer, ExploredGraph, ExploreConfig};

/// The classification result (§7.1). Node ids index `ExploredGraph::nodes`.
#[derive(Clone, Debug)]
pub struct Classification {
    /// Backward closure from the winning endings: the states from which
    /// winning is still possible.
    pub winnable: BTreeSet<usize>,
    /// The "walking dead" frontier: edges `u → v` where `u` is winnable,
    /// `v` still runs and can still reach some ending, but `v` is not
    /// winnable — the exact transition crossing the point of no return.
    pub point_of_no_return_edges: Vec<(usize, usize)>,
    /// Live states from which no ending at all is reachable.
    pub softlock: BTreeSet<usize>,
    /// Live SCCs of size > 1, or single nodes with a self-edge
    /// (state-neutral instant recipes produce self-loops).
    pub infinite_loops: Vec<Vec<usize>>,
    /// The explored graph the classification ran over.
    pub graph: ExploredGraph,
}

/// The classifier configuration: the winning ending set is caller-supplied
/// (endings carry only a free-form tier label the engine never ranks); an
/// `Ending(Some(id))` query defaults the set to `{id}`.
#[derive(Clone)]
#[derive(Default)]
pub struct ClassifyConfig {
    pub winning_endings: BTreeSet<EndingId>,
    pub explore: ExploreConfig,
}


/// Explore (canonical dedup only) and classify.
pub fn classify(analyzer: &mut Analyzer, config: &ClassifyConfig) -> Classification {
    let graph = analyzer.explore(&config.explore);
    classify_graph(&graph, &config.winning_endings)
}

/// Classify an already-explored graph.
pub fn classify_graph(graph: &ExploredGraph, winning: &BTreeSet<EndingId>) -> Classification {
    let _span = tracing::info_span!("classify_closure").entered();
    let n = graph.nodes.len();
    // Adjacency (forward) and reverse adjacency over node ids.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &graph.edges {
        adj[e.from].push(e.to);
        radj[e.to].push(e.from);
    }
    for a in &mut adj {
        a.sort_unstable();
        a.dedup();
    }
    for a in &mut radj {
        a.sort_unstable();
        a.dedup();
    }

    let ended_at = |v: usize| graph.nodes[v].state.ended.clone();
    let winning_nodes: BTreeSet<usize> = (0..n)
        .filter(|&v| matches!(ended_at(v), Some(id) if winning.contains(&id)))
        .collect();
    let any_ended_nodes: BTreeSet<usize> = (0..n).filter(|&v| ended_at(v).is_some()).collect();

    // Backward closure from the winning endings.
    let winnable = backward(&radj, &winning_nodes);

    // Forward reachability to any ending (backward BFS from the endings
    // over the reverse adjacency).
    let reaches_ending = reaches(&radj, n, &any_ended_nodes);

    let mut softlock = BTreeSet::new();
    let mut point_of_no_return_edges = Vec::new();
    for (v, reaches) in reaches_ending.iter().enumerate() {
        if ended_at(v).is_none() && !reaches {
            softlock.insert(v);
        }
    }
    for e in &graph.edges {
        let (u, v) = (e.from, e.to);
        if winnable.contains(&u)
            && ended_at(v).is_none()
            && reaches_ending[v]
            && !winnable.contains(&v)
        {
            point_of_no_return_edges.push((u, v));
        }
    }

    // Tarjan SCC over the explored digraph; live components count as
    // infinite loops.
    let sccs = tarjan(&adj, n);
    let mut infinite_loops = Vec::new();
    for scc in sccs {
        if scc.len() > 1 {
            if scc.iter().any(|&v| ended_at(v).is_none()) {
                infinite_loops.push(scc);
            }
        } else if let Some(&v) = scc.first() {
            if adj[v].contains(&v) && ended_at(v).is_none() {
                infinite_loops.push(scc);
            }
        }
    }

    Classification {
        winnable,
        point_of_no_return_edges,
        softlock,
        infinite_loops,
        graph: graph.clone(),
    }
}

fn backward(radj: &[Vec<usize>], seeds: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut out = seeds.clone();
    let mut stack: Vec<usize> = seeds.iter().copied().collect();
    while let Some(v) = stack.pop() {
        for &u in &radj[v] {
            if out.insert(u) {
                stack.push(u);
            }
        }
    }
    out
}

/// `out[v]` = whether `v` can reach any of `seeds`: a backward BFS from the
/// seeds over the reverse adjacency.
fn reaches(radj: &[Vec<usize>], n: usize, seeds: &BTreeSet<usize>) -> Vec<bool> {
    let mut out = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    for &s in seeds {
        out[s] = true;
        stack.push(s);
    }
    while let Some(v) = stack.pop() {
        for &u in &radj[v] {
            if !out[u] {
                out[u] = true;
                stack.push(u);
            }
        }
    }
    out
}

/// Tarjan's strongly connected components, iterative.
fn tarjan(adj: &[Vec<usize>], n: usize) -> Vec<Vec<usize>> {
    #[derive(Clone)]
    struct Frame {
        v: usize,
        child: usize,
    }
    let mut index: Vec<Option<u32>> = vec![None; n];
    let mut low: Vec<u32> = vec![0; n];
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0u32;
    let mut out: Vec<Vec<usize>> = Vec::new();

    for root in 0..n {
        if index[root].is_some() {
            continue;
        }
        let mut frames = vec![Frame { v: root, child: 0 }];
        while let Some(frame) = frames.last_mut() {
            let v = frame.v;
            if frame.child == 0 {
                index[v] = Some(next);
                low[v] = next;
                next += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            let outs = &adj[v];
            let mut descended = false;
            while frame.child < outs.len() {
                let w = outs[frame.child];
                frame.child += 1;
                if index[w].is_none() {
                    frames.push(Frame { v: w, child: 0 });
                    descended = true;
                    break;
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w].unwrap());
                }
            }
            if descended {
                continue;
            }
            frames.pop();
            if low[v] == index[v].unwrap() {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(comp);
            }
            if let Some(parent) = frames.last_mut() {
                low[parent.v] = low[parent.v].min(low[v]);
            }
        }
    }
    out
}

/// The ending set of an `Ending(Some(id))` query: the classification's
/// default winning set (§7.1).
pub fn ending_set(id: Option<&EndingId>) -> BTreeSet<EndingId> {
    match id {
        Some(id) => BTreeSet::from([id.clone()]),
        None => BTreeSet::new(),
    }
}

/// Convenience: classify from a reach query's ending goal.
pub fn classify_for_goal(
    analyzer: &mut Analyzer,
    mut config: ClassifyConfig,
    goal: &crate::query::GoalPredicate,
) -> Classification {
    if config.winning_endings.is_empty() {
        if let crate::query::GoalPredicate::Ending(id) = goal {
            config.winning_endings = ending_set(id.as_ref());
        }
    }
    classify(analyzer, &config)
}
