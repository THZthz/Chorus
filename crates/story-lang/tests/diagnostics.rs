//! Every compile diagnostic in language.md §9 (plus the adjacent unnumbered
//! failure shapes) fires on a minimal fixture, asserted by number, location,
//! and message/note substring. The `# ERR` comment marks the expected
//! diagnostic line in each fixture.

use story_content::{code, Diagnostic, Severity};

fn compile(src: &str) -> Vec<Diagnostic> {
    story_lang::compile_source("t.sw", src).diagnostics
}

fn marker_line(src: &str) -> u32 {
    for (i, line) in src.lines().enumerate() {
        if line.contains("# ERR") {
            return i as u32 + 1;
        }
    }
    panic!("fixture has no # ERR marker");
}

fn expect_error(src: &str, number: u32, contains: &str, note_contains: Option<&str>) {
    let diags = compile(src);
    let line = marker_line(src);
    let hit = diags
        .iter()
        .find(|d| d.severity == Severity::Error && d.number == number && d.line == line);
    let d = hit.unwrap_or_else(|| {
        panic!(
            "expected E{number} at t.sw:{line}; got:\n{}",
            diags
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    let hay = format!("{} {}", d.message, d.notes.join(" "));
    assert!(
        hay.contains(contains),
        "E{number} message mismatch: `{hay}` does not contain `{contains}`"
    );
    if let Some(note) = note_contains {
        let notes = d.notes.join(" ");
        assert!(
            notes.contains(note),
            "E{number} note mismatch: `{notes}` does not contain `{note}`"
        );
    }
}

fn expect_warning(src: &str, number: u32, contains: &str) {
    let diags = compile(src);
    let line = marker_line(src);
    let hit = diags
        .iter()
        .find(|d| d.severity == Severity::Warning && d.number == number && d.line == line);
    let d = hit.unwrap_or_else(|| {
        panic!(
            "expected W{number} at t.sw:{line}; got:\n{}",
            diags
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    assert!(
        d.message.contains(contains),
        "W{number} message mismatch: `{}`",
        d.message
    );
}

const PREAMBLE: &str = "\
game \"t\"
  seed host
  turn 1m
  home a

zones
  a

aspects
  heat: 0
";

fn fixture(body: &str) -> String {
    format!("{PREAMBLE}\n{body}\n")
}

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

// ---------------------------------------------------------------------------
// Adjacent stable numbers the two-pass compiler cannot avoid reporting.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// The relations primitive (language.md §9): E248 for undeclared kinds at
// every use site, E232 for a duplicate kind, E244 for an unknown boot-edge
// card, E239 for a non-unique card-ref endpoint.
// ---------------------------------------------------------------------------

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
