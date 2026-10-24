//! The transition generator (story-analyze spec §5.1, §5.2, §5.4): commit
//! enumeration with canonical binding sets, greedy displacement, mid-run
//! binds, the atomic advance and Tcrit jumps, and replay-verified
//! coalescing.

mod common;

use story_analyze::transitions::{GeneratorConfig, TransitionGenerator};
use story_analyze::{GoalPredicate, PlayerStep, ReachQuery, SearchObjective};
use story_content::ir::{CmpOp, Expr, QueryTerm, Scope};
use story_core::CardId;

/// Two interchangeable tools and one foreign card: the commit enumeration
/// yields one canonical binding set (the oldest-first class prefix).
const SYMMETRY: &str = r#"
game "Sym"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0
  spark: 0

cards
  card tool
    kind gadget  stackable
    aspects charge 1
    start in bench count 3
  card foreign
    kind gadget  stackable
    aspects charge 1
    start in bench count 1
  card prize
    kind treasure  stackable

legacies
  luck: 0

verbs
  verb make committable instances 2
    recipe craft
      craftable
      duration 0
      slots
        it: 1 consuming aspect:charge
      effects
        legacy luck += 1
"#;

fn gen(content: &std::sync::Arc<story_content::Content>) -> TransitionGenerator {
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: story_analyze::RngMode::Deterministic,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
        allowed_verbs: None,
        quiescence_pruning: false,
    };
    TransitionGenerator::new(content.clone(), engine, cfg)
}

#[test]
fn identical_candidates_yield_one_canonical_binding_set() {
    let content = common::compile(SYMMETRY);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let commits: Vec<_> = gen
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Commit { bindings, .. } => Some(bindings),
            _ => None,
        })
        .collect();
    // Four filter-matching candidates, two symmetry classes: the three
    // tools are one contiguous batch (their class prefix is the oldest
    // tool), the foreign card mints after the batch and forms its own
    // class. One commit per class, bound entity = the class prefix.
    assert_eq!(commits.len(), 2, "one commit per candidate class");
    let mut bound: Vec<u64> = commits
        .iter()
        .flatten()
        .map(|(_, e)| e.0)
        .collect();
    bound.sort_unstable();
    assert_eq!(bound, vec![1, 4], "class prefixes: oldest tool, foreign");
}

#[test]
fn batch_symmetry_spawns_one_class() {
    let src = r#"
game "Batch"
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
    start in bench count 4

verbs
  verb make committable instances 2
    recipe craft
      craftable
      duration 0
      slots
        it: 2 consuming aspect:charge
      effects
        narrate "used two"
"#;
    let content = common::compile(src);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let commits: Vec<_> = gen
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Commit { bindings, .. } => Some(bindings),
            _ => None,
        })
        .collect();
    // Four identical tools: the 2-pick prefix is one canonical set.
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].len(), 2);
}

#[test]
fn commits_and_the_atomic_advance_appear_at_boot() {
    let content = common::compile(SYMMETRY);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let steps = gen.steps(&state);
    assert!(steps.iter().any(|s| matches!(s, PlayerStep::Commit { .. })));
    assert!(
        steps
            .iter()
            .any(|s| matches!(s, PlayerStep::Advance { dt: 10 })),
        "the atomic advance(turn) is always legal at a live state"
    );
}

/// A gated recipe plus an autonomous heartbeat: the next critical moment is
/// the gate's clock threshold (or the recipe's ends_at), so a jump edge
/// exists alongside the atomic step.
const GATED: &str = r#"
game "Gate"
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

verbs
  verb heart autonomous instances 1
    recipe beat
      duration 10m
      when clock >= 50m
      effects
        legacy luck += 1

  verb wake committable instances 1
    recipe rise
      craftable
      duration 0
      effects
        ending won
"#;

#[test]
fn the_jump_reaches_the_next_critical_moment() {
    let content = common::compile(GATED);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let jumps: Vec<i64> = gen
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Advance { dt } => Some(dt),
            _ => None,
        })
        .collect();
    // The beat's `when` fails at boot (clock 0 < 50), so no due moment
    // exists yet; the only critical moment past one turn is the static
    // gate threshold 50 — the jump walks it one edge at a time.
    assert_eq!(jumps, vec![10, 50], "atomic step plus the gate jump");
}

