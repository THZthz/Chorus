//! Slot presence diagnostics: [E249] undeclared `slot:` references in a
//! `shown when` condition, [W111] `internal` + `open`, [W112] `greedy` +
//! `shown when`, the [E236] repeat, and the malformed-line [E201].

use crate::{expect_error, expect_warning, fixture};
use story_content::code;

#[test]
fn e249_presence_references_undeclared_slot() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..1
          shown when count(slot:missing) == 0  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E249,
        "references `slot:missing`, which the recipe does not declare",
        None,
    );
}

#[test]
fn e249_self_reference_is_legal() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..1
          shown when count(slot:a) == 0
      effects
        narrate \"ok\"
";
    let diags = crate::compile(&fixture(body));
    assert!(
        diags.iter().all(|d| d.number != code::E249),
        "self reference must not raise E249: {:?}",
        diags
    );
}

#[test]
fn w111_internal_with_open() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..1 internal open  # ERR
      effects
        narrate \"ok\"
";
    expect_warning(&fixture(body), code::W111, "`open` has no effect");
}

#[test]
fn w112_greedy_with_shown_when() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..2 greedy  # ERR
          shown when clock >= 1h
      effects
        narrate \"ok\"
";
    expect_warning(&fixture(body), code::W112, "fill the slot even while it is hidden");
}

#[test]
fn e236_second_shown_when_line() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..1
          shown when clock >= 1h
          shown when clock >= 2h  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E236,
        "second `shown when` line",
        None,
    );
}

#[test]
fn e201_shown_line_without_when() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        a: 0..1
          shown clock >= 1h  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E201,
        "a slot presence line is `shown when <condition>`",
        None,
    );
}
