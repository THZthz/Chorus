//! Edge writes and reads: relate/unrelate traffic, symmetric and exclusive maintenance.

use super::*;

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
        vec![
            "carries: [{related:carries of alice}]",
            "carries: [{related:carries of alice}]"
        ],
        "the cue is recorded for the presentation layer"
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
    assert_eq!(
        narration(&events),
        vec!["[{related:knows of first(world name:coin)}]"]
    );
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
