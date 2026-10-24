//! The per-unit failure policy (T4): rollback, abort, and a continuing advance.

use super::*;

const SRC: &str = r#"
game "Fragile"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb failboat autonomous instances 1
    recipe snap
      duration 10m
      effects
        consume aspect:ghost 1

  verb okbot autonomous instances 1
    recipe hum
      duration 10m
      effects
        narrate "still-humming"
"#;

/// T4: an effect failure rolls the unit back to its checkpoint, aborts the
/// unit's situation through the release path with `ResolutionFailed`, and
/// the advance continues — a failed resolution is normal traffic, never a
/// hard-stuck run.
#[test]
fn per_unit_failure_aborts_and_the_advance_continues() {
    let mut e = game(SRC);
    assert_eq!(situations(&e).len(), 2, "the boot checkpoint opened both");

    let events = e.advance(10).expect("advance");
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::ResolutionFailed { reason, .. } if reason.contains("consume shortage")
    )));
    assert_eq!(narration(&events), vec!["still-humming"]);

    // The checkpoint at the settled moment reopens both verbs; the next
    // advance works untouched.
    assert_eq!(situations(&e).len(), 2);
    let events = e.advance(10).expect("advance");
    assert_eq!(narration(&events), vec!["still-humming"]);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
}

/// Checkpoint granularity is per verb: a later verb's instant-completion
/// failure rolls back that verb's open only — an earlier verb's instance
/// survives with its events — and the failed situation is named while it
/// is still live, before its rollback.
#[test]
fn checkpoint_failure_keeps_sibling_opens() {
    let src = r#"
game "CpFail"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

cards
  card token
    stackable
    start in hall

verbs
  verb keeper autonomous instances 1
    recipe hold
      duration 10m
      effects
        narrate "held"

  verb broken autonomous instances 1
    recipe snap
      duration 0
      effects
        consume aspect:ghost 1
"#;
    let mut e = game(src);
    let events = e.advance(10).expect("advance");
    assert_eq!(
        narration(&events),
        vec!["held"],
        "the sibling's events survive the failure"
    );
    let failed: Vec<_> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::ResolutionFailed { sit, reason } => Some((*sit, reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "exactly one failure reported");
    let &(failed_sit, ref reason) = failed.as_slice().first().unwrap();
    assert!(reason.contains("consume shortage"));
    let live = situations(&e);
    assert_eq!(live.len(), 1, "only the keeper's instance is live");
    assert!(
        !live.contains(&failed_sit),
        "the reported id is the failed one"
    );
    assert_eq!(
        e.state().situations[&live[0]].recipe.as_str(),
        "keeper.hold"
    );
}