#[test]
fn an_open_ended_slot_enumerates_up_to_the_pool() {
    // `0..` with three candidates: the expansion covers every count from
    // the minimum through the pool's size within the pick band — the
    // 3-pick commit is one canonical binding set (the oldest-first class
    // prefix), not just the min and min+1 forms.
    let src = r#"
game "Stack"
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
    start in bench count 3

verbs
  verb hoard committable instances 1
    recipe pile
      craftable
      duration 0
      slots
        it: 0.. consuming aspect:charge
      effects
        narrate "piling"
"#;
    let content = common::compile(src);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let mut sizes: Vec<usize> = gen
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Commit { bindings, .. } => Some(bindings.len()),
            _ => None,
        })
        .collect();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![0, 1, 2, 3], "every affordable stacking is enumerated");
}

#[test]
fn greedy_displacement_forces_a_newer_card() {
    let src = r#"
game "Greedy"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  kindle: 0

cards
  card wood
    kind matter  stackable
    aspects kindle 1
    start in bench count 4

verbs
  verb feed committable instances 1
    recipe burn
      craftable
      duration 0
      slots
        pile: 0..2 greedy open aspect:kindle
      effects
        narrate "burning"
"#;
    let content = common::compile(src);
    let mut gen = gen(&content);
    let state = gen.state().clone();
    let commits: Vec<_> = gen
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Commit { bindings, .. } => Some(bindings),
            _ => None,
        })
        .collect();
    // Baseline: no explicit bindings (greedy tops up). Displacement: one
    // variant per class the greedy pull cannot reach — with one class of
    // four identical cards and a pull of two, the pull's first two cards
    // are reachable, so exactly the baseline remains.
    assert_eq!(commits.len(), 1);
    assert!(commits[0].is_empty(), "the greedy baseline is empty");
}

#[test]
fn coalescing_merges_advance_runs_and_refuses_observable_polls() {
    // In GATED the beats only open at 50m: advancing 10+10 then 10+10
    // converges with the jumps, and consecutive advance records coalesce
    // when the merged call reproduces the same canonical key and events.
    let content = common::compile(GATED);
    let mut a = common::analyzer(content.clone(), 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Expr(Expr::Legacy("luck".into())));
    q.objective = SearchObjective::EarliestTime;
    q.max_states = 500;
    q.max_time = Some(100);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "luck >= 1 is reachable at 60");
    let w = &out.witnesses[0];
    // The witness spans the 50m gate poll with an adjacent advance pair
    // (advance(50) then advance(10)). Coalescing refuses it: the merged
    // advance(60) from boot settles no due item before its target, so its
    // only checkpoint is the end-of-advance poll at 60 — the beat would
    // open too late to complete, and the merged call reproduces neither
    // the event stream nor the state. The count pins the refusal: every
    // observable poll keeps its advance boundary.
    let mut adjacent = 0;
    for pair in w.records.windows(2) {
        if matches!(
            (&pair[0].step, &pair[1].step),
            (PlayerStep::Advance { .. }, PlayerStep::Advance { .. })
        ) {
            adjacent += 1;
        }
    }
    assert_eq!(adjacent, 1, "the gate-poll pair refuses to coalesce");
    // The witness reaches the goal: replay verified in solve.
    assert!(w.final_state().now >= 50);
}

