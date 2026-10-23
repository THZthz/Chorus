//! Smoke test: solve() end to end on a tiny deterministic fixture.

mod common;

use story_analyze::{GoalPredicate, MdpConfig, ReachQuery, RngMode, SearchObjective};
use story_content::ir::Expr;

const SRC: &str = r#"
game "Smoke"
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
  card prize
    kind treasure  stackable

legacies
  luck: 0

endings
  ending won "Won" grand

verbs
  verb make committable instances 2
    recipe craft
      craftable
      duration 0
      effects
        spawn prize into bench
        legacy luck += 1
        if legacy.luck >= 2
          ending won
"#;

#[test]
fn smoke_solve_reaches_the_ending() {
    let content = common::compile(SRC);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.objective = SearchObjective::FewestActions;
    q.max_states = 500;
    q.max_time = Some(120);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "no witness found");
    assert!(!out.truncated, "truncated");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    for w in &out.witnesses {
        for r in &w.records {
            println!("step {:?} t={} events={}", dbg_step(r), r.time_after, r.events.len());
        }
        println!("actions={} depth={} clock={}", w.actions, w.depth, w.final_clock());
    }
}

fn dbg_step(r: &story_analyze::TransitionRecord) -> String {
    match &r.step {
        story_analyze::PlayerStep::Commit { verb, .. } => format!("commit {verb}"),
        story_analyze::PlayerStep::Bind { .. } => "bind".into(),
        story_analyze::PlayerStep::Advance { dt } => format!("advance {dt}"),
    }
}

#[test]
fn smoke_goal_via_expr() {
    let content = common::compile(SRC);
    let mut a = common::analyzer(content, 1).without_heuristic();
    // legacy luck >= 1 after one craft
    let goal = GoalPredicate::Expr(Expr::Legacy("luck".into()));
    let mut q = ReachQuery::new(goal);
    q.objective = SearchObjective::FewestActions;
    q.max_states = 500;
    q.max_time = Some(120);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "no witness for luck >= 1");
}

#[test]
fn smoke_branching_and_mdp() {
    let content = common::compile(SRC);
    // OptimisticBranching: the spawn is deterministic here, but the mode
    // must not crash and witnesses must replay.
    let mut a = common::analyzer(content.clone(), 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.rng_mode = RngMode::OptimisticBranching;
    q.max_states = 300;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "no branching witness");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    for w in &out.witnesses {
        let p: f64 = w.records.iter().map(|r| r.probability).product();
        assert_eq!(w.probability, Some(p), "the cumulative is the product");
        assert!(p > 0.0 && p <= 1.0);
    }

    // MDP: the craft is deterministic, so V_max = 1 and the guarantee holds.
    let cfg = MdpConfig::new(GoalPredicate::Ending(Some("won".into())));
    let out = story_analyze::mdp::solve(&a, &cfg);
    println!("mdp: {:?}", out.start);
    assert!(out.start.v_max > 0.999, "v_max = {}", out.start.v_max);
    assert!(out.start.v_minimax > 0.999);
    assert!(out.start.v_unif <= out.start.v_max + 1e-9);
}
