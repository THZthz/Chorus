//! The recognition face previews selection, bind eligibility, and slot candidates without mutating.

use super::*;

/// The recognition face answers without mutating state or spending time
/// (kernel-api.md §1): the would-be selection, bind eligibility, and the
/// greedy-ordered slot candidates.
#[test]
fn recognition_face_previews_without_mutating() {
    let src = include_str!("../../../../docs/emberwatch.sw");
    let mut e = boot(compile(src), 7);

    // The would-be selection including the guard outcome.
    let coal = common::named(&e, "coal");
    assert_eq!(
        e.preview_commit(&CommitAction {
            verb: "tend".into(),
            bindings: vec![("fuel".into(), coal)],
        })
        .expect("feed is the would-be selection")
        .0,
        "tend.feed"
    );
    assert_eq!(
        e.preview_commit(&CommitAction {
            verb: "tend".into(),
            bindings: Vec::new()
        })
        .expect("blaze wins the empty-handed scan")
        .0,
        "tend.blaze"
    );
    let d = e
        .preview_commit(&CommitAction {
            verb: "sup".into(),
            bindings: Vec::new(),
        })
        .expect_err("morsel minimum unmet");
    assert_eq!(d.kind, DiagKind::NoRecipe);
    let d = e
        .preview_commit(&CommitAction {
            verb: "omen_sight".into(),
            bindings: Vec::new(),
        })
        .expect_err("uncommittable verbs refuse");
    assert_eq!(d.kind, DiagKind::VerbAutonomous);
    assert_eq!(e.state().now, 0, "preview spends no time");

    // Slot candidates: greedy-ordered, filter-matching, unbound only.
    assert!(e
        .slot_candidates("tend.feed".into(), "kindle".into())
        .is_empty());
    assert_eq!(
        e.slot_candidates("tend.feed".into(), "fuel".into()).len(),
        4,
        "every loose coal is a candidate"
    );

    // Bind eligibility on a live situation's slots.
    common::commit_named(&mut e, "tend", &[("fuel", "coal")]).expect("feed opens");
    let after_open = state_bytes(&e);
    let bound: Vec<_> = e
        .state()
        .situations
        .values()
        .flat_map(|s| s.bindings.values().flatten().map(|b| b.entity))
        .collect();
    let sit = e
        .state()
        .situations
        .values()
        .find(|s| s.recipe.as_str() == "tend.feed")
        .map(|s| s.id)
        .expect("the feed is live");
    let other_coal = e
        .state()
        .entities
        .values()
        .find(|c| c.name == "coal" && !bound.contains(&c.id))
        .map(|c| c.id)
        .expect("a free coal");
    assert_eq!(
        e.can_bind(sit, "fuel".into(), other_coal).unwrap_err().kind,
        DiagKind::ZoneNotBindable,
        "fuel is not an open slot"
    );
    assert_eq!(
        e.can_bind(sit, "kindle".into(), common::named(&e, "tea"))
            .unwrap_err()
            .kind,
        DiagKind::SlotFilterFail,
        "tea carries no kindle"
    );
    // The recognition face mutated nothing: same bytes as before the asks.
    assert_eq!(state_bytes(&e), after_open, "recognition never mutates");
}
