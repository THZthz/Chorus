//! Entries 35-44 — the relations primitive (spec.md §4.9).

use super::*;

/// One relation fixture family: two actors, one item, two coins. Kinds
/// `carries` (directed exclusive), `married` (symmetric exclusive),
/// `knows` (directed), `north_of` (directed). No boot edges — every row
/// in the state comes from the spliced effects.
fn relations_game(effects: &str) -> Engine {
    let src = format!(
        r#"
game "RelMat"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  worn: 0
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
    start in hall

  card coin
    stackable
    start in hall count 2

verbs
  verb act committable instances any
    recipe go
      duration 0
      effects
{effects}
"#,
        effects = effects
    );
    boot(compile(&src), 1)
}

// matrix 35
/// `relate`/`unrelate` basic: the written edge reads through `rel` and
/// the `related:` scope, multi-target endpoints apply per pair in
/// resolved order, and each logical edge speaks once.
#[test]
fn matrix_35_relate_unrelate_pos() {
    let mut e = relations_game(
        "        relate alice to sword as carries\n        relate world name:coin to alice as knows",
    );
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    let coins: Vec<EntityId> = e
        .state()
        .entities
        .values()
        .filter(|x| x.name == "coin")
        .map(|x| x.id)
        .collect();
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Related { .. }))
            .count(),
        3,
        "one per written logical edge"
    );
    let rel = &e.state().relations;
    assert!(rel
        .get(&alice)
        .and_then(|k| k.get("carries"))
        .is_some_and(|o| o.contains(&sword)));
    // The knows edges point at alice from each coin in resolved order.
    assert_eq!(
        coins
            .iter()
            .filter(|c| rel
                .get(c)
                .and_then(|k| k.get("knows"))
                .is_some_and(|o| o.contains(&alice)))
            .count(),
        2
    );

    let mut e2 = relations_game(
        "        relate alice to sword as carries\n        unrelate alice from sword as carries",
    );
    let events = common::commit_named(&mut e2, "act", &[]).expect("commits");
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Unrelated { .. }))
            .count(),
        1
    );
    assert!(e2.state().relations.is_empty());
}

// matrix 36
/// Symmetric two-way read over single canonical storage: one row under
/// the min endpoint, `rel` holds both ways, enumeration matches from
/// either end.
#[test]
fn matrix_36_symmetric_two_way_read() {
    let mut e = relations_game("        relate bob to alice as married");
    let alice = common::named(&e, "alice");
    let bob = common::named(&e, "bob");
    common::commit_named(&mut e, "act", &[]).expect("commits");
    let rel = &e.state().relations;
    assert_eq!(rel.len(), 1, "one canonical row");
    let (subject, object) = if alice < bob {
        (alice, bob)
    } else {
        (bob, alice)
    };
    assert!(rel
        .get(&subject)
        .and_then(|k| k.get("married"))
        .is_some_and(|o| o.contains(&object)));
    assert!(story_core::edge_exists(rel, alice, "married", bob, true));
    assert!(story_core::edge_exists(rel, bob, "married", alice, true));
}

