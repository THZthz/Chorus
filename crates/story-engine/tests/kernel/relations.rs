//! Invariants T17-T21 (kernel-api.md §9): the relations primitive.

use super::*;

fn relations_game(effects: &str) -> Engine {
    let src = format!(
        r#"
game "RelKernel"
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
    lifetime 1m
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

  verb probe committable instances any
    recipe look
      duration 0
      effects
        narrate "looked"
"#,
        effects = effects
    );
    boot(compile(&src), 1)
}

fn edges(e: &Engine) -> Vec<(EntityId, String, EntityId)> {
    let mut out = Vec::new();
    for (subject, kinds) in &e.state().relations {
        for (kind, objects) in kinds {
            for object in objects {
                out.push((*subject, kind.clone(), *object));
            }
        }
    }
    out
}

// t17 — relation totality: `rel(...)` reads false and `related:` scopes
// read empty for dead or absent endpoints (an unresolved `first(...)`
// included), in every book context — the context-free event book face
// here; the expiry-book face lives in matrix_38 and the in-context faces
// in relations::first_empty_endpoint_reads_dead.
#[test]
fn t17_relation_reads_are_total() {
    let src = r#"
game "Total17"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  worn: 0

relations
  carries exclusive
  knows

cards
  card alice
    unique
    start in hall

  card ghost
    unique

verbs
  verb act committable instances any
    recipe go
      duration 0
      effects
        relate alice to alice as knows
        signal check

on event check
  rule probe
    effects
      if rel(carries, alice, ghost)
        narrate "never1"
      if rel(carries, ghost, alice)
        narrate "never2"
      if count(related:carries of ghost) == 0
        narrate "empty"
      if total(related:carries of ghost, aspect:worn) == 0
        narrate "zero"
      if rel(carries, alice, first(zone:vault))
        narrate "never3"
      narrate "done"
"#
    .to_string();
    let mut e = game(&src);
    let events = common::commit_named(&mut e, "act", &[]).expect("the signal fires");
    assert_eq!(
        narration(&events),
        vec!["empty", "zero", "done"],
        "every dead-endpoint read answered empty or false in a context-free book"
    );
}

// t18 — relation persistence: same content + seed + operations give
// identical relation storage; snapshot -> restore round-trips; the host
// undo stack restores bit-for-bit. The determinism face is owned by
// relations::relations_are_deterministic and matrix_41; the undo faces
// by relations::snapshot_roundtrip_and_undo (steal and destroy).
#[test]
fn t18_relations_persist_through_snapshot_restore_undo() {
    let mut e = relations_game(
        "        relate alice to sword as carries\n        relate bob to sword as carries",
    );
    let snap = e.snapshot();
    let snap_state: story_core::State =
        serde_json::from_slice(&snap.state).expect("snapshot state");
    common::commit_named(&mut e, "act", &[]).expect("steals");
    assert_eq!(edges(&e).len(), 1, "the steal left one edge");
    e.restore(&snap).expect("restores");
    assert_eq!(e.state().relations, snap_state.relations);

    // Determinism face: a second run over the same ops is byte-identical.
    let mut e2 = relations_game(
        "        relate alice to sword as carries\n        relate bob to sword as carries",
    );
    common::commit_named(&mut e2, "act", &[]).expect("steals");
    e.restore(&snap).expect("restores");
    common::commit_named(&mut e, "act", &[]).expect("steals again");
    assert_eq!(state_bytes(&e), state_bytes(&e2));
}

// t19 — destroy drops edges: every death path (destroy, consume, swap,
// default expiry) removes every logical edge touching the dying card
// silently; the survivor's other edges survive. Owned by
// relations::destruction_drops_edges_silently; the no-dangling-rows face
// by relations::destroyed_endpoints_leave_no_dangling_rows.
#[test]
fn t19_destroy_drops_edges_silently() {
    // Default expiry: the sword's lifetime is 1m.
    let mut e = relations_game(
        "        relate alice to sword as carries\n        relate alice to bob as knows",
    );
    common::commit_named(&mut e, "act", &[]).expect("relates");
    let events = e.advance(1).expect("expires");
    assert!(!events
        .iter()
        .any(|ev| matches!(ev, Event::Unrelated { .. })));
    let alice = common::named(&e, "alice");
    let knows = e
        .state()
        .relations
        .get(&alice)
        .and_then(|k| k.get("knows"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(knows.len(), 1, "the survivor keeps its own edges");
    assert!(e
        .state()
        .relations
        .get(&alice)
        .and_then(|k| k.get("carries"))
        .is_none());
}

// t20 — symmetric and exclusive maintenance: a symmetric relate stores
// one canonical row and reads both ways; an exclusive directed relate
// evicts prior holders (Unrelated per victim, before the Related); a
// symmetric exclusive relate steals prior partners of either endpoint;
// non-exclusive kinds accumulate. Owned by
// relations::{symmetric_reads_both_ways_from_one_row,
// symmetric_reverse_relate_is_a_noop, exclusive_directed_steal,
// exclusive_symmetric_valence_steal, non_exclusive_kinds_accumulate} and
// matrix_36 / matrix_37.
#[test]
fn t20_symmetric_and_exclusive_maintenance() {
    // One canonical row for a symmetric pair; the reverse spelling adds
    // nothing.
    let mut e = relations_game(
        "        relate alice to bob as married\n        relate bob to alice as married",
    );
    common::commit_named(&mut e, "act", &[]).expect("commits");
    let all = edges(&e);
    assert_eq!(all.len(), 1, "{all:?}");
    let (subject, _, object) = all[0];
    assert!(subject < object, "canonical orientation: {all:?}");

    // Exclusive directed steal, events in order: -old +new.
    let mut e2 = relations_game(
        "        relate bob to sword as carries\n        relate alice to sword as carries",
    );
    let events = common::commit_named(&mut e2, "act", &[]).expect("steals");
    let spoken: Vec<char> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::Related { .. } => Some('+'),
            Event::Unrelated { .. } => Some('-'),
            _ => None,
        })
        .collect();
    assert_eq!(spoken, vec!['+', '-', '+']);
}

// t21 — event fidelity: exactly one Related per written and one
// Unrelated per removed logical edge from authored effects and steals;
// self-pairs, no-op unrelates, and destroy-drops emit nothing. Owned by
// relations::{relate_then_unrelate_basic, self_pair_is_skipped_silently,
// noops_emit_nothing, unrelate_without_kind_severs_all_kinds,
// destruction_drops_edges_silently} and matrix_35 / matrix_44.
#[test]
fn t21_event_fidelity() {
    let mut e = relations_game(
        "        relate alice to alice as knows\n        relate alice to world name:coin as knows\n        unrelate alice from bob as knows\n        relate alice to sword as carries\n        relate bob to sword as carries\n        destroy sword\n        narrate \"done\"",
    );
    let alice = common::named(&e, "alice");
    let events = common::commit_named(&mut e, "act", &[]).expect("commits");
    let related = events
        .iter()
        .filter(|ev| matches!(ev, Event::Related { .. }))
        .count();
    let unrelated = events
        .iter()
        .filter(|ev| matches!(ev, Event::Unrelated { .. }))
        .count();
    // Writes: two knows edges (self-pair skipped) + carries + steal.
    // Removals: the absent unrelate is silent; the steal evicts one; the
    // destroy drops silently.
    assert_eq!(related, 4, "2 knows + 1 carries + 1 steal write");
    assert_eq!(unrelated, 1, "the steal victim only");
    assert_eq!(edges(&e).len(), 2, "the two knows edges survive");
    let _ = alice;
}
