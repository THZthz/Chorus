//! Entries 4, 15, 16, 30 — chains: via/count, fan-out, guards, and candidacy.

use super::*;

// matrix 04
/// `start via <recipe> count <n>` pins the recipe and fans out n opens.
#[test]
fn matrix_04_start_via_count_pos() {
    let src = r#"
game "Handoff"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start picked via second count 3

  verb picked uncommittable instances any
    recipe first
      chain-only
      duration 10m
      effects
        narrate "first-path"
    recipe second
      chain-only
      duration 10m
      effects
        narrate "second-path"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert_eq!(recipe_runs(&e, "picked.second"), 3, "count fanned out");
    assert_eq!(situations(&e).len(), 3);
    assert_eq!(
        e.state()
            .situations
            .values()
            .filter(|s| s.recipe.as_str() == "picked.second")
            .count(),
        3,
        "via pinned the recipe"
    );
}

// matrix 04
/// A `via` hand-off still checks `when`: a false one ends the chain
/// silently and nothing opens.
#[test]
fn matrix_04_start_via_count_neg() {
    let src = r#"
game "Handoff"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start picked via second

  verb picked uncommittable instances any
    recipe second
      chain-only
      duration 10m
      when count(world, aspect:ghost) >= 99
      effects
        narrate "second-path"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert!(chain_ended(&events, "picked"));
    assert_eq!(situations(&e).len(), 0, "nothing opened");
}

// matrix 15
/// Fan-out across one list opens every branch, and multiple advances
/// settle the instances in due order.
#[test]
fn matrix_15_multi_advance_fanout_pos() {
    let src = r#"
game "Fan"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start alpha
        start beta
        start gamma

  verb alpha uncommittable instances any
    recipe a
      chain-only
      duration 10m
      effects
        narrate "alpha-done"

  verb beta uncommittable instances any
    recipe b
      chain-only
      duration 30m
      effects
        narrate "beta-done"

  verb gamma uncommittable instances any
    recipe g
      chain-only
      duration 20m
      effects
        narrate "gamma-done"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert_eq!(situations(&e).len(), 3);
    assert_eq!(narration(&e.advance(10).expect("t=10")), vec!["alpha-done"]);
    assert_eq!(narration(&e.advance(10).expect("t=20")), vec!["gamma-done"]);
    assert_eq!(narration(&e.advance(10).expect("t=30")), vec!["beta-done"]);
}

// matrix 15
/// Fan-out is best-effort: a branch skipped at the instance cap never
/// rolls back its siblings in the same list.
#[test]
fn matrix_15_multi_advance_fanout_neg() {
    let src = r#"
game "Fan"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start capped
        start capped
        start free

  verb capped uncommittable instances 1
    recipe c
      chain-only
      duration 5m

  verb free uncommittable instances any
    recipe f
      chain-only
      duration 5m
      effects
        narrate "free-open"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert!(start_skipped(
        &events,
        "capped",
        &StartSkipReason::InstanceCap
    ));
    let live: Vec<String> = situations(&e)
        .into_iter()
        .map(|s| e.state().situations[&s].recipe.to_string())
        .collect();
    assert_eq!(
        live,
        vec!["capped.c".to_string(), "free.f".to_string()],
        "the second capped branch skipped, the free branch opened"
    );
}

// matrix 16
/// The winner's guard refuses the whole commit with its prose, costing
/// zero state and zero time (the T3 face for refusals).
#[test]
fn matrix_16_guard_refused_pos() {
    let src = r#"
game "Vault"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  sealed: 0

cards
  card beacon
    unique
    aspects sealed 0
    start in hall

verbs
  verb sealer committable instances 1
    recipe seal
      duration 0
      effects
        set beacon.sealed = 1

  verb vault committable instances 1
    recipe open_it
      duration 5m
      guard "The vault is sealed." when beacon.sealed >= 1
      effects
        narrate "opened"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "sealer", &[]).expect("seals");
    let before = state_bytes(&e);
    let now = e.state().now;
    let refused = common::commit_named(&mut e, "vault", &[]).expect_err("guarded");
    assert_eq!(refused.kind, DiagKind::GuardRefused);
    assert!(
        refused.message.contains("The vault is sealed."),
        "the guard's prose rides the refusal: {refused}"
    );
    assert_eq!(state_bytes(&e), before, "zero state change");
    assert_eq!(e.state().now, now, "zero time spent");
}

