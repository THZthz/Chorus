//! Declaration-shape diagnostics: uniqueness and recipe flags, recursion, duplicates, repeats, and parameter typing.

use crate::{compile, expect_error, expect_warning, fixture};
use story_content::code;

#[test]
fn e221_unique_with_stackable() {
    let body = "\
cards
  card c  # ERR
    unique
    stackable
";
    expect_error(
        &fixture(body),
        code::E221,
        "declares `unique` together with `stackable`/`group`",
        None,
    );
}

#[test]
fn e222_craftable_with_chain_only() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r  # ERR
      craftable chain-only
      duration 0m
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E222,
        "declares `craftable` together with `chain-only`",
        None,
    );
}

#[test]
fn e230_predicate_recursion() {
    let body = "\
predicates
  predicate a = b  # ERR
  predicate b = a
";
    expect_error(
        &fixture(body),
        code::E230,
        "predicate `a` expansion reaches itself",
        None,
    );
}

#[test]
fn e232_duplicate_id_second_site_named() {
    let body = "\
cards
  card c
    stackable

cards
  card c  # ERR
    stackable
";
    expect_error(
        &fixture(body),
        code::E232,
        "duplicate id `c` in the one namespace (card)",
        Some("first declared at t.sw:13"),
    );
}

#[test]
fn e236_repeated_meta_key() {
    let body = "\
cards
  card c
    unique
    meta icon \"a\"
    meta icon \"b\"  # ERR
";
    expect_error(
        &fixture(body),
        code::E236,
        "meta key `icon` repeats on one declaration",
        None,
    );
}

#[test]
fn e236_repeated_on_complete_qualifier() {
    let body = "\
aspects
  heat: 0

cards
  card c
    unique
    on complete [heat]
      rule r1
        effects
          narrate \"x\"
    on complete [heat]  # ERR
      rule r2
        effects
          narrate \"y\"
";
    expect_error(
        &fixture(body),
        code::E236,
        "repeated `on complete` qualifier on one card",
        None,
    );
}

#[test]
fn e236_repeated_on_expire_block() {
    let body = "\
cards
  card c
    unique
    on expire
      rule r1
        effects
          narrate \"x\"
    on expire  # ERR
      rule r2
        effects
          narrate \"y\"
";
    expect_error(
        &fixture(body),
        code::E236,
        "repeated `on expire` on one card",
        None,
    );
}

#[test]
fn e237_recipe_without_duration() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E237,
        "recipe `v.r` has no `duration` line",
        None,
    );
}

#[test]
fn e245_predicate_parameter_type_mismatch() {
    let body = "\
cards
  card c
    unique
    aspects heat 1

predicates
  predicate hot(n) = n.heat >= 1

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when hot(2)  # ERR
      effects
        narrate \"ok\"
";
    expect_error(
        &fixture(body),
        code::E245,
        "argument for `n` of `hot` must be a card name",
        None,
    );
}

#[test]
fn e246_non_positive_lifetime() {
    let body = "\
cards
  card c
    lifetime 0m  # ERR
";
    expect_error(
        &fixture(body),
        code::E246,
        "card lifetimes are positive minute values",
        None,
    );
}

#[test]
fn w110_shadowing_warning_and_allow_shadow_suppression() {
    // Identical `when` at equal priority: the second shadows the first.
    let body = "\
verbs
  verb v committable instances 1
    recipe r1
      priority 50
      duration 0m
      when clock >= 0m
    recipe r2  # ERR
      priority 50
      duration 0m
      when clock >= 0m
";
    expect_warning(
        &fixture(body),
        code::W110,
        "share priority 50 and an identical `when`",
    );

    // A missing `when` is always-true and shadows too.
    let body = "\
verbs
  verb v committable instances 1
    recipe r1
      priority 50
      duration 0m
    recipe r2  # ERR
      priority 50
      duration 0m
";
    expect_warning(
        &fixture(body),
        code::W110,
        "share priority 50 and an identical `when`",
    );

    let body_ok = "\
verbs
  verb v committable instances 1
    recipe r1
      priority 50
      duration 0m
      when clock >= 0m
    recipe r2
      priority 50 allow shadow
      duration 0m
      when clock >= 0m
";
    let diags = compile(&fixture(body_ok));
    assert!(
        diags.iter().all(|d| d.number != code::W110),
        "`allow shadow` must suppress W110: {diags:?}"
    );
}

/// Disjoint `when`s at equal priority never shadow: each moment picks one
/// recipe deterministically, so there is nothing to warn about.
#[test]
fn w110_disjoint_when_at_equal_priority_does_not_warn() {
    let body = "\
verbs
  verb v committable instances 1
    recipe r1
      priority 50
      duration 0m
      when clock >= 0m
    recipe r2
      priority 50
      duration 0m
      when clock >= 1m
";
    let diags = compile(&fixture(body));
    assert!(
        diags.iter().all(|d| d.number != code::W110),
        "disjoint `when`s must not warn: {diags:?}"
    );
}
