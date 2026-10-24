//! Entries 21, 22, 23 — instant recipes: inline commit resolution, the recursion budgets, the autonomous pulse.

use super::*;

// matrix 21
/// A `duration 0` commit completes inline: bind, effect, and completion
/// events return together, greedy pulls applied, consuming burned, zero
/// time spent, nothing left due.
#[test]
fn matrix_21_instant_commit_pos() {
    let src = r#"
game "Jab"
  seed host
  turn 1m
  home hall

zones
  hall
  shore

aspects
  spark: 0
  kindle: 0

cards
  card coal
    stackable
    aspects spark 1
    start in hall

  card wood
    stackable
    aspects kindle 1
    start in shore count 2

  card hallmark
    stackable
    start in hall

verbs
  verb poke committable instances 1
    recipe jab
      duration 0
      slots
        fuel: 1..1 consuming aspect:spark
        extra: 0..2 greedy open aspect:kindle
      effects
        narrate "jabbed"
"#;
    let mut e = game(src);
    let home = zone_of(&e, common::named(&e, "hallmark"));
    let events = common::commit_named(&mut e, "poke", &[("fuel", "coal")]).expect("commits");
    let bounds = events
        .iter()
        .filter(|ev| matches!(ev, Event::Bound { .. }))
        .count();
    assert_eq!(bounds, 3, "the explicit coal plus two greedy pulls");
    assert!(narration(&events) == vec!["jabbed"]);
    assert_eq!(completed_count(&events), 1, "completion in the same return");
    assert_eq!(e.state().now, 0, "an instant recipe spends no time");
    assert_eq!(count_named(&e, "coal"), 0, "the consuming slot burned");
    assert_eq!(count_named(&e, "wood"), 2);
    for w in ids_of(&e, "wood") {
        assert_eq!(zone_of(&e, w), home, "released survivors go home");
    }
    let events = e.advance(1).expect("quiet");
    assert!(
        events.is_empty(),
        "an instant recipe never enters the queue"
    );
}

// matrix 21
/// A failing instant commit refuses with the state bit-identical and the
/// recipe's run counter unbumped.
#[test]
fn matrix_21_instant_commit_neg() {
    let src = r#"
game "Jab"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb poke committable instances 1
    recipe snap
      duration 0
      effects
        consume aspect:ghost 1
"#;
    let mut e = game(src);
    let before = state_bytes(&e);
    assert_eq!(
        err_kind(common::commit_named(&mut e, "poke", &[])),
        DiagKind::EffectFailure
    );
    assert_eq!(state_bytes(&e), before);
    assert_eq!(recipe_runs(&e, "poke.snap"), 0, "the bump rolled back");
    assert_eq!(situations(&e).len(), 0);
}

// matrix 22
/// A bounded instant chain resolves fully inline: three links, three
/// completions, one commit return.
#[test]
fn matrix_22_recursion_budget_pos() {
    let src = r#"
game "Chain"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb a committable instances any
    recipe go
      duration 0
      effects
        start b

  verb b uncommittable instances any
    recipe go
      chain-only
      duration 0
      effects
        start c

  verb c uncommittable instances any
    recipe go
      chain-only
      duration 0
      effects
        narrate "c-done"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "a", &[]).expect("resolves inline");
    assert_eq!(completed_count(&events), 3, "the whole chain, one return");
    assert_eq!(recipe_runs(&e, "a.go"), 1);
    assert_eq!(recipe_runs(&e, "b.go"), 1);
    assert_eq!(recipe_runs(&e, "c.go"), 1);
}

// matrix 22
/// The recursion budget: an exponentially forking instant self-chain
/// breaches the per-call resolution budget and takes the Livelock
/// rollback path — the whole commit rolls back bit-for-bit, on an
/// ordinary main thread. (The depth-cap twin lives in
/// kernel::instant::instant_chain_over_budget_livelocks_and_rolls_back.)
#[test]
fn matrix_22_recursion_budget_neg() {
    let src = r#"
game "Fork"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb loop committable instances any
    recipe recurse
      duration 0
      effects
        start loop count 2
"#;
    let mut e = game(src);
    let before = state_bytes(&e);
    assert_eq!(
        err_kind(common::commit_named(&mut e, "loop", &[])),
        DiagKind::Livelock
    );
    assert_eq!(state_bytes(&e), before, "the whole commit rolled back");
}

// matrix 23
/// An all-instant autonomous verb pulses exactly once per checkpoint and
/// never occupies the due queue.
#[test]
fn matrix_23_autonomous_pulse_pos() {
    let src = r#"
game "Pulse"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb pulse autonomous instances 1
    recipe tick
      duration 0
      effects
        narrate "ticked"
"#;
    let mut e = game(src);
    assert_eq!(recipe_runs(&e, "pulse.tick"), 1, "the boot checkpoint");
    assert_eq!(situations(&e).len(), 0, "an instant pulse leaves no queue");
    let events = e.advance(1).expect("one checkpoint");
    assert_eq!(narration(&events), vec!["ticked"]);
    assert_eq!(recipe_runs(&e, "pulse.tick"), 2);
    let events = e.advance(5).expect("one more checkpoint");
    assert_eq!(narration(&events), vec!["ticked"]);
    assert_eq!(recipe_runs(&e, "pulse.tick"), 3, "one pulse per checkpoint");
    assert_eq!(situations(&e).len(), 0);
}

// matrix 23
/// The post-fix checkpoint policy: a failing instant pulse rolls back
/// that verb's open only — sibling verbs' events survive, and the
/// `ResolutionFailed` event names the situation that the failing pulse
/// really materialized (its `Bound` is in the same advance) before the
/// rollback freed it.
#[test]
fn matrix_23_autonomous_pulse_neg() {
    let src = r#"
game "Pulse"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb keeper autonomous instances 1
    recipe hold
      duration 10m
      effects
        narrate "held"

  verb keeper2 autonomous instances 1
    recipe hold2
      duration 10m
      effects
        narrate "held2"

  verb broken autonomous instances 1
    recipe snap
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        consume aspect:ghost 1
"#;
    let mut e = game(src);
    let events = e.advance(10).expect("the first settle");
    assert_eq!(
        narration(&events),
        vec!["held", "held2"],
        "the siblings' completions survived the failing pulse"
    );
    let failed: Vec<SituationId> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::ResolutionFailed { sit, .. } => Some(*sit),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "exactly one failing pulse");
    let failed = failed[0];
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            Event::Bound { sit, .. } if *sit == failed
        )),
        "the failing pulse materialized and bound before it failed"
    );
    assert!(
        !e.state().situations.contains_key(&failed),
        "the failing open rolled back"
    );
    let live: Vec<String> = situations(&e)
        .into_iter()
        .map(|s| e.state().situations[&s].recipe.to_string())
        .collect();
    assert_eq!(
        live,
        vec!["keeper.hold".to_string(), "keeper2.hold2".to_string()],
        "both keepers reopened at the settled moment"
    );
}
