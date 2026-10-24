//! Effect-lowering diagnostics: effectset calls, targets, slots, arithmetic, and signals.

use crate::{expect_error, fixture};
use story_content::code;

#[test]
fn e233_unknown_effectset() {
    let body = "\
cards
  card c
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        stoke(1)  # ERR
";
    expect_error(
        &fixture(body),
        code::E233,
        "unknown effectset `stoke`",
        None,
    );
}

#[test]
fn e234_effectset_arity() {
    let body = "\
effectset stoke(n)
  effects
    narrate \"x\"

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        stoke(1, 2)  # ERR
";
    expect_error(
        &fixture(body),
        code::E234,
        "effectset `stoke` expects 1 argument(s), got 2",
        None,
    );
}

#[test]
fn e235_effectset_recursion() {
    let body = "\
effectset rec(n)
  effects
    rec(1)  # ERR

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        rec(1)
";
    expect_error(
        &fixture(body),
        code::E235,
        "effectset `rec` expansion reaches itself",
        None,
    );
}

#[test]
fn e238_handle_target_without_spawn() {
    let body = "\
cards
  card c
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        destroy ghost  # ERR
";
    expect_error(
        &fixture(body),
        code::E238,
        "handle target `ghost` has no enclosing `spawn ... as ghost`",
        None,
    );
}

#[test]
fn e239_name_path_on_non_unique_card() {
    let body = "\
cards
  card c
    stackable
    aspects heat 1

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        set c.heat += 1  # ERR
";
    expect_error(
        &fixture(body),
        code::E239,
        "addresses `c`, which is not `unique`",
        None,
    );
}

#[test]
fn e240_slot_min_exceeds_max() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        s: 3..2 aspect:heat  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E240,
        "minimum 3 exceeds maximum 2",
        None,
    );
}

#[test]
fn e241_greedy_without_finite_max() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      slots
        s: 0.. greedy aspect:heat  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E241,
        "greedy slot `s` has no finite maximum",
        None,
    );
}

#[test]
fn e242_constant_zero_divisor() {
    let body = "\
cards
  card c
    unique
    aspects heat 1

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        set c.heat += 5 / 0  # ERR
";
    expect_error(
        &fixture(body),
        code::E242,
        "expression divides by a constant zero",
        None,
    );
}

#[test]
fn e242_constant_zero_divisor_folded() {
    let body = "\
cards
  card c
    unique
    aspects heat 1

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        set c.heat += 5 / (2 - 2)  # ERR
";
    expect_error(
        &fixture(body),
        code::E242,
        "expression divides by a constant zero",
        None,
    );
}

#[test]
fn e243_signal_without_event_book() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        signal dawn  # ERR
";
    expect_error(
        &fixture(body),
        code::E243,
        "no `on event` book declares `dawn`",
        None,
    );
}

#[test]
fn e244_unknown_card_reference() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        spawn nosuch  # ERR
";
    expect_error(&fixture(body), code::E244, "unknown card `nosuch`", None);
}
