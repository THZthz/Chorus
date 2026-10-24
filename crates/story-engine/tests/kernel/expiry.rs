//! Context-free expiry books (T9) and expiry deferred while bound.

use super::*;

const SRC: &str = r#"
game "Decay"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card ephemeral
    lifetime 5m
    aspects spark 1
    start in hall
    on expire
      rule linger when total(bound, aspect:spark) == 0 and count(interior, aspect:spark) == 0
        effects
          narrate "expired-clean"

verbs
  verb binder committable instances 1
    recipe hold
      duration 10m
      slots
        kept: 1..1 aspect:spark
"#;

/// T9: expiry books are context-free — `bound` and `interior` read empty —
/// and the book replaces default destruction.
#[test]
fn expiry_book_is_context_free_and_replaces_destruction() {
    let mut e = game(SRC);

    let events = e.advance(5).expect("advance to the due moment");
    assert_eq!(narration(&events), vec!["expired-clean"]);
    assert_eq!(
        count_named(&e, "ephemeral"),
        1,
        "the book replaced destruction"
    );
    let card = common::named(&e, "ephemeral");
    assert_eq!(e.state().entities[&card].expires_at, None, "one-shot timer");
}

/// A bound card's expiry is deferred: it fires on the next resolution
/// pass after release (spec.md §4.6).
#[test]
fn expiry_is_deferred_while_bound_and_fires_after_release() {
    let mut e = game(SRC);
    let card = common::named(&e, "ephemeral");
    common::commit_named(&mut e, "binder", &[("kept", "ephemeral")]).expect("bound");

    // Past due while bound: nothing fires, the timer waits.
    let events = e.advance(5).expect("to the due moment");
    assert!(narration(&events).is_empty(), "bound cards do not expire");
    assert_eq!(e.state().entities[&card].expires_at, Some(5));
    assert_eq!(count_named(&e, "ephemeral"), 1);

    // Released at 10m, past due: the same pass expires it, book first.
    let events = e.advance(5).expect("to the release moment");
    assert_eq!(narration(&events), vec!["expired-clean"]);
    assert_eq!(count_named(&e, "ephemeral"), 1);
}
