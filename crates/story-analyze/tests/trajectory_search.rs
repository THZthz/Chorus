//! Tests for trajectory-guided search, paced rollouts, play traces, and the
//! explored envelope coverage report.

mod common;

use story_analyze::{
    GoalPredicate, PlayTrace, ReachQuery, RolloutConfig, RolloutSampler, TraceStep,
};
use story_content::ir::Expr;
use story_core::VerbId;

const SIMPLE_GAME: &str = r#"
game "SimpleQuest"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  mana: 0
  fire: 0

cards
  card catalyst
    kind gadget stackable
    aspects mana 1
    start in bench count 2

legacies
  victory: 0

endings
  ending won "Won" grand

verbs
  verb cast committable instances 1
    recipe spark
      craftable
      duration 10m
      effects
        legacy victory += 1
"#;

fn goal_victory() -> GoalPredicate {
    GoalPredicate::Expr(Expr::Legacy("victory".into()))
}

#[test]
fn test_play_trace_serialization_and_execution() {
    let content = common::compile(SIMPLE_GAME);
    let mut trace = PlayTrace::new(1, None);
    trace.game = Some("SimpleQuest".into());
    trace.steps = vec![
        TraceStep::Advance { turns: 1 },
        TraceStep::Commit {
            verb: VerbId::from("cast"),
            bindings: vec![],
        },
        TraceStep::Advance { turns: 1 },
    ];

    let json = trace.to_json().expect("serialize trace");
    let deserialized = PlayTrace::from_json(&json).expect("deserialize trace");
    assert_eq!(trace, deserialized);

    let witness = trace
        .execute(content.clone(), story_engine::EffectRegistry::new())
        .expect("execute trace");
    assert_eq!(witness.actions, 1);
    assert_eq!(witness.depth, 3);

    let analyzer = common::analyzer(content, 1);
    let replayer = analyzer.replayer();
    let replayed = replayer.replay(&witness, Some(&goal_victory()), None);
    assert!(replayed.is_ok(), "replayer should accept executed trace");
}

#[test]
fn test_rollout_sampler_produces_valid_witness() {
    let content = common::compile(SIMPLE_GAME);
    let analyzer = common::analyzer(content.clone(), 1);
    let query = ReachQuery::new(goal_victory());

    let gen = analyzer.engine_with_start();
    let config = story_analyze::transitions::GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: query.rng_mode,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
        allowed_verbs: None,
        quiescence_pruning: false,
    };
    let mut generator = story_analyze::TransitionGenerator::new(
        content.clone(),
        gen,
        config,
    );

    let sampler = RolloutSampler::new(
        &content,
        analyzer.causal(),
        &query.goal,
        RolloutConfig {
            max_steps: 10,
            max_time: Some(60),
            rng_mode: query.rng_mode,
            p_advance: 0.5,
        },
    );

    let path = sampler.sample_path(&mut generator, analyzer.start(), &query.goal, 42);
    assert!(!path.records.is_empty(), "rollout sampler should produce steps");
    let replayer = analyzer.replayer();
    let res = replayer.replay(&path, None, None);
    assert!(res.is_ok(), "rollout path should replay cleanly through engine");
}

#[test]
fn test_trajectory_seeded_search_with_envelope() {
    let content = common::compile(SIMPLE_GAME);
    let mut analyzer = common::analyzer(content, 1).without_heuristic();

    let mut query = ReachQuery::new(goal_victory());
    query.seed_samples = 3;
    query.tube_radius = Some(1);
    query.envelope = true;
    query.max_states = 1_000;

    let outcome = analyzer.solve(&query);
    assert!(!outcome.witnesses.is_empty(), "should find victory witness");
    assert!(outcome.envelope.is_some(), "outcome should include explored envelope");

    let env = outcome.envelope.unwrap();
    assert_eq!(env.seed_trajectories, 3);
    assert_eq!(env.exhaustive_radius, 1);
    assert!(env.states_evaluated > 0);
    assert!(env.unique_canonical_states > 0);
    assert!(env.recipes_initiated.contains(&story_core::RecipeId::from("cast.spark")));
}

#[test]
fn test_seed_trace_guides_search_immediately() {
    let content = common::compile(SIMPLE_GAME);
    let mut analyzer = common::analyzer(content, 1).without_heuristic();

    let mut trace = PlayTrace::new(1, None);
    trace.steps = vec![
        TraceStep::Commit {
            verb: VerbId::from("cast"),
            bindings: vec![],
        },
        TraceStep::Advance { turns: 1 },
    ];

    let mut query = ReachQuery::new(goal_victory());
    query.seed_trace = Some(trace);
    query.tube_radius = Some(1);
    query.envelope = true;

    let outcome = analyzer.solve(&query);
    assert_eq!(outcome.witnesses.len(), 1);
    let w = &outcome.witnesses[0];
    assert_eq!(w.actions, 1);

    let env = outcome.envelope.expect("envelope should be computed");
    assert_eq!(env.seed_trajectories, 1);
    assert_eq!(env.exhaustive_radius, 1);
}

#[test]
fn test_anytime_search_retains_single_best_witness_in_non_diverse() {
    let content = common::compile(SIMPLE_GAME);
    let mut analyzer = common::analyzer(content, 1).without_heuristic();

    // A seed trace with unnecessary extra advance before cast
    let mut trace = PlayTrace::new(1, None);
    trace.steps = vec![
        TraceStep::Advance { turns: 5 },
        TraceStep::Commit {
            verb: VerbId::from("cast"),
            bindings: vec![],
        },
        TraceStep::Advance { turns: 1 },
    ];

    let mut query = ReachQuery::new(goal_victory());
    query.objective = story_analyze::SearchObjective::EarliestTime;
    query.seed_trace = Some(trace);
    query.tube_radius = Some(usize::MAX); // full search to improve

    let outcome = analyzer.solve(&query);
    // Non-diverse search should return exactly 1 best witness, not multiple!
    assert_eq!(outcome.witnesses.len(), 1);
    let w = &outcome.witnesses[0];
    // Improved witness should have earlier clock than the seed trace (which delayed 5 turns)
    assert!(w.final_clock() <= 10, "optimal witness should finish in 10m, got {}m", w.final_clock());
}
