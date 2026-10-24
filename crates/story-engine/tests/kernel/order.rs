//! Completion order (T12): the recipe book first, then card books in bind order.

use super::*;

const SRC: &str = r#"
game "Order"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card alpha
    stackable
    start in hall
    on complete
      rule announce
        effects
          narrate "card-alpha"

  card beta
    stackable
    start in hall
    on complete [stamped]
      rule qualified
        effects
          narrate "card-beta-qualified"
    on complete
      rule plain
        effects
          narrate "card-beta-plain"

  card gamma
    stackable
    start in hall

verbs
  verb ritual committable instances 1
    recipe cast
      duration 0
      declares stamped
      slots
        focus: 1..1
        second: 1..1
      effects
        narrate "effects-first"
      on complete
        rule book
          effects
            narrate "recipe-book"
"#;

/// T12: the recipe's book fires before the card books; card books are
/// deduplicated across slots and ordered by first bind; the
/// aspect-qualified book fires only under a matching `declares`; an
/// instant completion keeps the whole order inside the one commit.
#[test]
fn completion_order_recipe_book_then_card_books_in_bind_order() {
    let mut e = game(SRC);
    let alpha = common::named(&e, "alpha");
    let beta = common::named(&e, "beta");
    let gamma = common::named(&e, "gamma");

    // alpha first, beta second: alpha's book walks before beta's.
    let events = common::commit(&mut e, "ritual", &[("focus", alpha), ("second", beta)])
        .expect("instant commit");
    assert_eq!(
        narration(&events),
        vec![
            "effects-first",
            "recipe-book",
            "card-alpha",
            "card-beta-qualified",
            "card-beta-plain",
        ],
        "recipe book first, then card books in bind order"
    );
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));

    // Binding beta first swaps the card-book order.
    let events = common::commit(&mut e, "ritual", &[("focus", beta), ("second", alpha)])
        .expect("instant commit");
    assert_eq!(
        narration(&events),
        vec![
            "effects-first",
            "recipe-book",
            "card-beta-qualified",
            "card-beta-plain",
            "card-alpha",
        ]
    );

    // The same card in both slots walks once (deduplicated).
    let events = common::commit(&mut e, "ritual", &[("focus", alpha), ("second", alpha)])
        .expect("instant commit");
    assert_eq!(
        narration(&events),
        vec!["effects-first", "recipe-book", "card-alpha"]
    );

    // A card without a book stays silent; the rest of the order holds.
    let events = common::commit(&mut e, "ritual", &[("focus", gamma), ("second", alpha)])
        .expect("instant commit");
    assert_eq!(
        narration(&events),
        vec!["effects-first", "recipe-book", "card-alpha"]
    );
}
