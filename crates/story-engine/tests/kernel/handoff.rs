//! The `start via` hand-off pins the named recipe.

use super::*;

/// A `start via` hand-off still checks `when` and the instance cap; the
/// pinned recipe opens instead of the selection scan's winner.
#[test]
fn start_via_pins_the_recipe() {
    let src = r#"
game "Handoff"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start hidden_verb via alt

  verb hidden_verb uncommittable instances 2
    recipe main
      chain-only
      duration 5m
      effects
        narrate "main-path"
    recipe alt
      chain-only
      duration 5m
      effects
        narrate "alt-path"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(10).expect("completion, then the hand-off");
    assert_eq!(narration(&events), vec!["alt-path"]);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
}