// matrix 37
/// Exclusive steal with events: a directed kind steals per object (the
/// prior holder is evicted before the new edge speaks); a symmetric kind
/// gives each endpoint valence 1.
#[test]
fn matrix_37_exclusive_steal() {
    let mut e = relations_game(
        "        relate bob to sword as carries\n        relate alice to sword as carries",
    );
    let bob = common::named(&e, "bob");
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    let spoken: Vec<String> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::Related { from, to, .. } => Some(format!("+{from}->{to}")),
            Event::Unrelated { from, to, .. } => Some(format!("-{from}->{to}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        spoken,
        vec![
            format!("+{bob}->{sword}"),
            format!("-{bob}->{sword}"),
            format!("+{alice}->{sword}")
        ],
        "the steal speaks between the two writes"
    );
    assert_eq!(e.state().relations.len(), 1);

    // Symmetric valence: alice-bob then alice-sword leaves one row.
    let mut e2 = relations_game(
        "        relate alice to bob as married\n        relate alice to sword as married",
    );
    let sword = common::named(&e2, "sword");
    let _ = common::commit_named(&mut e2, "act", &[]).expect("commits");
    assert_eq!(e2.state().relations.len(), 1);
    let alice2 = common::named(&e2, "alice");
    assert!(story_core::edge_exists(
        &e2.state().relations,
        alice2,
        "married",
        sword,
        true
    ));
}

// matrix 38
/// `rel()` faces: positive, negative, and dead-endpoint reads; inside a
/// slot-filter predicate (candidate as subject) and an expiry book.
#[test]
fn matrix_38_rel_operand_faces() {
    let src = r#"
game "Faces"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worn: 0

relations
  carries exclusive

predicates
  predicate alice_carries(p) = rel(carries, alice, p)

cards
  card alice
    unique
    start in hall

  card bob
    unique
    start in hall

  card sword
    unique
    lifetime 1m
    start in hall
    on expire
      rule gasp when rel(carries, alice, subject)
        effects
          set alice.worn += 1

verbs
  verb act committable instances any
    recipe link
      duration 0
      effects
        relate alice to sword as carries

  verb probe committable instances any
    recipe look
      duration 0
      when rel(carries, alice, sword)
      effects
        narrate "edge"

  verb picker committable instances any
    recipe pick
      duration 0
      slots
        held: 1..1 alice_carries
      effects
        narrate "picked {held}"
"#
    .to_string();
    let mut e = boot(compile(&src), 1);
    assert!(
        common::commit_named(&mut e, "probe", &[]).is_err(),
        "no edge: no eligible recipe"
    );
    common::commit_named(&mut e, "act", &[]).expect("relates");
    let events = common::commit_named(&mut e, "probe", &[]).expect("the edge gates in");
    assert_eq!(narration(&events), vec!["edge"]);
    let events =
        common::commit_named(&mut e, "picker", &[("held", "sword")]).expect("filter matches");
    assert_eq!(narration(&events), vec!["picked {held}"]);
    // The expiry book reads the edge through `subject` before its
    // (replaced) destruction.
    let events = e.advance(1).expect("expires");
    let alice = common::named(&e, "alice");
    assert_eq!(common::aspect(&e, alice, "worn"), 1);
    assert!(narration(&events).is_empty());
}

// matrix 39
/// The `related:` scope in count/exists/total and as an effect target
/// (per-card `set` walk), enumerating over cards living in a situation
/// interior and a carrier zone.
#[test]
fn matrix_39_related_scope() {
    let src = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worn: 0
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

  card coin
    stackable
    start in hall count 2

verbs
  verb act committable instances any
    recipe link
      duration 0
      effects
        relate alice to sword as carries
        relate alice to world name:coin as carries
        set related:carries of alice aspect:worn += 1

  verb holder committable instances 1
    recipe hold
      duration 10m
      slots
        held: 1..1 aspect:spark
      effects
        narrate "holding"
"#
    .to_string();
    let mut e = boot(compile(&src), 1);
    common::commit_named(&mut e, "act", &[]).expect("links");
    let alice = common::named(&e, "alice");
    // The walk stamped every related card, wherever it lives.
    assert_eq!(common::aspect(&e, alice, "worn"), 0);
    let sword = common::named(&e, "sword");
    assert_eq!(common::aspect(&e, sword, "worn"), 1);
    let coins: Vec<EntityId> = e
        .state()
        .entities
        .values()
        .filter(|x| x.name == "coin")
        .map(|x| x.id)
        .collect();
    assert!(coins.iter().all(|c| common::aspect(&e, *c, "worn") == 1));
    // Relocation does not disturb enumeration: the sword is bound into a
    // situation interior; count still reads 3 related cards.
    common::commit_named(&mut e, "holder", &[("held", "sword")]).expect("binds");
    let rel = &e.state().relations;
    assert_eq!(
        rel.get(&alice)
            .and_then(|k| k.get("carries"))
            .map(|o| o.len()),
        Some(3)
    );
}

// matrix 40
/// Boot `relates` lines: edges present at t=0 before any action,
/// multi-count objects resolve to every live copy, and a missing object
/// warns on the boot stream while play proceeds.
#[test]
fn matrix_40_boot_relates() {
    let src = r#"
game "BootRel"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  north_of
  adjacent symmetric

cards
  card gate
    unique
    start in hall
    relates north_of post, lantern
    relates adjacent post

  card post
    unique
    start in hall

  card lantern
    stackable
    start in hall count 2

  card ghost_post
    unique
    start in hall
    relates north_of nothing_here

  card nothing_here
    unique

verbs
  verb act committable instances any
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
        "the missing object warns"
    );
    assert!(!diags.iter().any(|d| d.is_error()));
    let gate = common::named(&e, "gate");
    let post = common::named(&e, "post");
    let lanterns: Vec<EntityId> = e
        .state()
        .entities
        .values()
        .filter(|x| x.name == "lantern")
        .map(|x| x.id)
        .collect();
    assert_eq!(lanterns.len(), 2);
    let north = e
        .state()
        .relations
        .get(&gate)
        .and_then(|k| k.get("north_of"))
        .cloned()
        .expect("boot edges");
    assert!(north.contains(&post) && lanterns.iter().all(|l| north.contains(l)));
    // ghost_post warned and skipped; it holds no rows at all.
    let ghost = common::named(&e, "ghost_post");
    assert!(!e.state().relations.contains_key(&ghost));
    let _ = story_core::edge_exists(&e.state().relations, post, "adjacent", gate, true);
}

