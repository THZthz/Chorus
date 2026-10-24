//! Unknown registry keys: aspect keys, card kinds, legacy keys, and prose tokens.

use crate::{expect_error, fixture};
use story_content::code;

#[test]
fn e203_unknown_aspect_key_with_nearest_note() {
    let body = "\
cards
  card c
    unique
    aspects coal 1

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when count(world, aspect:heal) >= 1  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E203,
        "unknown aspect key `heal`",
        Some("nearest declared key is `heat`"),
    );
}

#[test]
fn e203_unknown_aspect_key_without_near_match() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when count(world, aspect:nope) >= 1  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E203,
        "unknown aspect key `nope`",
        None,
    );
}

#[test]
fn e203_unknown_card_kind() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when count(world, kind:ghost) >= 1  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E203,
        "unknown card kind `ghost`",
        None,
    );
}

#[test]
fn e204_unknown_legacy_key() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when legacy.nope >= 1  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E204,
        "unknown legacy key `nope`",
        None,
    );
}

#[test]
fn e205_unknown_prose_token() {
    let body = "\
cards
  card c
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        narrate \"You feel {nope}.\"  # ERR
";
    expect_error(
        &fixture(body),
        code::E205,
        "prose token `{nope}` resolves to nothing known",
        None,
    );
}