#[test]
fn mid_run_binds_generate_for_open_slots() {
    let src = r#"
game "Midrun"
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
    start in bench count 3

verbs
  verb work committable instances 1
    recipe job
      craftable
      duration 30m
      slots
        it: 1 consuming aspect:charge
      effects
        narrate "working"
        narrate "done"
"#;
    let content = common::compile(src);
    let mut g1 = gen(&content);
    let state = g1.state().clone();
    let steps = g1.steps(&state);
    // At boot the job is not open (no situation yet): no bind steps.
    assert!(!steps.iter().any(|s| matches!(s, PlayerStep::Bind { .. })));
    // Commit, then the situation is live and the open... `it` is not
    // declared open; declare an open slot fixture instead.
    let src_open = src.replace("it: 1 consuming aspect:charge", "it: 1..3 open aspect:charge");
    let content = common::compile(&src_open);
    let mut g2 = gen(&content);
    let commit = g2
        .steps(&state)
        .into_iter()
        .find(|s| matches!(s, PlayerStep::Commit { .. }))
        .unwrap();
    let after = g2.execute(&state, &commit).into_iter().next().unwrap();
    let steps = g2.steps(&after.state_after);
    let binds: Vec<_> = steps
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Bind { entity, .. } => Some(entity),
            _ => None,
        })
        .collect();
    // The slot is open and below max... max is None (open-ended): one
    // canonical representative per candidate class — the three identical
    // tools are one class, so one bind step. (The slot's bound card is
    // locked; the other two are candidates.)
    assert_eq!(binds.len(), 1);
}

/// A 25-minute token lifetime in a 10m-turn game: the expiry is due
/// strictly inside the jump's span and off the turn grid. The kernel's
/// advance accepts only positive turn multiples, so no transition can land
/// on 25 — the walk lands on the enclosing grid point 30 and the record is
/// marked with the due moment its span settled (§5.4).
const DUE: &str = r#"
game "Due"
  seed host
  turn 10m
  home bench

zones
  bench

cards
  card token
    kind gadget  stackable
    lifetime 25m
    start in bench count 1
"#;

#[test]
fn an_off_grid_due_moment_is_marked_on_its_enclosing_landing() {
    let content = common::compile(DUE);
    let mut g = gen(&content);
    let state = g.state().clone();
    let advances: Vec<i64> = g
        .steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Advance { dt } => Some(dt),
            _ => None,
        })
        .collect();
    assert_eq!(
        advances,
        vec![10, 30],
        "the atomic step plus the walk onto the due moment's enclosing grid point"
    );
    let records = g.execute(&state, &PlayerStep::Advance { dt: 30 });
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.time_after, 30);
    assert_eq!(r.due_moments, vec![25], "the settled due moment is marked");
    assert!(
        !r.state_after
            .entities
            .values()
            .any(|e| e.prototype == CardId::from("token")),
        "the expiry settled inside the jump"
    );
}

#[test]
fn the_invariant_gate_checks_the_due_window() {
    let content = common::compile(DUE);
    let engine = common::boot(content.clone(), 1);
    let inv = GoalPredicate::Expr(Expr::Cmp {
        op: CmpOp::Ge,
        left: Box::new(Expr::Count {
            scope: Scope::Anywhere,
            query: vec![QueryTerm::Name(CardId::from("token"))],
        }),
        right: Box::new(Expr::Lit(1)),
    });
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: story_analyze::RngMode::Deterministic,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: Some(inv),
        query_thresholds: Default::default(),
        allowed_verbs: None,
        quiescence_pruning: false,
    };
    let mut g = TransitionGenerator::new(content, engine, cfg);
    let state = g.state().clone();
    // The atomic step ends before the window: kept, unmarked.
    let atomic = g.execute(&state, &PlayerStep::Advance { dt: 10 });
    assert_eq!(atomic.len(), 1);
    assert!(atomic[0].due_moments.is_empty());
    // The jump settles the expiry at 25 inside its span: the token is gone
    // from every materialized checkpoint, so the gate discards the record.
    let jump = g.execute(&state, &PlayerStep::Advance { dt: 30 });
    assert!(
        jump.is_empty(),
        "a record whose span violates the invariant is discarded"
    );
}

#[test]
fn quiescence_pruning_suppresses_advance_on_idle_states() {
    let content = common::compile(SYMMETRY);
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: story_analyze::RngMode::Deterministic,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
        allowed_verbs: None,
        quiescence_pruning: true,
    };
    let mut g = TransitionGenerator::new(content, engine, cfg);
    let state = g.state().clone();
    let steps = g.steps(&state);
    assert!(
        !steps.iter().any(|s| matches!(s, PlayerStep::Advance { .. })),
        "idle state with no active situations, timers, or autonomous verbs prunes advance"
    );
}
