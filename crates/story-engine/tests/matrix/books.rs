//! Entries 14, 25 — trigger books: signal/on-event and card-carried books.

use super::*;

fn signal_src(book: &str) -> String {
    format!(
        r#"
game "Alarm"
  seed host
  turn 1m
  home hall

zones
  hall

legacies
  favor: 0

cards
  card bell
    stackable

verbs
  verb crier committable instances 1
    recipe cry
      duration 0
      effects
        legacy favor += 50
        signal alarm
        narrate "post"

  verb crier2 committable instances 1
    recipe cry
      duration 0
      effects
        signal alarm
        narrate "post"

{book}
"#
    )
}

const TWO_RULE_BOOK: &str = r#"
on event alarm
  rule whisper priority 1 once when legacy.favor >= 10
    effects
      narrate "whisper"
  rule shout priority 10
    effects
      narrate "shout"
"#;

// matrix 14
/// `signal` ignites the named event book under the selection law: the
/// first rule whose `when` holds fires, `once` spends it for the run,
/// and the signaling list continues after the book.
#[test]
fn matrix_14_signal_books_pos() {
    let src = signal_src(TWO_RULE_BOOK);
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "crier", &[]).expect("cries");
    assert_eq!(narration(&events), vec!["whisper", "post"]);
    // The once rule is spent; the scan falls through to the next rule.
    let events = common::commit_named(&mut e, "crier2", &[]).expect("cries");
    assert_eq!(narration(&events), vec!["shout", "post"]);
    let spent = e.state().fired_once.iter().cloned().collect::<Vec<_>>();
    assert_eq!(spent.len(), 1, "the once rule is marked spent");
}

// matrix 14
/// A book whose every rule misses its `when` stays silent: the signal is
/// not a failure and the signaling list runs on.
#[test]
fn matrix_14_signal_books_neg() {
    let book = r#"
on event alarm
  rule whisper priority 1 when legacy.favor >= 99
    effects
      narrate "whisper"
"#;
    let src = signal_src(book);
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "crier", &[]).expect("cries");
    assert_eq!(narration(&events), vec!["post"], "the book stayed silent");
}

// matrix 25
/// Card-carried completion books: unqualified books fire at every
/// completion, aspect-qualified ones under a matching `declares`, after
/// the recipe's own book, in bind order — including inside an inline
/// instant completion.
#[test]
fn matrix_25_card_books_pos() {
    let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card bard
    stackable
    start in hall
    on complete
      rule song
        effects
          narrate "bard-song"

  card omened
    stackable
    aspects stamped 1
    start in hall
    on complete [stamped]
      rule vision
        effects
          narrate "omened-vision"

verbs
  verb rite_a committable instances 1
    recipe full
      duration 0
      declares stamped
      slots
        f: 1..1
        s: 1..1
      on complete
        rule own
          effects
            narrate "rite-book"

  verb rite_c committable instances 1
    recipe pull
      duration 0
      declares stamped
      slots
        f: 0..1 greedy aspect:stamped
      on complete
        rule own
          effects
            narrate "rite-c-book"

  verb starter committable instances 1
    recipe go
      duration 0
      effects
        start rite_c
"#;
    let mut e = game(src);
    // The whole order lands in one return.
    let events = common::commit_named(&mut e, "rite_a", &[("f", "bard"), ("s", "omened")])
        .expect("completes");
    assert_eq!(
        narration(&events),
        vec!["rite-book", "bard-song", "omened-vision"],
        "recipe book first, card books in bind order, qualified under declares"
    );
    // The inline face: a started instant recipe runs card books inline.
    let events = common::commit_named(&mut e, "starter", &[]).expect("resolves inline");
    assert_eq!(
        narration(&events),
        vec!["rite-c-book", "omened-vision"],
        "the greedy pull bound omened; its book fired inside the inline completion"
    );
    assert_eq!(completed_count(&events), 2, "starter.go and rite_c.pull");
}

// matrix 25
/// An aspect-qualified book stays silent when the recipe does not declare
/// the aspect and the situation's interior pool does not contain it; the
/// completion itself is unaffected.
#[test]
fn matrix_25_card_books_neg() {
    let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card omened
    stackable
    start in hall
    on complete [stamped]
      rule vision
        effects
          narrate "omened-vision"

verbs
  verb rite_b committable instances 1
    recipe bare
      duration 0
      slots
        f: 1..1
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "rite_b", &[("f", "omened")]).expect("completes");
    assert!(
        narration(&events).is_empty(),
        "the qualified book never fires without `declares` or an interior pool holding the aspect"
    );
    assert_eq!(completed_count(&events), 1);
}

// guidance 5b
/// Review guidance for M2, item 5b: a `once` rule's spent mark lives in
/// the state, so a resolution-unit rollback un-spends it — the rule fires
/// again at the next completion.
#[test]
fn once_rule_unspends_when_the_unit_rolls_back() {
    let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

cards
  card bard
    stackable
    start in hall
    on complete
      rule song once
        effects
          narrate "song"

  card jinx
    stackable
    start in hall
    on complete
      rule hex
        effects
          consume aspect:ghost 1

  card muse
    stackable
    start in hall

verbs
  verb rite committable instances 1
    recipe full
      duration 5m
      slots
        f: 1..1
        s: 1..1
"#;
    let mut e = game(src);
    // bard's once book fires, then jinx's book fails the unit: the whole
    // completion rolls back, the spent mark included.
    common::commit_named(&mut e, "rite", &[("f", "bard"), ("s", "jinx")]).expect("opens");
    let events = e.advance(5).expect("the unit fails; the advance continues");
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
    assert!(situations(&e).is_empty(), "the situation aborted");
    for name in ["bard", "jinx", "muse"] {
        assert_eq!(count_named(&e, name), 1, "{name} released, not consumed");
    }
    assert!(
        e.state().fired_once.is_empty(),
        "the once rule un-spent with the rollback"
    );
    // The next completion walks the once rule again.
    common::commit_named(&mut e, "rite", &[("f", "bard"), ("s", "muse")]).expect("opens");
    let events = e.advance(5).expect("completes");
    assert_eq!(narration(&events), vec!["song"]);
    assert_eq!(e.state().fired_once.len(), 1, "spent for real this time");
}
