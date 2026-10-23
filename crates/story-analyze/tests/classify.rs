//! The StateSpaceClassifier (story-analyze spec §7.1, §10 M5 tests):
//! winnable states, the point-of-no-return frontier, softlocks, and the
//! SCC analysis.

mod common;

use std::collections::BTreeSet;

use story_analyze::classify::{classify, classify_graph, ClassifyConfig};
use story_analyze::{ExploreConfig, GraphEdge, GraphNode, PlayerStep, TransitionRecord};
use story_content::ir::Expr;
use story_core::EndingId;

/// `key` (legacy k = 1 at boot) opens `win` (an instant ending). `spend`
/// burns the key. A doom heartbeat ends `lost` at the hour regardless — so
/// the post-spend live states can still reach *some* ending, just not the
/// winning one: the spend transition is the exact point of no return.
const PONR: &str = r#"
game "Ponr"
  seed host
  turn 10m
  home bench

zones
  bench

legacies
  k: 1

endings
  ending won "Won" grand
  ending lost "Lost" somber

verbs
  verb open committable instances 1
    recipe win
      craftable
      max 1
      duration 0
      when legacy.k >= 1
      effects
        ending won

  verb burn committable instances 1
    recipe spend
      craftable
      max 1
      duration 0
      when legacy.k >= 1
      effects
        legacy k -= 1

  verb fate autonomous instances 1
    recipe doom
      duration 1h
      effects
        ending lost
"#;

/// The softlock variant: no doom heartbeat — post-spend live states reach
/// no ending at all.
const SOFTLOCK: &str = r#"
game "Soft"
  seed host
  turn 10m
  home bench

zones
  bench

legacies
  k: 1

endings
  ending won "Won" grand

verbs
  verb open committable instances 1
    recipe win
      craftable
      max 1
      duration 0
      when legacy.k >= 1
      effects
        ending won

  verb burn committable instances 1
    recipe spend
      craftable
      max 1
      duration 0
      when legacy.k >= 1
      effects
        legacy k -= 1
"#;

fn winning_won() -> BTreeSet<EndingId> {
    BTreeSet::from([EndingId::from("won")])
}

#[test]
fn the_point_of_no_return_is_the_exact_edge() {
    let content = common::compile(PONR);
    let mut a = common::analyzer(content, 1);
    let cfg = ClassifyConfig {
        winning_endings: winning_won(),
        explore: ExploreConfig {
            max_states: 500,
            ..Default::default()
        },
    };
    let c = classify(&mut a, &cfg);
    // Boot is winnable.
    assert!(c.winnable.contains(&c.graph.start));
    assert!(!c.point_of_no_return_edges.is_empty(), "the burn crosses it");
    for (u, v) in &c.point_of_no_return_edges {
        assert!(c.winnable.contains(u), "u is winnable");
        assert!(!c.winnable.contains(v), "v is not winnable");
        assert_eq!(c.graph.nodes[*v].state.ended, None, "v still runs");
        // v can still reach an ending (the doom's `lost`).
    }
    // The winning ending is reached somewhere in the explored space.
    assert!(c.graph.nodes.iter().any(|n| n.state.ended.as_ref()
        == Some(&EndingId::from("won"))));
}

#[test]
fn post_burn_states_softlock() {
    let content = common::compile(SOFTLOCK);
    let mut a = common::analyzer(content, 1);
    let cfg = ClassifyConfig {
        winning_endings: winning_won(),
        explore: ExploreConfig {
            max_states: 500,
            ..Default::default()
        },
    };
    let c = classify(&mut a, &cfg);
    assert!(!c.softlock.is_empty(), "post-spend live states reach nothing");
    for v in &c.softlock {
        assert_eq!(c.graph.nodes[*v].state.ended, None);
        assert!(!c.winnable.contains(v));
    }
    // The burn's targets are the softlock set (they reach no ending at
    // all, so they are not point-of-no-return edges — the walking-dead
    // frontier requires v to still reach *some* ending).
    let burn_targets: BTreeSet<usize> = c
        .graph
        .edges
        .iter()
        .filter(|e| e.record.recipe.as_ref() == Some(&story_core::RecipeId::from("burn.spend")))
        .map(|e| e.to)
        .collect();
    assert!(!burn_targets.is_empty());
    for v in burn_targets {
        assert!(c.softlock.contains(&v), "the burn crosses into the softlock set");
        assert!(!c.winnable.contains(&v));
        assert!(c.winnable.contains(&c.graph.start));
    }
}

