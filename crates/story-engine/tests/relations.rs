//! The relations primitive (kernel-api.md §10 entries 35-44, spec.md §4.9):
//! edge writes and reads, symmetry, exclusivity, enumeration, boot edges,
//! and the destroy/move/snapshot interactions.

mod common;

use common::{boot, compile, narration};
use story_core::{EntityId, Relations};
use story_engine::{Engine, Event};

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

fn ids_of(engine: &Engine, name: &str) -> Vec<EntityId> {
    engine
        .state()
        .entities
        .values()
        .filter(|e| e.name == name)
        .map(|e| e.id)
        .collect()
}

fn edge_count(relations: &Relations) -> usize {
    relations
        .values()
        .map(|kinds| kinds.values().map(|o| o.len()).sum::<usize>())
        .sum()
}

fn related_of(e: &Engine, subject: EntityId, kind: &str) -> Vec<EntityId> {
    e.state()
        .relations
        .get(&subject)
        .and_then(|kinds| kinds.get(kind))
        .map(|o| o.iter().copied().collect())
        .unwrap_or_default()
}

/// One effect-runner verb: a 0-duration recipe `go` whose effects are spliced
/// in, so a commit named `act` applies them and returns the events.
fn runner(effects: &str) -> String {
    format!(
        r#"
game "Rel"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  worn: 0
  weight: 0
  spark: 1

relations
  carries exclusive
  married symmetric exclusive
  knows
  north_of

cards
  card alice
    unique
    aspects spark 1
    start in hall

  card bob
    unique
    start in hall

  card sword
    unique
    aspects weight 2
    start in hall

  card coin
    stackable
    aspects weight 1
    start in hall count 2

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
{effects}

  verb probe committable instances any
    recipe look
      duration 0
      effects
        narrate "looked"
"#
    )
}

// ---------------------------------------------------------------------------
// Writes and reads
// ---------------------------------------------------------------------------

// matrix 35
/// `relate` writes the edge, `unrelate` removes it; each written or removed
/// logical edge speaks exactly once, and the prose token renders the walk.
#[test]
fn relate_then_unrelate_basic() {
    let src = runner(
        r#"        relate alice to sword as carries
        narrate "carries: [{related:carries of alice}]"
        unrelate alice from sword as carries
        narrate "carries: [{related:carries of alice}]""#,
    );
    let mut e = game(&src);
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        narration(&events),
        vec!["carries: [sword]", "carries: []"],
        "the token renders the walk while the edge lives and after it goes"
    );
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Related { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Unrelated { .. }))
            .count(),
        1
    );
    assert_eq!(edge_count(&e.state().relations), 0, "all edges gone");
    assert_eq!(related_of(&e, alice, "carries"), Vec::<EntityId>::new());
    let _ = sword;
}

// matrix 35
/// Multi-card endpoints address all matches; application runs over every
/// subject x object pair in resolved order, self-pairs skipped.
#[test]
fn relate_multi_targets_apply_per_pair() {
    let src = runner(
        r#"        relate world name:coin to world name:coin as knows
        narrate "[{related:knows of first(world name:coin)}]""#,
    );
    let mut e = game(&src);
    let coins = ids_of(&e, "coin");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Related { .. }))
            .count(),
        2,
        "two subjects x two objects, both self-pairs skipped"
    );
    assert_eq!(edge_count(&e.state().relations), 2);
    // Each coin knows the other and nothing else.
    for c in &coins {
        assert_eq!(related_of(&e, *c, "knows").len(), 1);
    }
    assert_eq!(narration(&events), vec!["[coin]"]);
}

/// Self-pairs are skipped silently: nothing written, nothing emitted.
#[test]
fn self_pair_is_skipped_silently() {
    let src = runner(
        r#"        relate alice to alice as knows
        unrelate alice from alice as knows
        narrate "done""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["done"]);
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Related { .. } | Event::Unrelated { .. })));
    assert_eq!(edge_count(&e.state().relations), 0);
}

/// A no-op unrelate (edge already absent) emits nothing; re-relating an
/// existing edge does not double-speak.
#[test]
fn noops_emit_nothing() {
    let src = runner(
        r#"        relate alice to sword as carries
        relate alice to sword as carries
        unrelate alice from sword as knows
        narrate "done""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["done"]);
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Related { .. }))
            .count(),
        1,
        "the second relate found the edge present"
    );
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));
}

// ---------------------------------------------------------------------------
// Symmetry
// ---------------------------------------------------------------------------