// matrix 16
/// A false guard opens normally, and a guard hit during a `start` ends
/// the chain silently instead of refusing.
#[test]
fn matrix_16_guard_refused_neg() {
    let src = r#"
game "Vault"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  sealed: 0

cards
  card beacon
    unique
    aspects sealed 0
    start in hall

verbs
  verb sealer committable instances 1
    recipe seal
      duration 0
      effects
        set beacon.sealed = 1

  verb vault committable instances 1
    recipe open_it
      duration 5m
      guard "The vault is sealed." when beacon.sealed >= 1
      effects
        narrate "opened"

  verb starter committable instances 1
    recipe go
      duration 0
      effects
        start vault
"#;
    let mut e = game(src);
    // Guard false: the commit proceeds.
    common::commit_named(&mut e, "vault", &[]).expect("opens");
    e.advance(5).expect("completes");
    // Guard true on the start path: the chain ends silently.
    common::commit_named(&mut e, "sealer", &[]).expect("seals");
    let events = common::commit_named(&mut e, "starter", &[]).expect("runs");
    assert!(chain_ended(&events, "vault"));
    assert_eq!(situations(&e).len(), 0);
}

// matrix 30
/// Autonomous candidacy counts state-qualifying greedy candidates per
/// slot: a satisfiable minimum opens at the checkpoint and again at the
/// next one after release (spec.md §4.3).
#[test]
fn matrix_30_candidacy_pos() {
    let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      slots
        orb: 1..1 aspect:spark
      effects
        narrate "watching"
"#;
    let mut e = game(src);
    assert_eq!(
        e.state()
            .situations
            .values()
            .filter(|s| s.recipe.as_str() == "watcher.stare")
            .count(),
        1,
        "the boot checkpoint opened the stare"
    );
    let events = e.advance(20).expect("completes and repolls");
    assert_eq!(narration(&events), vec!["watching"]);
    assert!(
        events.iter().any(|ev| matches!(ev, Event::Bound { .. })),
        "the reopen at the settled moment re-bound the orb"
    );
    assert_eq!(recipe_runs(&e, "watcher.stare"), 2);
    assert_eq!(situations(&e).len(), 1);
}

// matrix 30
/// Post-fix candidacy: a per-slot minimum above the state-countable
/// candidates makes the recipe NOT a candidate — the start chain ends
/// (`ChainEnded`, never `StartSkipped`), and an autonomous poll misses
/// silently (review guidance for M2, entry 30).
#[test]
fn matrix_30_candidacy_neg_unmet_minimum() {
    let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      when count(world, aspect:spark) >= 1
      slots
        orb: 2..2 aspect:spark

  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start watcher
"#;
    let mut e = game(src);
    // Autonomous: the poll misses silently — nothing opens, no event.
    let events = e.advance(20).expect("advance");
    assert_eq!(situations(&e).len(), 0, "the unmet minimum never opens");
    assert!(
        !events.iter().any(|ev| matches!(
            ev,
            Event::StartSkipped { verb, .. } | Event::ChainEnded { verb }
                if verb.as_str() == "watcher"
        )),
        "a poll miss is silent: {events:?}"
    );
    // Start: the chain ends, and it is not a materialization skip.
    let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert!(chain_ended(&events, "watcher"));
    assert!(!start_skipped(
        &events,
        "watcher",
        &StartSkipReason::MaterializationFailed
    ));
}

// matrix 30
/// `StartSkipped { MaterializationFailed }` needs slots that each pass
/// candidacy yet exhaust the shared pool at materialization: a greedy
/// slot pulling to its max starves the later slot's minimum.
#[test]
fn matrix_30_candidacy_neg_materialization_short() {
    let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start watcher

  verb watcher uncommittable instances 1
    recipe stare
      chain-only
      duration 20m
      when count(world, aspect:spark) >= 2
      slots
        grab: 2..2 greedy aspect:spark
        keep: 1..1 aspect:spark
      effects
        narrate "staring"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
    assert!(start_skipped(
        &events,
        "watcher",
        &StartSkipReason::MaterializationFailed
    ));
    assert!(
        completed_count(&events) >= 1,
        "the parent resolution completed untouched"
    );
    assert_eq!(
        e.state()
            .situations
            .values()
            .filter(|s| s.recipe.as_str() == "watcher.stare")
            .count(),
        0,
        "the short materialization never opened"
    );
}

// matrix 30
/// Locked cards are not state-qualifying candidates: an orb bound by a
/// live instance fails the minimum and the poll misses.
#[test]
fn matrix_30_candidacy_neg_locked_candidates() {
    let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb binder committable instances 1
    recipe hold
      duration 20m
      slots
        kept: 1..1 aspect:spark

  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      when clock >= 10
      slots
        orb: 1..1 aspect:spark
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "binder", &[("kept", "orb")]).expect("locks the orb");
    let events = e.advance(10).expect("to the poll");
    assert_eq!(
        e.state()
            .situations
            .values()
            .filter(|s| s.recipe.as_str() == "watcher.stare")
            .count(),
        0,
        "the locked orb is not a candidate"
    );
    assert!(narration(&events).is_empty(), "a poll miss is silent");
}
