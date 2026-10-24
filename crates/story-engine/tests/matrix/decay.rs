//! Entries 6, 10, 17 — decay and decks: on-expire books, fallback exhaustion, wrap cycling.

use super::*;

fn expire_src(fail_book: bool) -> String {
    let book = if fail_book {
        "    on expire\n      rule flutter\n        effects\n          consume aspect:ghost 1\n          set subject.spark += 1\n"
    } else {
        "    on expire\n      rule flutter\n        effects\n          set subject.spark += 1\n          narrate \"fluttered\"\n"
    };
    format!(
        r#"
game "Moth"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  ghost: 0

cards
  card moth
    lifetime 5m
    aspects spark 0
    start in hall
{book}
verbs
  verb watcher committable instances 1
    recipe stare
      duration 30m
      effects
        narrate "staring"
"#
    )
}

// matrix 06
/// An `on expire` book replaces default destruction, addresses the dying
/// card as `subject`, and spends its one-shot timer.
#[test]
fn matrix_06_on_expire_pos() {
    let src = expire_src(false);
    let mut e = game(&src);
    let events = e.advance(5).expect("to the due moment");
    assert_eq!(narration(&events), vec!["fluttered"]);
    let moth = common::named(&e, "moth");
    assert_eq!(count_named(&e, "moth"), 1, "the book replaced destruction");
    assert_eq!(aspect(&e, moth, "spark"), 1, "subject writes landed");
    assert_eq!(e.state().entities[&moth].expires_at, None, "one-shot");
    let events = e.advance(5).expect("quiet");
    assert!(events.is_empty(), "the timer does not re-arm");
}

// matrix 06
/// A failing expiry book rolls the unit back silently but the timer is
/// spent: the card survives the cycle with `expires_at` cleared and
/// never re-fires (the reviewer's implemented judgment call 5a).
#[test]
fn matrix_06_on_expire_neg_failing_book_spends_the_timer() {
    let src = expire_src(true);
    let mut e = game(&src);
    let moth = common::named(&e, "moth");
    let events = e.advance(5).expect("to the due moment");
    assert!(
        events.is_empty(),
        "an expiry unit carries no situation, so its failure is silent: {events:?}"
    );
    assert_eq!(count_named(&e, "moth"), 1, "rolled back, still alive");
    assert_eq!(aspect(&e, moth, "spark"), 0, "the writes rolled back");
    assert_eq!(
        e.state().entities[&moth].expires_at,
        None,
        "the timer is spent regardless"
    );
    let events = e.advance(5).expect("quiet");
    assert!(events.is_empty(), "no re-fire: expiry is one-shot");
    assert_eq!(count_named(&e, "moth"), 1);
}

// matrix 10
/// A `fallback` deck serves the relief card for every draw past the
/// permutation, however many.
#[test]
fn matrix_10_fallback_pos() {
    let src = r#"
game "Deals"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card aa
    stackable

  card bb
    stackable

  card cc
    stackable

decks
  deck small
    entries aa, bb
    fallback cc

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from small
        draw from small
        draw from small
        draw from small
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "dreamer", &[]).expect("deals");
    assert_eq!(count_named(&e, "aa"), 1);
    assert_eq!(count_named(&e, "bb"), 1);
    assert_eq!(count_named(&e, "cc"), 2, "the fallback served twice");
    assert_eq!(
        *e.state().decks.values().next().expect("the deck exists"),
        4,
        "cursor"
    );
}

// matrix 10
/// A deck declaring neither relief path raises the exhaustion failure on
/// the first draw past its entries; boot already warned about it. The
/// unit rolls back — cursors included — and the commit refuses.
#[test]
fn matrix_10_fallback_neg() {
    let src = r#"
game "Deals"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card aa
    stackable

  card bb
    stackable

decks
  deck bare
    entries aa, bb

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from bare
        draw from bare
        draw from bare
"#;
    let content = compile(src);
    let (mut e, diags) = Engine::boot(content, story_engine::EffectRegistry::new(), 1, None);
    assert!(
        diags.iter().any(|d| d.kind == DiagKind::DeckExhausted),
        "boot warns about a deck with no relief path: {diags:?}"
    );
    let before = state_bytes(&e);
    assert_eq!(
        err_kind(common::commit_named(&mut e, "dreamer", &[])),
        DiagKind::DeckExhausted
    );
    assert_eq!(state_bytes(&e), before, "the deck cursor rolled back");
    assert_eq!(count_named(&e, "aa"), 0);
    assert_eq!(count_named(&e, "bb"), 0);
}

// matrix 17
/// A `wrap` deck cycles its permutation: the deal sequence repeats with
/// the permutation's period.
#[test]
fn matrix_17_wrap_cycling_pos() {
    let src = r#"
game "Tides"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card ebb
    stackable

  card flood
    stackable

decks
  deck tides
    entries ebb, flood
    wrap

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from tides
        draw from tides
        draw from tides
        draw from tides
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "dreamer", &[]).expect("deals");
    assert_eq!(count_named(&e, "ebb"), 2, "the permutation cycled");
    assert_eq!(count_named(&e, "flood"), 2);
    assert_eq!(
        *e.state().decks.values().next().expect("the deck exists"),
        4
    );
}

// matrix 17
/// A declared `fallback` outranks `wrap`: the relief card serves forever
/// and the permutation never cycles.
#[test]
fn matrix_17_wrap_cycling_neg() {
    let src = r#"
game "Tides"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card ebb
    stackable

  card flood
    stackable

  card calm
    stackable

decks
  deck tides
    entries ebb, flood
    fallback calm
    wrap

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from tides
        draw from tides
        draw from tides
        draw from tides
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "dreamer", &[]).expect("deals");
    assert_eq!(count_named(&e, "ebb"), 1);
    assert_eq!(count_named(&e, "flood"), 1);
    assert_eq!(count_named(&e, "calm"), 2, "the fallback outranks wrap");
}
