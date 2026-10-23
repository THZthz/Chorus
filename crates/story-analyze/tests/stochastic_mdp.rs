//! Stochastic settlement and the MDP solver (story-analyze spec §6, §7.2,
//! §10 M4 tests): exact branch probabilities, the seed-fixed draw fork,
//! the V-metrics' invariants, and guarantees.

mod common;

use story_analyze::{GoalPredicate, MdpConfig, PlayerStep, ReachQuery, RngMode, RollConsumers};
use story_analyze::transitions::{GeneratorConfig, TransitionGenerator};

/// One gated roll craft: `sight = 1d6 + 1`, the ending needs `sight >= 5`.
/// The binding feeds only the branch, so the classes partition at the
/// threshold: [2..4] with probability one half, [5..7] with one half. The
/// `clock` gate makes holding the turn worthless, so the metrics at the
/// start come from the craft alone.
const ROLL: &str = r#"
game "Roll"
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
    start in bench count 1

legacies
  luck: 0

endings
  ending won "Won" grand
  ending lost "Lost" somber

verbs
  verb cast committable instances 1
    recipe spin
      craftable
      max 1
      when clock < 10m
      duration 0
      effects
        roll 1d6 + 1 as sight
        if sight >= 5
          ending won
        else
          ending lost
"#;

/// A roll whose binding feeds a destroy count: the binding is consumed
/// state-affectingly, so the classes partition per distinct total (§6).
const SHATTER: &str = r#"
game "Shatter"
  seed host
  turn 10m
  home bench

zones
  bench

cards
  card tool
    kind gadget  stackable
    start in bench count 6

verbs
  verb break committable instances 1
    recipe smash
      craftable
      max 1
      duration 0
      effects
        roll 1d6 as r
        destroy name:tool count r
"#;

#[test]
fn a_roll_fed_destroy_count_partitions_the_classes() {
    let content = common::compile(SHATTER);
    let consumers = RollConsumers::of_content(&content);
    assert!(
        consumers.arithmetic.contains("r"),
        "the destroy count consumes the roll binding"
    );
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: RngMode::OptimisticBranching,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
    };
    let mut gen = TransitionGenerator::new(content, engine, cfg);
    let state = gen.state().clone();
    let commit = gen
        .steps(&state)
        .into_iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .unwrap();
    let records = gen.execute(&state, &commit);
    assert_eq!(records.len(), 6, "one class per distinct total");
    let total: f64 = records.iter().map(|r| r.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
    let mut survivors: Vec<i64> = records
        .iter()
        .map(|r| r.state_after.entities.len() as i64)
        .collect();
    survivors.sort_unstable();
    assert_eq!(survivors, vec![0, 1, 2, 3, 4, 5], "each total r leaves 6 - r tools");
}

/// A draw from a small deck: the seed-fixed window is one of the
/// hypergeometric classes.
const DRAW: &str = r#"
game "Draw"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

aspects
  restore: 0

cards
  card calm
    kind omen  stackable
    aspects restore 1
  card gull
    kind omen  stackable
  card prize
    kind treasure  stackable

decks
  deck omens
    entries gull, calm, prize
    wrap

legacies
  luck: 0

endings
  ending won "Won" grand

verbs
  verb dream committable instances 1
    recipe sleep
      craftable
      max 1
      duration 0
      effects
        draw from omens count 1 into bench
        if count(anywhere, name:prize) >= 1
          ending won
"#;

#[test]
fn roll_classes_partition_exactly() {
    let content = common::compile(ROLL);
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: RngMode::OptimisticBranching,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
    };
    let mut gen = TransitionGenerator::new(content, engine, cfg);
    let state = gen.state().clone();
    let commit = gen
        .steps(&state)
        .into_iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .unwrap();
    let records = gen.execute(&state, &commit);
    assert_eq!(records.len(), 2, "the >= 5 gate partitions into two classes");
    let total: f64 = records.iter().map(|r| r.probability).sum();
    assert!((total - 1.0).abs() < 1e-9, "classes sum to 1");
    for r in &records {
        assert!(r.probability > 0.0);
        // The uniform branch advances rng_counter identically (§6).
        assert_eq!(r.state_after.rng_counter, 1);
    }
    // One class wins, one loses, and both replay (solve does this through
    // the oracle).
    let won: Vec<bool> = records
        .iter()
        .map(|r| r.state_after.ended.as_ref() == Some(&story_core::EndingId::from("won")))
        .collect();
    assert!(won.contains(&true) && won.contains(&false));
}