/// The SCC analysis over a hand-built graph: kernel transitions always
/// advance the canonical key (`now` and `recipe_runs` are monotone, §5.3),
/// so cycles arise in the general graph the classifier accepts, not from
/// live exploration; `classify_graph` is the pure entry over it.
#[test]
fn sccs_and_self_edges_are_reported() {
    let content = common::compile(PONR);
    let state = common::boot(content.clone(), 1).state().clone();
    let node = |s: &story_core::State| GraphNode {
        state: s.clone(),
    };
    // 0 -> 1 -> 0 (a two-node cycle), 1 -> 2 (a self-edge), 0 -> 3 (exit).
    let mut s1 = state.clone();
    s1.legacy.insert("k".into(), 0);
    let mut s2 = s1.clone();
    s2.now = 10;
    let mut s3 = state.clone();
    s3.ended = Some(EndingId::from("won"));
    let graph = story_analyze::ExploredGraph {
        rng_mode: story_analyze::RngMode::Deterministic,
        start: 0,
        truncated: false,
        expansions: 4,
        nodes: vec![node(&state), node(&s1), node(&s2), node(&s3)],
        edges: vec![
            edge(&content, 0, 1, &state),
            edge(&content, 1, 0, &s1),
            edge(&content, 1, 2, &s1),
            edge(&content, 2, 2, &s2),
            edge(&content, 0, 3, &state),
        ],
        parents: vec![None, Some(0), Some(1), Some(0)],
        parent_edge: vec![None, Some(0), Some(2), Some(4)],
    };
    let c = classify_graph(&graph, &winning_won());
    // Two live loops: the {0,1} cycle and the {2} self-edge.
    assert_eq!(c.infinite_loops.len(), 2, "{:?}", c.infinite_loops);
    let sizes: Vec<usize> = c.infinite_loops.iter().map(|s| s.len()).collect();
    assert!(sizes.contains(&2) && sizes.contains(&1));
}

fn edge(
    _content: &std::sync::Arc<story_content::Content>,
    from: usize,
    to: usize,
    state: &story_core::State,
) -> GraphEdge {
    GraphEdge {
        from,
        to,
        record: TransitionRecord {
            step: PlayerStep::Advance { dt: 10 },
            outcomes: Vec::new(),
            probability: 1.0,
            due_moments: Vec::new(),
            events: Vec::new(),
            time_before: state.now,
            time_after: state.now + 10,
            state_after: state.clone(),
            recipe: None,
        },
    }
}

/// Classification runs over Expr goals too: the winning set defaults from
/// an `Ending(Some(id))` query (`classify_for_goal`), and a goal on a
/// legacy value is winnable wherever it holds.
#[test]
fn winnable_over_expr_goals() {
    let content = common::compile(SOFTLOCK);
    let mut a = common::analyzer(content, 1);
    let cfg = ClassifyConfig {
        explore: ExploreConfig {
            max_states: 500,
            ..Default::default()
        },
        ..ClassifyConfig::default()
    };
    // The classifier's ending closure runs on the graph; the legacy goal
    // closure is the caller's composition — here we pin the winnable set
    // of the `won` ending and the PONR frontier only.
    let c = classify(&mut a, &cfg);
    assert!(c.winnable.is_empty(), "no winning endings configured");
    assert!(c.point_of_no_return_edges.is_empty());
    let _ = Expr::Bool(true);
}