// matrix 36
/// A symmetric kind stores one canonical row (min endpoint as subject) yet
/// `rel` holds either way.
#[test]
fn symmetric_reads_both_ways_from_one_row() {
    let src = runner(
        r#"        relate alice to bob as married
        if rel(married, alice, bob)
          narrate "a->b"
        if rel(married, bob, alice)
          narrate "b->a""#,
    );
    let mut e = game(&src);
    let alice = common::named(&e, "alice");
    let bob = common::named(&e, "bob");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["a->b", "b->a"]);
    let relations = &e.state().relations;
    assert_eq!(edge_count(relations), 1);
    let (subject, object) = if alice < bob {
        (alice, bob)
    } else {
        (bob, alice)
    };
    assert!(relations
        .get(&subject)
        .and_then(|k| k.get("married"))
        .is_some_and(|o| o.contains(&object)));
}

/// A symmetric relate keeps a single row no matter which endpoint is
/// authored as the subject; the reverse spelling adds nothing and speaks
/// nothing.
#[test]
fn symmetric_reverse_relate_is_a_noop() {
    let src = runner(
        "        relate alice to bob as married\n        relate bob to alice as married\n        narrate \"once\"",
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["once"]);
    assert_eq!(edge_count(&e.state().relations), 1);
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Related { .. }))
            .count(),
        1
    );
}

/// `unrelate` clears a symmetric edge from either spelling.
#[test]
fn symmetric_unrelate_either_spelling() {
    for line in [
        "        relate alice to bob as married\n        unrelate alice from bob as married",
        "        relate bob to alice as married\n        unrelate bob from alice as married",
    ] {
        let src = runner(line);
        let mut e = game(&src);
        common::commit_named(&mut e, "act", &[]).expect("commits");
        assert_eq!(edge_count(&e.state().relations), 0);
    }
}

// ---------------------------------------------------------------------------
// Exclusivity
// ---------------------------------------------------------------------------

// matrix 37
/// Directed exclusive steal: re-equipping removes the prior bearer first
/// (one `Unrelated` per victim, stored orientation) then speaks the
/// `Related`.
#[test]
fn exclusive_directed_steal() {
    let src = runner(
        r#"        relate bob to sword as carries
        relate alice to sword as carries
        if count(related:carries to sword) == 1
          narrate "one bearer""#,
    );
    let mut e = game(&src);
    let bob = common::named(&e, "bob");
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    let mut related = events.iter().filter_map(|ev| match ev {
        Event::Related { from, to, .. } => Some((*from, *to)),
        _ => None,
    });
    let mut unrelated = events.iter().filter_map(|ev| match ev {
        Event::Unrelated { from, to, .. } => Some((*from, *to)),
        _ => None,
    });
    assert_eq!(related.next(), Some((bob, sword)));
    // The steal speaks before the new edge.
    assert_eq!(unrelated.next(), Some((bob, sword)));
    assert_eq!(related.next(), Some((alice, sword)));
    assert!(related.next().is_none() && unrelated.next().is_none());
    assert_eq!(narration(&events), vec!["one bearer"]);
    assert_eq!(edge_count(&e.state().relations), 1);
    assert_eq!(related_of(&e, alice, "carries"), vec![sword]);
    assert!(related_of(&e, bob, "carries").is_empty());
}

// matrix 37
/// Symmetric exclusive valence: a new partnership steals every existing edge
/// of that kind touching either endpoint.
#[test]
fn exclusive_symmetric_valence_steal() {
    let src =
        runner("        relate alice to bob as married\n        relate bob to sword as married");
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    let relations = &e.state().relations;
    assert_eq!(
        edge_count(relations),
        1,
        "alice-bob was stolen by bob-sword"
    );
    let sword = common::named(&e, "sword");
    let bob = common::named(&e, "bob");
    let alice = common::named(&e, "alice");
    assert!(
        story_core::edge_exists(relations, bob, "married", sword, true),
        "bob married sword"
    );
    assert!(
        !story_core::edge_exists(relations, alice, "married", bob, true),
        "alice divorced"
    );
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Unrelated { .. }))
            .count(),
        1,
        "the stolen edge speaks once"
    );
}

/// Non-exclusive kinds accumulate holders freely.
#[test]
fn non_exclusive_kinds_accumulate() {
    let src = runner("        relate alice to bob as knows\n        relate sword to bob as knows");
    let mut e = game(&src);
    common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(edge_count(&e.state().relations), 2);
}

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
// Severing and destruction
// ---------------------------------------------------------------------------

