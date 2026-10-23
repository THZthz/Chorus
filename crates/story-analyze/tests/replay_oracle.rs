//! The replay oracle (story-analyze spec §5.6, §10 M0 tests): hand-built
//! deterministic witnesses replay to identical states and event streams;
//! branching witnesses replay through their recorded chance outcomes;
//! tampered witnesses fail; refusals never appear inside generated
//! witnesses.

mod common;

use story_analyze::transitions::{GeneratorConfig, TransitionGenerator};
use story_analyze::{GoalPredicate, PlayerStep, ReachQuery, RngMode, Replayer, ReplayError};
use story_content::ir::Expr;

const DICE: &str = r#"
game "Dice"
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
  card prize
    kind treasure  stackable

decks
  deck bag
    entries prize, prize, tool
    wrap

legacies
  luck: 0

endings
  ending won "Won" grand

verbs
  verb cast committable instances 3
    recipe spin
      craftable
      duration 0
      effects
        roll 1d6 + 1 as sight
        if sight >= 5
          ending won

  verb pick committable instances 3
    recipe grab
      craftable
      duration 0
      effects
        draw from bag count 1 into bench
"#;

const PLAIN: &str = r#"
game "Plain"
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

verbs
  verb make committable instances 2
    recipe craft
      craftable
      duration 20m
      slots
        it: 1 consuming aspect:charge
      effects
        legacy luck += 1
"#;

fn gen_of(content: &std::sync::Arc<story_content::Content>, mode: RngMode) -> TransitionGenerator {
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: mode,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
    };
    TransitionGenerator::new(content.clone(), engine, cfg)
}

#[test]
fn hand_built_deterministic_witness_replays_to_identical_states() {
    let content = common::compile(PLAIN);
    let mut gen = gen_of(&content, RngMode::Deterministic);
    let start = gen.state().clone();

    // Assemble a witness by hand from generated records.
    let steps = gen.steps(&start);
    let commit = steps
        .iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .expect("a commit step exists at boot")
        .clone();
    let records = gen.execute(&start, &commit);
    assert_eq!(records.len(), 1);
    let r0 = records.into_iter().next().unwrap();

    // Two turns complete the 20m recipe: luck increments at completion.
    let mut records = vec![r0.clone()];
    let mut cur = r0.state_after.clone();
    for _ in 0..2 {
        let steps = gen.steps(&cur);
        let adv = steps
            .iter()
            .find(|s| matches!(s, PlayerStep::Advance { dt: 10 }))
            .expect("the atomic advance is always legal")
            .clone();
        let rs = gen.execute(&cur, &adv);
        let r = rs.into_iter().next().unwrap();
        cur = r.state_after.clone();
        records.push(r);
    }
    let last = records.last().unwrap().clone();

    let witness = story_analyze::WitnessPath {
        start: start.clone(),
        records,
        actions: 1,
        depth: 3,
        probability: Some(1.0),
    };
    let replayer = Replayer::new(content.clone(), Default::default(), 1, None);
    let final_state = replayer
        .replay(&witness, Some(&GoalPredicate::Expr(Expr::Legacy("luck".into()))), None)
        .expect("the oracle replays the witness");
    // The final state is logically identical to the record's.
    let k1 = story_analyze::CanonicalStateKey::of(&final_state, &content);
    let k2 = story_analyze::CanonicalStateKey::of(&last.state_after, &content);
    assert_eq!(k1, k2);
}

#[test]
fn branching_witness_replays_through_recorded_outcomes() {
    let content = common::compile(DICE);
    let mut gen = gen_of(&content, RngMode::OptimisticBranching);
    let start = gen.state().clone();
    let steps = gen.steps(&start);
    let commit = steps
        .iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .expect("a commit step exists at boot")
        .clone();
    let records = gen.execute(&start, &commit);
    // 1d6 + 1 with an arithmetic-free consumer? `sight` feeds only the
    // branch: classes partition at >= 5 — two branches, each probability
    // one half. (The ending effect sits in the branch; no other reads.)
    assert_eq!(records.len(), 2, "two threshold classes");
    let probs: Vec<f64> = records.iter().map(|r| r.probability).collect();
    let total: f64 = probs.iter().sum();
    assert!((total - 1.0).abs() < 1e-9, "class probabilities sum to 1");
    for r in records {
        assert_eq!(r.outcomes.len(), 1, "one chance point answered");
        let witness = story_analyze::WitnessPath {
            start: start.clone(),
            records: vec![r.clone()],
            actions: 1,
            depth: 1,
            probability: Some(r.probability),
        };
        let replayer = Replayer::new(content.clone(), Default::default(), 1, None);
        // The branch replays through its recorded outcome; only the
        // winning branch satisfies the ending, so the oracle runs goalless
        // here — the stream and canonical-state assertions are the point.
        replayer
            .replay(&witness, None, None)
            .unwrap_or_else(|e| panic!("branch {:?} failed to replay: {e}", r.outcomes));
    }
}

#[test]
fn tampered_witnesses_fail_the_oracle() {
    let content = common::compile(PLAIN);
    let mut gen = gen_of(&content, RngMode::Deterministic);
    let start = gen.state().clone();
    let steps = gen.steps(&start);
    let commit = steps
        .iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .expect("a commit step exists")
        .clone();
    let mut record = gen.execute(&start, &commit).into_iter().next().unwrap();

    // Tampered event stream: the oracle rejects.
    let mut witness = story_analyze::WitnessPath {
        start: start.clone(),
        records: vec![record.clone()],
        actions: 1,
        depth: 1,
        probability: Some(1.0),
    };
    let replayer = Replayer::new(content.clone(), Default::default(), 1, None);
    record.events.push(story_engine::Event::Narration {
        text: "forged".into(),
    });
    witness.records = vec![record];
    match replayer.replay(&witness, Some(&GoalPredicate::Ending(None)), None) {
        Err(ReplayError::EventMismatch { .. }) => {}
        other => panic!("expected EventMismatch, got {other:?}"),
    }

    // A refused step never replays: a commit the state cannot take.
    let bogus = story_analyze::WitnessPath {
        start: start.clone(),
        records: vec![story_analyze::TransitionRecord {
            step: PlayerStep::Commit {
                verb: "nosuchverb".into(),
                bindings: Vec::new(),
            },
            outcomes: Vec::new(),
            probability: 1.0,
            due_moments: Vec::new(),
            events: Vec::new(),
            time_before: 0,
            time_after: 0,
            state_after: start.clone(),
            recipe: None,
        }],
        actions: 1,
        depth: 1,
        probability: Some(1.0),
    };
    match replayer.replay(&bogus, Some(&GoalPredicate::Ending(None)), None) {
        Err(ReplayError::ActionRefused { .. }) => {}
        other => panic!("expected ActionRefused, got {other:?}"),
    }
}

#[test]
fn refusals_never_appear_inside_generated_witnesses() {
    let content = common::compile(DICE);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.rng_mode = RngMode::OptimisticBranching;
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty());
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    for w in &out.witnesses {
        for r in &w.records {
            // A refused action produces no record at all (T3); a record's
            // stream must never carry a diagnostic-flavored event set —
            // replay success is the proof, and it ran in `solve`.
            assert!(!r.events.is_empty() || matches!(r.step, PlayerStep::Advance { .. }));
        }
    }
}
