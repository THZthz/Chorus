//! Severing, destruction, snapshot/undo determinism, and move independence.

use super::*;

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
