//! Entry 26 — spawn handles.

use super::*;

// matrix 26
/// A handle pool lives for its effect list: multi-mints order the pool
/// oldest-first, targets walk it in that order, branches share it, and
/// prose tokens render it.
#[test]
fn matrix_26_handles_pos() {
    let src = r#"
game "Pool"
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
  verb minter committable instances 1
    recipe work
      duration 0
      effects
        if count(anywhere, aspect:spark) >= 0
          spawn coin count 3 as pile
        narrate "pool {pile}"
        destroy pile count 2
        narrate "left {pile}"
"#;
    let mut e = game(src);
    let next_before = e.state().next_entity;
    let events = common::commit_named(&mut e, "minter", &[]).expect("mints");
    assert_eq!(
        narration(&events),
        vec!["pool coin, coin, coin", "left coin"],
        "the pool renders oldest-first and shrinks as its cards die"
    );
    let minted: Vec<EntityId> = (next_before..next_before + 3).map(EntityId).collect();
    assert_eq!(count_named(&e, "coin"), 1, "two of the pool destroyed");
    assert_eq!(
        ids_of(&e, "coin"),
        vec![minted[2]],
        "the count walked the pool oldest-first"
    );
}

// matrix 26
/// A handle reference with no enclosing `spawn ... as` in the same effect
/// list is refused at compile time — the pool cannot outlive its list.
/// A handle-shaped prose token is refused as [E205] (language.md §9: E238
/// covers handle targets; orphan tokens are the token checker's E205).
#[test]
fn matrix_26_handles_neg() {
    let orphan_target = r#"
game "Pool"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card coin
    stackable

verbs
  verb minter committable instances 1
    recipe work
      duration 0
      effects
        destroy pile
"#;
    assert!(compile_error_codes(orphan_target).contains(&code::E238));
    let orphan_token = orphan_target.replace("destroy pile", "narrate \"{pile}\"");
    assert!(compile_error_codes(&orphan_token).contains(&code::E205));
}
