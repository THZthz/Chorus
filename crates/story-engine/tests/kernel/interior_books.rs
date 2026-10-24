//! The card-book walk over all interior cards, bound and unbound (kernel-api.md §8).

use super::*;

/// The card-book walk covers ALL interior cards: the bound bindings first
/// in bind order, then unbound interior storage in mint order. A card
/// spawned into the interior during the effects gets its completion book
/// walked like any bound card (kernel-api.md §8).
#[test]
fn unbound_interior_storage_cards_walk_their_books() {
    let src = r#"
game "StorageBooks"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card holder
    stackable
    start in hall
    on complete
      rule announce
        effects
          narrate "holder-book"
  card note
    stackable
    start in hall
    on complete
      rule ping
        effects
          narrate "note-book"

verbs
  verb ritual committable instances 1
    recipe cast
      duration 0
      slots
        h: 1..1
      effects
        spawn note
        narrate "cast"
"#;
    let mut e = game(src);
    let events =
        common::commit_named(&mut e, "ritual", &[("h", "holder")]).expect("instant commit");
    assert_eq!(
        narration(&events),
        vec!["cast", "holder-book", "note-book"],
        "bound cards first in bind order, then unbound interior storage in mint order"
    );
}

/// The aspect-qualified book's second catalyst pass, with its per-stack
/// key constraint: a `on complete [k]` book fires through the pool half
/// when the situation's interior aspect pool contains `k` — bound cards
/// included — AND the walking card's own declaration (element id and
/// aspect keys) carries `k`; a recipe `declares` fires it anywhere. A
/// card that does not itself carry `k` stays silent even when the pool
/// holds it (a stack's candidate triggers are only those declared on its
/// own elements).
#[test]
fn qualified_books_fire_on_the_interior_aspect_pool() {
    let src = r#"
game "PoolGate"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  cred: 0

cards
  card key
    stackable
    aspects cred 1
    start in hall count 2
    on complete [cred]
      rule unlock
        effects
          narrate "key-unlock"
  card blank
    stackable
    start in hall count 2
    on complete [cred]
      rule never
        effects
          narrate "blank-unlock"

verbs
  verb gate committable instances any
    recipe open
      duration 0
      slots
        k: 1..1
        d: 1..1
      effects
        narrate "opened"
"#;
    // The bound key's own `cred` aspect both fills the interior pool and
    // sits on the key card's own declaration: the key's `[cred]` book
    // fires through the pool pass; the blank card carries no `cred` on
    // its own declaration, so its `[cred]` book stays silent — the pool
    // half alone no longer fires a book on a stack that cannot declare
    // the trigger.
    let mut e = game(src);
    let key1 = common::named(&e, "key");
    let blank1 = e
        .state()
        .entities
        .values()
        .find(|c| c.name == "blank")
        .map(|c| c.id)
        .unwrap();
    let events =
        common::commit(&mut e, "gate", &[("k", key1), ("d", blank1)]).expect("instant commit");
    assert_eq!(
        narration(&events),
        vec!["opened", "key-unlock"],
        "the pool holds cred and the key card carries it; the blank card does not, so only its own silence"
    );

    // Without any cred carrier in the interior, the same books stay
    // silent.
    let mut e = game(src);
    let blank2 = common::named(&e, "blank");
    let blank3 = e
        .state()
        .entities
        .values()
        .filter(|c| c.name == "blank")
        .map(|c| c.id)
        .nth(1)
        .unwrap();
    let events = common::commit(&mut e, "gate", &[("k", blank2), ("d", blank3)])
        .expect("instant commit");
    assert_eq!(narration(&events), vec!["opened"], "the pool lacks cred");
}

/// The pass-1 half is conjunct-free: a recipe `declares` fires the
/// `[k]` book on every walking card whose declaration carries `k`,
/// regardless of the pool — and the recipe's own declaration reaches
/// cards whose own keys do not name the catalyst.
#[test]
fn declared_books_fire_without_the_pool() {
    let src = r#"
game "DeclGate"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  cred: 0

cards
  card tome
    stackable
    start in hall
    on complete [cred]
      rule unlock
        effects
          narrate "tome-unlock"

verbs
  verb gate committable instances any
    recipe open
      duration 0
      declares cred
      slots
        t: 1..1
      effects
        narrate "opened"
"#;
    let mut e = game(src);
    let tome = common::named(&e, "tome");
    let events = common::commit(&mut e, "gate", &[("t", tome)]).expect("instant commit");
    assert_eq!(
        narration(&events),
        vec!["opened", "tome-unlock"],
        "the declaring recipe fires the tome's [cred] book with an empty pool"
    );
}