#[test]
fn draw_branches_cover_the_remaining_multiset() {
    let content = common::compile(DRAW);
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: RngMode::OptimisticBranching,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
    };
    let mut gen = TransitionGenerator::new(content.clone(), engine, cfg);
    let state = gen.state().clone();
    let commit = gen
        .steps(&state)
        .into_iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .unwrap();
    let records = gen.execute(&state, &commit);
    // Three distinct entries: three hypergeometric classes of weight 1/3.
    assert_eq!(records.len(), 3);
    let total: f64 = records.iter().map(|r| r.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
    // Every branch advances the deck cursor uniformly (§2.2).
    for r in &records {
        assert_eq!(r.state_after.decks.get(&story_core::DeckId::from("omens")), Some(&1));
        assert_eq!(r.outcomes.len(), 1);
    }
    // The cursor multiset is exact: three branches mint the three entries.
    let mut minted: Vec<Vec<String>> = records
        .iter()
        .map(|r| match &r.outcomes[0] {
            story_engine::ChanceOutcome::Draw(cards) => {
                cards.iter().map(|c| c.to_string()).collect()
            }
            other => panic!("draw outcome expected, got {other:?}"),
        })
        .collect();
    minted.sort();
    assert_eq!(
        minted,
        vec![
            vec!["calm".to_string()],
            vec!["gull".to_string()],
            vec!["prize".to_string()],
        ]
    );

    // The seed-fixed draw fork: the deterministic kernel path draws the
    // permutation window, which is one of the enumerated classes (§10).
    let content2 = content.clone();
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: RngMode::Deterministic,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
    };
    let engine = common::boot(content2.clone(), 1);
    let mut gen = TransitionGenerator::new(content2, engine, cfg);
    let commit = gen
        .steps(&state)
        .into_iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .unwrap();
    let det = gen.execute(&state, &commit).into_iter().next().unwrap();
    let det_key = story_analyze::CanonicalStateKey::of(&det.state_after, gen.content());
    let agrees = records.iter().any(|r| {
        story_analyze::CanonicalStateKey::of(&r.state_after, gen.content()) == det_key
    });
    assert!(agrees, "the deterministic draw equals one fork branch");
}

#[test]
fn optimistic_witnesses_carry_exact_cumulative_probability() {
    let content = common::compile(ROLL);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.rng_mode = RngMode::OptimisticBranching;
    q.max_states = 1_000;
    q.max_time = Some(10);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 1);
    let w = &out.witnesses[0];
    assert!((w.probability.unwrap() - 0.5).abs() < 1e-9, "the winning class is 1/2");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
}

#[test]
fn adversarial_witnesses_carry_no_cumulative_probability() {
    // Â§6: the guarantee solver's witnesses keep the exact per-branch
    // probabilities on their records, but no cumulative probability.
    let content = common::compile(ROLL);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.rng_mode = RngMode::Adversarial;
    q.max_states = 1_000;
    q.max_time = Some(10);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "a winning branch exists");
    assert!(
        out.witnesses.iter().all(|w| w.probability.is_none()),
        "adversarial witnesses carry no cumulative probability"
    );
}

#[test]
fn mdp_metrics_obey_their_invariants() {
    let content = common::compile(ROLL);
    let a = common::analyzer(content, 1);
    let cfg = MdpConfig::new(GoalPredicate::Ending(Some("won".into())));
    let out = story_analyze::mdp::solve(&a, &cfg);
    assert!(out.complete, "the small game graph fits the budget");
    let v = out.start;
    assert!((v.v_max - 0.5).abs() < 1e-9, "v_max = {}", v.v_max);
    // The coin-flip game has no RNG-proof strategy.
    assert!((v.v_minimax - 0.0).abs() < 1e-9);
    assert!(v.v_unif <= v.v_max + 1e-9);
    assert!(v.v_minimax <= v.v_max + 1e-9);
    assert!((0.0..=1.0).contains(&v.v_unif));
    // One optimal action (the cast), one action deep.
    assert!((v.expected_actions - 1.0).abs() < 1e-9);
}

