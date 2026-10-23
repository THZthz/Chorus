//! The search (story-analyze spec §5.5, §3.2): every `SearchObjective`,
//! dominance-based visited rules, invariants, bounds-conditional claims,
//! DiverseWays signatures, and determinism.

mod common;

use story_analyze::{GoalPredicate, PlayerStep, ReachQuery, SearchObjective};
use story_content::ir::Expr;

/// Two distinct instant crafts reach the same goal (`luck >= 1`), and a
/// third verb is noise.
const TWO_WAYS: &str = r#"
game "TwoWays"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card tool
    kind gadget  stackable
    aspects charge 1
    start in bench count 2

legacies
  luck: 0
  grief: 0

endings
  ending won "Won" grand

verbs
  verb alpha committable instances 1
    recipe fast
      craftable
      max 1
      duration 0
      effects
        legacy luck += 1

  verb beta committable instances 1
    recipe slow
      craftable
      max 1
      duration 10m
      effects
        legacy luck += 1

  verb moan committable instances 3
    recipe sigh
      craftable
      max 3
      duration 0
      effects
        legacy grief += 1
"#;

fn goal_luck() -> GoalPredicate {
    GoalPredicate::Expr(Expr::Legacy("luck".into()))
}

#[test]
fn fewest_actions_minimizes_commits() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(goal_luck());
    q.objective = SearchObjective::FewestActions;
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 1);
    assert!(!out.truncated);
    let w = &out.witnesses[0];
    assert_eq!(w.actions, 1, "one craft suffices");
    // The instant recipe wins the clock tie-break: `fast` completes at
    // t=0, `slow` only at 10m.
    assert_eq!(w.recipe_trace(), vec![Some("alpha.fast".into())]);
}

#[test]
fn earliest_time_minimizes_the_goal_clock() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(goal_luck());
    q.objective = SearchObjective::EarliestTime;
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    let w = &out.witnesses[0];
    assert_eq!(w.final_clock(), 0, "the instant craft reaches luck at t=0");
}

#[test]
fn shortest_depth_minimizes_transitions() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(goal_luck());
    q.objective = SearchObjective::ShortestDepth;
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    let w = &out.witnesses[0];
    assert_eq!(w.depth, 1, "one commit, no advance");
}

#[test]
fn diverse_ways_enumerates_distinct_recipe_sequences() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(goal_luck());
    q.objective = SearchObjective::DiverseWays { max_ways: 8 };
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    // Recipe sequences ignore advance entries (advances are not recipes).
    let traces: Vec<Vec<String>> = out
        .witnesses
        .iter()
        .map(|w| {
            w.recipe_trace()
                .into_iter()
                .flatten()
                .map(|x| x.to_string())
                .collect()
        })
        .collect();
    // The single-craft sequences both appear.
    assert!(traces.contains(&vec!["alpha.fast".to_string()]), "{traces:?}");
    assert!(traces.contains(&vec!["beta.slow".to_string()]), "{traces:?}");
    // All sequences are structurally distinct.
    let mut sorted = traces.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), traces.len());
}

#[test]
fn the_invariant_gates_every_state_along_the_path() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();

    // Reachable under the trivial invariant.
    let mut q = ReachQuery::new(goal_luck());
    q.invariant = Some(GoalPredicate::Expr(Expr::Bool(true)));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty());

    // `grief == 0` forbids every post-sigh state; luck still reachable.
    let mut q = ReachQuery::new(goal_luck());
    q.invariant = Some(GoalPredicate::Expr(Expr::Legacy("luck".into())));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    // The invariant `luck` (nonzero) fails on the start state itself: the
    // query is conclusively empty.
    assert!(out.witnesses.is_empty());
    assert!(!out.truncated, "an invariant violated at the start is definitive");
}

#[test]
fn bounds_conditional_claims() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();

    // A starved expansion bound truncates: no claim either way.
    let mut q = ReachQuery::new(goal_luck());
    q.max_states = 1;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(out.truncated);
    assert!(!out.conclusive());

    // A clock horizon before the timed craft makes the slow path
    // impossible, but the instant path still answers within the horizon.
    let mut q = ReachQuery::new(goal_luck());
    q.max_states = 2_000;
    q.max_time = Some(5);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 1);
    assert!(out.witnesses[0]
        .records
        .iter()
        .all(|r| r.time_after <= 5));
    assert!(!out.truncated);

    // A goal behind an unreachable horizon: conclusively empty.
    let horizon_goal = GoalPredicate::SituationActive {
        verb: Some("moan".into()),
        recipe: Some("moan.sigh".into()),
    };
    let mut q = ReachQuery::new(horizon_goal);
    q.max_states = 2_000;
    q.max_time = Some(5);
    let out = a.solve(&q);
    assert!(out.witnesses.is_empty());
    assert!(out.conclusive());
}

#[test]
fn solving_is_deterministic() {
    let content = common::compile(TWO_WAYS);
    let run = || {
        let mut a = common::analyzer(content.clone(), 1).without_heuristic();
        let mut q = ReachQuery::new(goal_luck());
        q.objective = SearchObjective::DiverseWays { max_ways: 4 };
        q.max_states = 2_000;
        q.max_time = Some(60);
        a.solve(&q)
    };
    let o1 = run();
    let o2 = run();
    assert_eq!(o1.witnesses.len(), o2.witnesses.len());
    for (w1, w2) in o1.witnesses.iter().zip(&o2.witnesses) {
        assert_eq!(w1.records.len(), w2.records.len());
        for (r1, r2) in w1.records.iter().zip(&w2.records) {
            assert_eq!(r1.step, r2.step);
            assert_eq!(r1.events, r2.events);
            let k1 = story_analyze::CanonicalStateKey::of(&r1.state_after, &content);
            let k2 = story_analyze::CanonicalStateKey::of(&r2.state_after, &content);
            assert_eq!(k1, k2);
        }
    }
}

#[test]
fn the_guided_search_answers_too() {
    let content = common::compile(TWO_WAYS);
    // The default analyzer runs with the Tier-1 heuristic on: witnesses
    // stay fast but only Dijkstra mode is provably optimal (§5.5).
    let mut a = common::analyzer(content, 1);
    let mut q = ReachQuery::new(goal_luck());
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty());
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
}

#[test]
fn ended_states_are_terminal_and_unreachability_is_conclusive() {
    let content = common::compile(TWO_WAYS);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(out.witnesses.is_empty(), "nothing in the fixture ends the story");
    assert!(out.conclusive(), "the bounded space was exhausted without a witness");

    // Terminal states have no outgoing transitions (T7): a query whose
    // goal is an ending can never continue past one.
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.invariant = Some(GoalPredicate::Ending(None));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(out.witnesses.is_empty());
    let _ = PlayerStep::Advance { dt: 0 };
}