// matrix 44
/// `unrelate a from b` without a kind severs every logical edge between the
/// pair, either direction, once per edge.
#[test]
fn unrelate_without_kind_severs_all_kinds() {
    let src = runner(
        r#"        relate alice to bob as knows
        relate alice to bob as carries
        relate bob to alice as knows
        unrelate alice from bob
        if count(related:knows of alice) == 0
          narrate "n0"
        if count(related:carries of alice) == 0
          narrate "c0"
        if count(related:knows of bob) == 0
          narrate "m0""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["n0", "c0", "m0"]);
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Unrelated { .. }))
            .count(),
        3,
        "one Unrelated per removed logical edge"
    );
    assert_eq!(edge_count(&e.state().relations), 0);
}

// matrix 42 (destroy half), T19
/// Every destruction path drops the dying card's edges silently: destroy,
/// consume, swap, and default expiry. Survivors keep their own edges.
#[test]
fn destruction_drops_edges_silently() {
    // destroy
    let src = runner(
        r#"        relate alice to sword as carries
        relate alice to bob as knows
        destroy sword
        if count(related:knows of alice) == 1
          narrate "n1"
        if count(related:carries of alice) == 0
          narrate "c0""#,
    );
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["n1", "c0"]);
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));

    // consume: the surviving coin keeps its edge
    let src = runner(
        r#"        relate alice to world name:coin as carries
        consume world name:coin 1
        if count(related:carries of alice) == 1
          narrate "kept""#,
    );
    let mut e2 = game(&src);
    let events = common::commit_named(&mut e2, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["kept"]);
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));

    // swap: the successor starts edgeless
    let src = runner(
        r#"        relate alice to sword as carries
        swap sword to coin
        if count(related:carries of alice) == 0
          narrate "edgeless""#,
    );
    let mut e3 = game(&src);
    let events = common::commit_named(&mut e3, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["edgeless"]);
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));

    // default expiry destruction
    let src = r#"
game "Decay"
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
    lifetime 1m
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        relate alice to sword as carries
"#
    .to_string();
    let mut e4 = game(&src);
    common::commit_named(&mut e4, "act", &[]).expect("relates");
    let events = e4.advance(1).expect("expires");
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));
    let alice = common::named(&e4, "alice");
    assert!(related_of(&e4, alice, "carries").is_empty());
}