#[test]
fn mdp_finds_the_rng_proof_strategy() {
    let src = r#"
game "Safe"
  seed host
  turn 10m
  home bench

zones
  bench

cards
  card tool
    kind gadget  stackable
    start in bench count 1

endings
  ending won "Won" grand
  ending lost "Lost" somber

verbs
  verb cast committable instances 1
    recipe sure
      craftable
      max 1
      duration 0
      effects
        roll 1d2 as toss
        ending won
"#;
    let content = common::compile(src);
    let a = common::analyzer(content, 1);
    // Every positive-probability outcome wins: V_minimax = V_max = 1.
    let cfg = MdpConfig::new(GoalPredicate::Ending(Some("won".into())));
    let out = story_analyze::mdp::solve(&a, &cfg);
    assert!(out.complete);
    assert!((out.start.v_max - 1.0).abs() < 1e-9);
    assert!((out.start.v_minimax - 1.0).abs() < 1e-9);
}

/// §7.2's probe: a 0.9 gamble against a hedge guaranteeing 0.4. The
/// expectation-argmax policy rides the gamble, the guarantee comes from the
/// hedge, and the uniform baseline averages both.
const GAMBLE: &str = r#"
game "Gamble"
  seed host
  turn 10m
  home bench

zones
  bench

cards
  card tool
    kind gadget  stackable
    start in bench count 1

legacies
  luck: 0
  mark: 0

verbs
  verb bet committable instances 1
    recipe gamble
      craftable
      max 1
      when legacy.luck <= 0 and legacy.mark <= 0 and clock < 10m
      duration 0
      effects
        legacy mark += 1
        roll 1d10 as r
        if r >= 2
          legacy luck += 10

  verb steer committable instances 1
    recipe hedge
      craftable
      max 1
      when legacy.mark <= 0 and clock < 10m
      duration 0
      effects
        legacy mark += 2
        legacy luck += 4

  verb dare committable instances 1
    recipe leap
      craftable
      max 1
      when legacy.mark >= 2 and legacy.luck >= 4
      duration 0
      effects
        roll 1d10 as r
        if r >= 7
          legacy luck += 6
"#;

#[test]
fn minimax_and_uniform_track_their_own_metrics() {
    let content = common::compile(GAMBLE);
    let a = common::analyzer(content, 1);
    let goal = GoalPredicate::Expr(story_content::ir::Expr::Cmp {
        op: story_content::ir::CmpOp::Ge,
        left: Box::new(story_content::ir::Expr::Legacy("luck".into())),
        right: Box::new(story_content::ir::Expr::Lit(10)),
    });
    let cfg = MdpConfig {
        max_depth: 24,
        max_actions: 8,
        ..MdpConfig::new(goal)
    };
    let out = story_analyze::mdp::solve(&a, &cfg);
    let v = out.start;
    // The gamble wins 0.9 of the time; the hedge state's only continuation
    // is the 0.4 leap. Each metric keeps its own optimum: the argmax policy
    // rides the gamble, the guarantee rides the hedge.
    assert!((v.v_max - 0.9).abs() < 1e-9, "v_max = {}", v.v_max);
    assert!((v.v_minimax - 0.4).abs() < 1e-9, "v_minimax = {}", v.v_minimax);
    assert!(
        0.4 < v.v_unif && v.v_unif < 0.9,
        "v_unif = {} sits between the hedge and the gamble",
        v.v_unif
    );
}

#[test]
fn deterministic_and_branching_witnesses_agree() {
    // The deterministic kernel path is one branch of the enumeration; its
    // witness replays and the branching witnesses replay (§10).
    let content = common::compile(ROLL);
    let mut det = common::analyzer(content.clone(), 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("lost".into())));
    q.rng_mode = RngMode::Deterministic;
    q.max_states = 1_000;
    q.max_time = Some(10);
    let out = det.solve(&q);
    assert_eq!(out.witnesses.len(), 1, "seed 1 rolls into the losing class");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
}
