//! Boot `relates` edges: canonical rows, reverse reads, and missing-object warnings.

use super::*;

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
