//! The relations primitive (language.md §9): undeclared kinds at every use site, duplicate kinds, boot-edge cards, and card-ref endpoints.

use crate::{expect_error, fixture};
use story_content::code;

const REL_KINDS: &str = "\
relations
  carries exclusive
";

#[test]
fn e248_unknown_kind_in_rel_operand() {
    let body = format!(
        "\
{REL_KINDS}
verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when rel(marries, a, b)  # ERR
      effects
        narrate \"ok\"
"
    );
    expect_error(
        &fixture(&body),
        code::E248,
        "relation kind `marries` is read or written but is not declared",
        None,
    );
}

#[test]
fn e248_unknown_kind_in_related_scope_and_token() {
    let scope = format!(
        "\
{REL_KINDS}
cards
  card a
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when count(related:ties of a) >= 1  # ERR
      effects
        narrate \"ok\"
"
    );
    expect_error(
        &fixture(&scope),
        code::E248,
        "relation kind `ties` is read or written but is not declared",
        None,
    );
    let token = format!(
        "\
{REL_KINDS}
cards
  card a
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        narrate \"x {{related:ties of a}}\"  # ERR
"
    );
    // The prose-token face reports the token checker's E205 for the unknown
    // kind (language.md §7: an unresolvable token), not E248.
    expect_error(
        &fixture(&token),
        code::E205,
        "prose token `{related:ties of a}` resolves to nothing known",
        None,
    );
}

#[test]
fn e248_unknown_kind_in_relate_and_unrelate() {
    let relate = format!(
        "\
{REL_KINDS}
cards
  card a
    unique

  card b
    unique

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      effects
        relate a to b as wields  # ERR
"
    );
    expect_error(
        &fixture(&relate),
        code::E248,
        "relation kind `wields` is read or written but is not declared",
        None,
    );
    let unrelate = relate.replace("relate a to b as wields", "unrelate a from b as wields");
    expect_error(
        &fixture(&unrelate),
        code::E248,
        "relation kind `wields` is read or written but is not declared",
        None,
    );
}

#[test]
fn e248_unknown_kind_in_boot_relates_line() {
    let body = "\
game \"t\"
  seed host
  turn 1m
  home a

zones
  a

relations
  carries exclusive

cards
  card a
    unique
    start in a
    relates ties b  # ERR
";
    expect_error(
        body,
        code::E248,
        "relation kind `ties` is read or written but is not declared",
        None,
    );
}

#[test]
fn e232_duplicate_relation_kind() {
    let body = "\
relations
  carries exclusive

relations
  carries symmetric  # ERR
"
    .to_string();
    expect_error(
        &fixture(&body),
        code::E232,
        "duplicate relation kind `carries`",
        None,
    );
}

#[test]
fn e244_unknown_card_in_boot_relates_line() {
    let body = "\
game \"t\"
  seed host
  turn 1m
  home a

zones
  a

relations
  carries exclusive

cards
  card a
    unique
    start in a
    relates carries ghost  # ERR
";
    expect_error(body, code::E244, "unknown card `ghost`", None);
}

#[test]
fn e239_non_unique_card_ref_endpoint() {
    let body = format!(
        "\
{REL_KINDS}
cards
  card a
    unique

  card pile
    stackable

verbs
  verb v committable instances 1
    recipe r
      duration 0m
      when rel(carries, a, pile)  # ERR
      effects
        narrate \"ok\"
"
    );
    expect_error(
        &fixture(&body),
        code::E239,
        "name path addresses `pile`, which is not `unique`",
        None,
    );
}