/// An edge whose endpoint dies leaves no dangling rows: every stored subject
/// and object is a live entity.
#[test]
fn destroyed_endpoints_leave_no_dangling_rows() {
    let src = runner(
        r#"        relate alice to sword as carries
        destroy sword
        narrate "done""#,
    );
    let mut e = game(&src);
    common::commit_named(&mut e, "act", &[]).expect("commits");
    for (subject, kinds) in &e.state().relations {
        assert!(
            e.state().entities.contains_key(subject),
            "dangling subject {subject}"
        );
        for (kind, objects) in kinds {
            for o in objects {
                let _ = kind;
                assert!(
                    e.state().entities.contains_key(o),
                    "dangling object {o} under {subject}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Boot edges
// ---------------------------------------------------------------------------

// matrix 40
/// Boot `relates` lines: edges present at t=0, objects resolve to every
/// live same-name entity, symmetric kinds store one canonical row, and boot
/// emits no events.
#[test]
fn boot_relates_edges() {
    let src = r#"
game "Boot"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  north_of
  adjacent symmetric

cards
  card hall_gate
    unique
    start in hall
    relates north_of tower, cabin
    relates adjacent tower

  card tower
    unique
    start in hall

  card cabin
    stackable
    start in hall count 2

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        narrate "warm"
"#
    .to_string();
    let mut e = game(&src);
    let gate = common::named(&e, "hall_gate");
    let tower = common::named(&e, "tower");
    let cabins = ids_of(&e, "cabin");
    assert_eq!(cabins.len(), 2);
    let north = related_of(&e, gate, "north_of");
    assert!(north.contains(&tower) && cabins.iter().all(|c| north.contains(c)));
    assert_eq!(related_of(&e, gate, "adjacent"), vec![tower]);
    // Symmetric read holds the other way from one canonical row.
    assert!(story_core::edge_exists(
        &e.state().relations,
        tower,
        "adjacent",
        gate,
        true
    ));
    assert_eq!(edge_count(&e.state().relations), 4);
    // A commit is the first observable traffic; boot itself spoke nothing.
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["warm"]);
}

// matrix 40
/// A boot `relates` object with no live instance warns on the boot stream
/// and skips; play proceeds.
#[test]
fn boot_relates_missing_object_warns() {
    let src = r#"
game "BootWarn"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  north_of

cards
  card hall_gate
    unique
    start in hall
    relates north_of ghost_tower

  card ghost_tower
    unique

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
        narrate "warm"
"#
    .to_string();
    let content = compile(&src);
    let (e, diags) =
        story_engine::Engine::boot(content, story_engine::EffectRegistry::new(), 1, None);
    assert!(
        diags
            .iter()
            .any(|d| !d.is_error() && d.kind.word() == "UnresolvableRef"),
        "a warning names the missing object: {diags:?}"
    );
    assert!(!diags.iter().any(|d| d.is_error()), "play proceeds");
    let gate = common::named(&e, "hall_gate");
    assert!(!e.state().relations.contains_key(&gate));
}

// ---------------------------------------------------------------------------
// Snapshot, undo, determinism
// ---------------------------------------------------------------------------

// matrix 41, T18
/// Relations ride the snapshot: round-trip is bit-for-bit, and the host undo
/// stack restores them after a steal and after a destruction.
#[test]
fn snapshot_roundtrip_and_undo() {
    let src = runner(
        r#"        relate bob to sword as carries
        relate alice to sword as carries
        narrate "stolen""#,
    );
    let mut e = game(&src);
    let before = e.snapshot();
    let before_relations = before_state_relations(&before);
    common::commit_named(&mut e, "act", &[]).expect("steals");
    assert_eq!(edge_count(&e.state().relations), 1);
    e.restore(&before).expect("restores");
    assert_eq!(
        &e.state().relations,
        &before_relations,
        "restored relations match the pre-action snapshot"
    );
    let after = e.snapshot();
    assert_eq!(before.state, after.state, "bit-for-bit undo");

    // Undo after a destruction: the edge pre-death comes back.
    let destroy_fixture = r#"
game "Undo2"
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
  verb relate_to committable instances any
    recipe link
      duration 0
      effects
        relate alice to sword as carries

  verb breaker committable instances any
    recipe smash
      duration 0
      effects
        destroy sword
"#
    .to_string();
    let mut e5 = game(&destroy_fixture);
    common::commit_named(&mut e5, "relate_to", &[]).expect("relates");
    let snap5 = e5.snapshot();
    common::commit_named(&mut e5, "breaker", &[]).expect("destroys");
    assert_eq!(edge_count(&e5.state().relations), 0);
    e5.restore(&snap5).expect("restores");
    let alice = common::named(&e5, "alice");
    assert_eq!(
        related_of(&e5, alice, "carries").len(),
        1,
        "the edge comes back"
    );
}

fn before_state_relations(snap: &story_core::Snapshot) -> Relations {
    serde_json::from_slice::<story_core::State>(&snap.state)
        .expect("snapshot state deserializes")
        .relations
}

/// Same content + seed + operations produce identical relation storage
/// (canonical orientation included).
#[test]
fn relations_are_deterministic() {
    let src = runner(
        r#"        relate alice to world name:coin as knows
        relate bob to sword as carries
        narrate "ok""#,
    );
    let a = {
        let mut e = game(&src);
        common::commit_named(&mut e, "act", &[]).expect("commits");
        common::state_bytes(&e)
    };
    let b = {
        let mut e = game(&src);
        common::commit_named(&mut e, "act", &[]).expect("commits");
        common::state_bytes(&e)
    };
    assert_eq!(a, b);
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

// ---------------------------------------------------------------------------
// Movement independence
// ---------------------------------------------------------------------------

// matrix 42 (move half)
/// `move` never touches edges: a walking card keeps its relations; carrier
/// relocation included.
#[test]
fn move_never_touches_edges() {
    let src = r#"
game "Walk"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

relations
  carries exclusive
  married symmetric

cards
  card alice
    unique
    start in hall

  card bob
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
        relate alice to bob as married
        move sword into vault
        move sword into alice
        move alice into vault
        narrate "moved"

  verb probe committable instances any
    recipe look
      duration 0
      effects
        narrate "looked"
"#
    .to_string();
    let mut e = game(&src);
    let alice = common::named(&e, "alice");
    let bob = common::named(&e, "bob");
    let sword = common::named(&e, "sword");
    common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(edge_count(&e.state().relations), 2);
    assert_eq!(related_of(&e, alice, "carries"), vec![sword]);
    assert!(story_core::edge_exists(
        &e.state().relations,
        alice,
        "married",
        bob,
        true
    ));
    // The carrier relocation landed the sword inside alice, edges intact.
    assert_eq!(common::zone_kind_of(&e, sword), "carrier");
}
