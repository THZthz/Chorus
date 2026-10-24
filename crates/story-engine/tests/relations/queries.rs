//! Relation reads: rel operands, related: scopes, first() endpoints, and prose tokens.

use super::*;

// ---------------------------------------------------------------------------
// rel(...) and related: queries
// ---------------------------------------------------------------------------

// matrix 38
/// `rel(...)` in a recipe `when`: the edge a previous action wrote gates a
/// later recipe; without the edge no recipe is eligible.
#[test]
fn rel_operand_when_faces() {
    let src = r#"
game "Gate"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

verbs
  verb act committable instances any
    recipe relate
      duration 0
      effects
        relate alice to sword as carries

  verb probe committable instances any
    recipe look
      duration 0
      when rel(carries, alice, sword)
      effects
        narrate "the edge holds"
"#
    .to_string();
    let mut e = game(&src);
    // The negative face: without the edge the probe has no eligible recipe.
    assert!(common::commit_named(&mut e, "probe", &[]).is_err());
    common::commit_named(&mut e, "act", &[]).expect("relates");
    // The positive face: the edge holds.
    let events = common::commit_named(&mut e, "probe", &[]).expect("the edge gates the probe in");
    assert_eq!(narration(&events), vec!["the edge holds"]);
    // A dead endpoint reads false: destroy one endpoint, the gate closes.
    common::commit_named(&mut e, "probe", &[]).expect("warm");
    let src_unrelate = src.replace(
        "        relate alice to sword as carries",
        "        unrelate alice from sword as carries",
    );
    let mut e2 = game(&src_unrelate);
    common::commit_named(&mut e2, "act", &[]).expect("unrelates");
    assert!(common::commit_named(&mut e2, "probe", &[]).is_err());
}

// matrix 38
/// `rel(...)` inside a slot-filter predicate: the candidate arrives as the
/// endpoint through the subject path.
#[test]
fn rel_in_slot_filter_predicate() {
    let src = r#"
game "Filter"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  carries exclusive

predicates
  predicate alice_carries(p) = rel(carries, alice, p)

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

  card stone
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        relate alice to sword as carries

  verb picker committable instances 1
    recipe pick
      duration 0
      slots
        held: 1..1 alice_carries
      effects
        narrate "picked {held}"
"#
    .to_string();
    let mut e = game(&src);
    common::commit_named(&mut e, "act", &[]).expect("relates");
    let events =
        common::commit_named(&mut e, "picker", &[("held", "sword")]).expect("the sword matches");
    assert_eq!(narration(&events), vec!["picked sword"]);
    assert!(common::commit_named(&mut e, "picker", &[("held", "stone")]).is_err());
}

// matrix 38
/// A predicate parameter read only through a `related:` scope card-ref is a
/// card reference: `holds_any(alice)` compiles and routes the `when` gate
/// (closed with no edge, open once the edge exists).
#[test]
fn scope_only_card_parameter_compiles_and_routes() {
    let src = r#"
game "ScopeParam"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  carries exclusive

predicates
  predicate holds_any(p) = count(related:carries of p) >= 1

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        relate alice to sword as carries

  verb probe committable instances any
    recipe look
      duration 0
      when holds_any(alice)
      effects
        narrate "alice holds something"
"#
    .to_string();
    let mut e = game(&src);
    // The negative face: no edge, the gate keeps the probe out.
    assert!(common::commit_named(&mut e, "probe", &[]).is_err());
    common::commit_named(&mut e, "act", &[]).expect("relates");
    // The positive face: the edge opens the gate.
    let events = common::commit_named(&mut e, "probe", &[]).expect("the gate routes in");
    assert_eq!(narration(&events), vec!["alice holds something"]);
}

// matrix 38
/// `rel(..., subject, ...)` inside an expiry book: the dying card reads as
/// the endpoint before its (replaced) destruction.
#[test]
fn rel_with_subject_in_expiry_book() {
    let src = r#"
game "Expiry"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worn: 0

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    lifetime 1m
    start in hall
    on expire
      rule last_gasp when rel(carries, alice, subject)
        effects
          set alice.worn += 1

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        relate alice to sword as carries
"#
    .to_string();
    let mut e = game(&src);
    common::commit_named(&mut e, "act", &[]).expect("relates");
    let events = e.advance(1).expect("expires");
    assert!(narration(&events).is_empty());
    let alice = common::named(&e, "alice");
    assert_eq!(common::aspect(&e, alice, "worn"), 1, "the edge read alive");
    // The book replaced default destruction; an uncarried copy would not
    // fire. (A second sword-less run shows the contrast.)
    let src2 = src.replace(
        "        relate alice to sword as carries",
        "        narrate \"idle\"",
    );
    let mut e2 = game(&src2);
    let _ = e2.advance(1).expect("expires");
    assert_eq!(
        common::aspect(&e2, common::named(&e2, "alice"), "worn"),
        0,
        "no edge, no last gasp"
    );
}

