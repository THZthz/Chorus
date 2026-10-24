//! Instant recipes: inline open and completion, the recursion budget (T5), the autonomous pulse.

use super::*;

const SRC: &str = r#"
game "Instant"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1

verbs
  verb poke committable instances 1
    recipe jab
      duration 0
      effects
        spawn coin
        narrate "poked"

  verb loop committable instances any
    recipe recurse
      duration 0
      effects
        start loop

  verb pulse autonomous instances 1
    recipe tick
      duration 0
      effects
        narrate "ticked"
"#;

/// A `duration 0` commit completes inline: the open and completion events
/// return together and the interior leftovers go home (spec.md §4.2).
#[test]
fn instant_commit_opens_and_completes_together() {
    let mut e = game(SRC);
    let events = common::commit_named(&mut e, "poke", &[]).expect("commit");
    assert_eq!(narration(&events), vec!["poked"]);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
    assert_eq!(count_named(&e, "coin"), 1);
    let coin = common::named(&e, "coin");
    assert_eq!(common::zone_kind_of(&e, coin), "table", "leftovers go home");
    assert_eq!(recipe_runs(&e, "poke.jab"), 1);
    // The boot checkpoint already pulsed the autonomous instant verb.
    e.advance(50).expect("advance to 50m");
    let events = e.advance(10).expect("one more checkpoint");
    assert_eq!(narration(&events), vec!["ticked"], "exactly one pulse");
    assert_eq!(recipe_runs(&e, "pulse.tick"), 3, "boot, 50m, and 60m");
}

/// The recursion budget: an instant self-start chain bottoms out at the
/// native depth cap and takes the Livelock rollback path (T5) — no coin,
/// no run counters — on an ordinary main thread, no big-stack harness.
#[test]
fn instant_chain_over_budget_livelocks_and_rolls_back() {
    let mut e = game(SRC);
    let before = state_bytes(&e);
    let err = common::commit_named(&mut e, "loop", &[]).expect_err("livelocks");
    assert_eq!(err.kind, DiagKind::Livelock);
    assert_eq!(state_bytes(&e), before, "the whole commit rolled back");
}
