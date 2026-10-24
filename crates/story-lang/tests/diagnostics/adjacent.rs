//! Adjacent stable numbers the two-pass compiler cannot avoid reporting.

use crate::{expect_error, fixture, PREAMBLE};
use story_content::code;

#[test]
fn e201_syntax_error() {
    let body = "frobnicate \"x\"  # ERR\n";
    expect_error(
        &fixture(body),
        code::E201,
        "unknown top-level declaration",
        None,
    );
}

#[test]
fn e202_tab_is_a_lex_error() {
    let src = format!("{PREAMBLE}\n\tcard x  # ERR\n");
    expect_error(&src, code::E202, "tab is a lex error", None);
}

#[test]
fn e244_unknown_zone_in_scoped_query() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when count(zone:nowhere, aspect:heat) >= 1  # ERR
      effects
        narrate \"ok\"
";
    expect_error(&fixture(body), code::E244, "unknown zone `nowhere`", None);
}

#[test]
fn e244_unknown_ending_reference() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        ending vanished  # ERR
";
    expect_error(
        &fixture(body),
        code::E244,
        "unknown ending `vanished`",
        None,
    );
}

#[test]
fn e247_continue_inside_a_recipe_book() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        narrate \"ok\"
      on complete
        rule beat
          effects
            continue via r  # ERR
";
    expect_error(
        &fixture(body),
        code::E247,
        "continue is legal only in recipe effects",
        None,
    );
}

#[test]
fn e247_continue_inside_a_top_level_event_book() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        narrate \"ok\"

on event dawn
  rule rise
    effects
      continue via v.r  # ERR
";
    expect_error(
        &fixture(body),
        code::E247,
        "continue is legal only in recipe effects",
        None,
    );
}

#[test]
fn e244_continue_unknown_target() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        continue via nowhere  # ERR
";
    expect_error(
        &fixture(body),
        code::E244,
        "unknown recipe `nowhere` targeted by `continue`",
        None,
    );
}

#[test]
fn e244_continue_unknown_global_target() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        continue via other.missing  # ERR
";
    expect_error(
        &fixture(body),
        code::E244,
        "unknown recipe `other.missing` targeted by `continue`",
        None,
    );
}

#[test]
fn e201_continue_without_via() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        continue  # ERR
";
    expect_error(
        &fixture(body),
        code::E201,
        "continue is `continue via <recipe>`",
        None,
    );
}