// matrix 39
/// The `related:` scope through `count` / existence / `total`, forward and
/// incoming.
#[test]
fn related_scope_in_count_exists_total() {
    let src = runner(
        r#"        relate alice to sword as carries
        relate alice to world name:coin as carries
        if count(related:carries of alice) == 3
          narrate "c3"
        if count(related:carries of alice, name:sword) >= 1
          narrate "e"
        if total(related:carries of alice, aspect:weight) == 4
          narrate "t4"
        if count(related:carries to sword) == 1
          narrate "i1"
        if count(related:carries of bob) == 0
          narrate "empty0""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["c3", "e", "t4", "i1", "empty0"]);
}

// matrix 39
/// The `related:` scope as an effect target: the per-card walk writes once
/// per related card.
#[test]
fn related_scope_as_effect_target_walk() {
    let src = runner(
        r#"        relate alice to sword as carries
        relate alice to world name:coin as carries
        set related:carries of alice aspect:worn += 1
        if total(anywhere, aspect:worn) == 3
          narrate "three cards stamped"
        if sword.worn == 1
          narrate "sword stamped""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        narration(&events),
        vec!["three cards stamped", "sword stamped"]
    );
}

/// Enumeration is location-transparent: a related card bound inside a
/// situation interior still enumerates, and the edge survives the move.
#[test]
fn related_scope_is_location_transparent() {
    let src = r#"
game "Move"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 1

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        relate alice to sword as carries

  verb holder committable instances 1
    recipe hold
      duration 10m
      slots
        held: 1..1 aspect:spark
      effects
        narrate "holding"
"#
    .to_string();
    let mut e = game(&src);
    common::commit_named(&mut e, "act", &[]).expect("relates");
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    common::commit_named(&mut e, "holder", &[("held", "sword")]).expect("binds");
    assert_eq!(
        related_of(&e, alice, "carries"),
        vec![sword],
        "the edge ignores the binding's interior"
    );
}

// ---------------------------------------------------------------------------
// Incoming enumeration and first() endpoints
// ---------------------------------------------------------------------------

/// `related:<kind> of X` enumerates the stored direction; `related:<kind>
/// to X` answers the reverse question over directed kinds.
#[test]
fn incoming_enumeration_answers_reverse_question() {
    let src = runner(
        r#"        relate alice to sword as north_of
        relate bob to alice as north_of
        if count(related:north_of of alice) == 1
          narrate "out1"
        if count(related:north_of to alice) == 1
          narrate "in1"
        if count(related:north_of to sword) == 1
          narrate "rev1""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["out1", "in1", "rev1"]);
}

// matrix 43
/// `first(...)` endpoints: the oldest copy in scope order, and the empty
/// read.
#[test]
fn first_endpoints_resolve_in_scope_order() {
    let src = runner(
        r#"        relate alice to world name:coin as knows
        if rel(knows, alice, first(world name:coin))
          narrate "oldest coin holds the edge"
        if rel(knows, first(world name:coin), alice)
          narrate "reverse reads false (directed kind)"
        narrate "done""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        narration(&events),
        vec!["oldest coin holds the edge", "done"]
    );
}

/// A `first(...)` that resolves to nothing is a dead endpoint: `rel` reads
/// false and the enumeration reads empty.
#[test]
fn first_empty_endpoint_reads_dead() {
    let src = runner(
        r#"        relate alice to zone:vault as knows
        if rel(knows, alice, first(zone:vault name:sword))
          narrate "never"
        if count(related:knows of first(zone:vault)) == 0
          narrate "c0"
        narrate "done""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["c0", "done"]);
    assert_eq!(edge_count(&e.state().relations), 0);
}

/// A `slot:` scope endpoint addresses the bound card.
#[test]
fn first_slot_scope_endpoint() {
    let src = r#"
game "SlotEnd"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      slots
        hero: 1..1 name:alice
      effects
        relate alice to sword as carries
        narrate "carries: [{related:carries of first(slot:hero)}]"
"#
    .to_string();
    let mut e = game(&src);
    let events =
        common::commit_named(&mut e, "act", &[("hero", "alice")]).expect("commits with binding");
    assert_eq!(narration(&events), vec!["carries: [sword]"]);
}

// ---------------------------------------------------------------------------
// Prose
// ---------------------------------------------------------------------------

/// `{related:<kind> of X}` renders the forward walk; `{related:<kind> to X}`
/// renders the reverse walk; an empty set renders the empty string.
#[test]
fn related_token_forward_reverse_and_empty() {
    let src = r#"
game "Prose"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  north_of

cards
  card gate
    unique
    start in hall
    relates north_of tower, cabin

  card tower
    unique
    start in hall

  card cabin
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        narrate "forward: {related:north_of of gate}"
        narrate "reverse: {related:north_of to tower}"
        narrate "empty: [{related:north_of of cabin}]"
"#
    .to_string();
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        narration(&events),
        vec!["forward: tower, cabin", "reverse: gate", "empty: []"]
    );
}