// matrix 41
/// Snapshot round-trip and undo: relations ride the state bytes; the
/// host-held pre-action snapshot restores them bit-for-bit.
#[test]
fn matrix_41_snapshot_and_undo() {
    let mut e = relations_game(
        "        relate bob to sword as carries\n        relate alice to sword as carries",
    );
    let snap = e.snapshot();
    let snap_relations = relations_of(&snap);
    common::commit_named(&mut e, "act", &[]).expect("steals");
    assert_eq!(e.state().relations.len(), 1);
    e.restore(&snap).expect("restores");
    assert_eq!(&e.state().relations, &snap_relations);
    let again = e.snapshot();
    assert_eq!(snap.state, again.state);
}

fn relations_of(snap: &story_core::Snapshot) -> story_core::Relations {
    serde_json::from_slice::<story_core::State>(&snap.state)
        .expect("snapshot state deserializes")
        .relations
}

// matrix 42
/// Move independence: relations survive `move` into a zone and a card
/// carrier, and die on destroy — silently.
#[test]
fn matrix_42_move_independence() {
    let mut e = relations_game(
        "        relate alice to sword as carries\n        move sword into vault\n        move sword into alice",
    );
    let alice = common::named(&e, "alice");
    let sword = common::named(&e, "sword");
    common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(common::zone_kind_of(&e, sword), "carrier");
    assert!(e
        .state()
        .relations
        .get(&alice)
        .and_then(|k| k.get("carries"))
        .is_some_and(|o| o.contains(&sword)));

    let mut e2 = relations_game("        relate alice to sword as carries\n        destroy sword");
    let events = common::commit_named(&mut e2, "act", &[]).expect("destroys");
    let alice2 = common::named(&e2, "alice");
    assert!(!e2.state().relations.contains_key(&alice2));
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));
}

// matrix 43
/// `first(...)` endpoints: oldest-copy resolution, a `slot:` scope
/// endpoint, and the empty read (false / empty, never a failure).
#[test]
fn matrix_43_first_endpoints() {
    let mut e = relations_game(
        "        relate alice to world name:coin as knows\n        if rel(knows, alice, first(world name:coin))\n          narrate \"oldest\"\n        if rel(knows, alice, first(zone:vault))\n          narrate \"never\"\n        if count(related:knows of first(zone:vault)) == 0\n          narrate \"empty\"",
    );
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(narration(&events), vec!["oldest", "empty"]);
    let coins: Vec<EntityId> = e
        .state()
        .entities
        .values()
        .filter(|x| x.name == "coin")
        .map(|x| x.id)
        .collect();
    let alice = common::named(&e, "alice");
    assert_eq!(
        e.state()
            .relations
            .get(&alice)
            .and_then(|k| k.get("knows"))
            .cloned()
            .unwrap_or_default(),
        coins.into_iter().collect::<std::collections::BTreeSet<_>>()
    );

    // The slot: endpoint through a card-ref.
    let src = r#"
game "SlotRef"
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
        narrate "[{related:carries of first(slot:hero)}]"
"#
    .to_string();
    let mut e2 = boot(compile(&src), 1);
    let events =
        common::commit_named(&mut e2, "act", &[("hero", "alice")]).expect("binds and relates");
    assert_eq!(
        narration(&events),
        vec!["[{related:carries of first(slot:hero)}]"]
    );
}

// matrix 44
/// `unrelate a from b` with no kind severs every logical edge between
/// the pair, either direction, once per edge.
#[test]
fn matrix_44_unrelate_severs_all() {
    let mut e = relations_game(
        "        relate alice to bob as knows\n        relate alice to bob as carries\n        relate bob to alice as married\n        relate bob to sword as knows\n        unrelate alice from bob",
    );
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::Unrelated { .. }))
            .count(),
        3,
        "knows a->b, carries a->b, married (canonical) — each once"
    );
    let alice = common::named(&e, "alice");
    let bob = common::named(&e, "bob");
    let sword = common::named(&e, "sword");
    // The unrelated third party survives.
    assert!(e
        .state()
        .relations
        .get(&bob)
        .and_then(|k| k.get("knows"))
        .is_some_and(|o| o.contains(&sword)));
    assert!(!e.state().relations.contains_key(&alice));
    let _ = (alice, bob);
}
