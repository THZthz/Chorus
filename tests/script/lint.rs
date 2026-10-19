//! Lint — the compiler's load-time discipline: loud load errors for the
//! corners that would otherwise detonate later, and the lint layer
//! (warnings that never throw).

use chorus::content::script::{
    compile_script, lint_script, lint_script_findings, LintFinding, LintSeverity,
};
use chorus::core::dict::core_dictionary;

use crate::common::harness::assert_err_msg;

/// The bundled showcase, read as text (script text = data).
const FLYER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/the-nightjar-flyer.txt"));

/// The shared base script. Byte-shape
/// preserved (leading newline, section order) because line numbers flow into
/// load-error messages.
fn base(extra: &str) -> String {
    format!(
        "
script: Probe
player: MARCH
start: 0:0

entities:
  MARCH is character at HOUSE
  HOUSE is place
{extra}
"
    )
}

fn compile(
    text: &str,
) -> Result<chorus::content::script::CompiledScript, chorus::error::ChorusError> {
    let dict = core_dictionary();
    compile_script(text, &dict)
}

// ---------------------------------------------------------------------------
// error upgrades — silent accepts become load errors
// ---------------------------------------------------------------------------

#[test]
fn a_template_for_an_unregistered_verb_is_rejected() {
    assert_err_msg(
        compile(&base("templates:\n  WRONGVERB says \"{actors} reads.\"")),
        r"WRONGVERB.*not a registered verb",
    );
}

#[test]
fn player_must_name_a_declared_character() {
    let dict = core_dictionary();
    assert_err_msg(
        compile_script(
            "script: P\nplayer: GHOST\nstart: 0:0\nentities:\n  ANA is character at HOUSE\n  HOUSE is place",
            &dict,
        ),
        r"player 'GHOST' is not a declared entity",
    );
    assert_err_msg(
        compile_script(
            "script: P\nplayer: HOUSE\nstart: 0:0\nentities:\n  ANA is character at HOUSE\n  HOUSE is place",
            &dict,
        ),
        r"player 'HOUSE' must be a character",
    );
}

#[test]
fn reserved_words_cannot_be_names() {
    // (§5.1)
    assert_err_msg(compile(&base("  TO is place")), r"'TO' is a reserved word");
    assert_err_msg(compile(&base("  WITH is character at HOUSE")), r"'WITH' is a reserved word");
    assert_err_msg(compile(&base("  MARCH is parent_child of TO")), r"'TO' is a reserved word");
    assert_err_msg(compile(&base("prototypes:\n  IF is idea")), r"'IF' is a reserved word");
    // non-lexeme names are legal — §10's unsafe-name corner, quote every ref
    assert!(compile(&base("  X-Y is item")).is_ok());
}

#[test]
fn duplicate_entity_declarations_are_a_load_error_not_last_write_wins() {
    assert_err_msg(
        compile(&base("  HOUSE is place with study:1")),
        r"duplicate entity declaration 'HOUSE'",
    );
}

#[test]
fn a_name_declared_both_as_entity_and_prototype_can_never_spawn_load_error() {
    assert_err_msg(
        compile(&base("  SPARK is idea\nprototypes:\n  SPARK is idea")),
        r"'SPARK' is declared both as an entity and a prototype",
    );
}

#[test]
fn a_typo_d_slot_role_is_a_load_error_not_a_silent_never_matching_slot() {
    // Q4 (spec §4.3's role list + §10's fail-loudly rule): the slot
    // key is validated against the role vocabulary at load — a slot can
    // never compile into a silent never-match.
    // (The fixture trips exactly ONE error — the count and query are legal.)
    assert_err_msg(
        compile(&base(
            "verbs:\n  READ:\n    actrs: 1 kind:character\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"read.\"\n",
        )),
        r"line \d+: bad slot role 'actrs' \(expected one of actors, object, with, at, about, to, held\)",
    );
}

#[test]
fn duplicate_prototype_declarations_are_a_load_error_not_last_write_wins() {
    // Q5: duplicate prototype declarations are a load error like
    // entities/endings. The fixture trips exactly ONE
    // error — both lines are individually legal (no slot typo to double-fire
    // into Q4's check).
    assert_err_msg(
        compile(&base("prototypes:\n  SPARK is idea\n  SPARK is idea with secret:1")),
        r"line \d+: duplicate prototype 'SPARK'",
    );
}

#[test]
fn duplicate_template_declarations_are_a_load_error_not_last_write_wins() {
    assert_err_msg(
        compile(&base(
            "templates:\n  READ says \"{actors} read {object}.\"\n  READ says \"{actors} skim {object}.\"",
        )),
        r"line \d+: duplicate template 'READ'",
    );
}

#[test]
fn a_repeated_verb_header_is_a_load_error_not_a_silent_reset() {
    // a repeated header is a load error, never a silent block reset
    assert_err_msg(
        compile(&base(
            "verbs:\n  READ:\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"one.\"\n  READ:\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"two.\"\n",
        )),
        r"line \d+: duplicate verb declaration 'READ'",
    );
}

#[test]
fn case_colliding_entity_names_are_a_minted_id_collision_load_error() {
    // Q5's fourth half: names are case-exact (§5.1), so 'ana'/'ANA' pass
    // the name dedup — but both mint the script id s_ana
    // (`s_{name.to_lowercase()}`), and the second add_entity would silently
    // clobber the first. The id collision is its own load error.
    assert_err_msg(
        compile(&base("  ana is character at HOUSE\n  ANA is character at HOUSE")),
        r"line \d+: minted id 's_ana' already in use \('ana' vs 'ANA'\)",
    );
}

#[test]
fn an_illegal_character_in_a_when_line_is_a_load_error_naming_char_and_offset() {
    // Q2 (§6.3's grammar has no `@`): the tokenizer fails at load naming
    // the illegal character and its offset — a `@3`-class typo can never
    // silently read as a number. (The fixture trips exactly ONE error.)
    assert_err_msg(
        compile(&base(
            "verbs:\n  READ:\n    object: 1 kind:text\n    recipes:\n      r1 when object.aspect.secret >= @2 -> NARRATE \"x.\"\n",
        )),
        r"illegal character '@' at offset 24 in predicate 'object.aspect.secret >= @2'",
    );
}

// ---------------------------------------------------------------------------
// lint — warnings that never throw
// ---------------------------------------------------------------------------

#[test]
fn the_bundled_showcase_lints_clean() {
    let dict = core_dictionary();
    assert_eq!(lint_script(FLYER, &dict).unwrap(), Vec::<String>::new());
}

#[test]
fn a_verb_with_slots_but_no_recipes() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            "
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
",
        ),
        &dict,
    )
    .unwrap();
    assert!(w.iter().any(|x| x.contains("verb READ: declares slots but no recipes")), "{w:?}");
}

#[test]
fn a_recipe_after_a_catch_all_is_unreachable() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            "
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.any -> NARRATE \"{actors} read.\"
      read.late when object.aspect.secret >= 3 -> NARRATE \"secret.\"
",
        ),
        &dict,
    )
    .unwrap();
    assert!(w.iter().any(|x| x.contains("read.late") && x.contains("unreachable")), "{w:?}");
}

#[test]
fn an_identical_when_shadows_the_later_recipe() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            "
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.first when object.aspect.secret >= 3 -> NARRATE \"a.\"
      read.second when object.aspect.secret   >= 3 -> NARRATE \"b.\"
",
        ),
        &dict,
    )
    .unwrap();
    assert!(
        w.iter().any(|x| x.contains("read.second") && x.contains("shadowed by read.first")),
        "{w:?}"
    );
}

#[test]
fn a_count_role_bound_beyond_the_slot_cardinality_can_never_hold() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            "
verbs:
  TALK:
    actors: 1..2 kind:character
    duration: 30m
    recipes:
      talk.trio when count(actors) >= 3 -> NARRATE \"three.\"
      talk.duo when count(actors) = 2 -> NARRATE \"two.\"
",
        ),
        &dict,
    )
    .unwrap();
    assert!(
        w.iter()
            .any(|x| x.contains("talk.trio") && x.contains("count(actors) >= 3 can never hold")),
        "{w:?}"
    );
    assert!(!w.iter().any(|x| x.contains("talk.duo")), "{w:?}"); // = 2 is satisfiable
}

/// One `when`, one verdict: the slot-dead count gate already proves the
/// recipe's when false, so the count-gate ERROR and the never-matching
/// WARNING (the same root cause) surface as ONE finding. A count over a
/// role the verb never declares produces no count-gate error at all — that
/// warning survives the guard
/// (`a_count_over_a_role_the_verb_never_binds_warns_and_its_negation_does_not`).
#[test]
fn a_slot_dead_count_gate_reports_one_error_not_an_error_plus_its_warning() {
    let findings = findings_for(
        "
verbs:
  TALK:
    actors: 1..2 kind:character
    duration: 30m
    recipes:
      talk.trio when count(actors) >= 3 -> NARRATE \"three.\"
",
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].severity, LintSeverity::Error, "{findings:?}");
    assert!(
        findings[0].message.contains("talk.trio") && findings[0].message.contains(">= 3"),
        "{findings:?}"
    );
}

/// `!=` is the count-gate alternation's sixth op. Its arm: dead only when
/// the slot's WHOLE range is the excluded value; a partial
/// exclusion (or any other slot value) is satisfiable and silent.
#[test]
fn a_count_neq_excluding_the_whole_slot_range_is_an_error_not_a_panic() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            "
verbs:
  TALK:
    actors: 1..2 kind:character
    duration: 30m
    recipes:
      talk.solo when count(actors) != 1 -> NARRATE \"a.\"
      talk.duo when count(actors) != 2 -> NARRATE \"b.\"
  WORK:
    actors: 1 kind:character
    duration: 1h
    recipes:
      work.alone when count(actors) != 1 -> NARRATE \"c.\"
      work.drift when count(actors) != 3 -> NARRATE \"d.\"
",
        ),
        &dict,
    )
    .unwrap();
    // the 1..1 slot minus the excluded 1 is EMPTY → dead
    assert!(
        w.iter()
            .any(|x| x.contains("work.alone") && x.contains("count(actors) != 1 can never hold")),
        "{w:?}"
    );
    // partial exclusions on 1..2 keep the other value → satisfiable
    assert!(!w.iter().any(|x| x.contains("talk.solo")), "{w:?}");
    assert!(!w.iter().any(|x| x.contains("talk.duo")), "{w:?}");
    // excluding a value the slot never takes is vacuous → satisfiable
    assert!(!w.iter().any(|x| x.contains("work.drift")), "{w:?}");
}

#[test]
fn unknown_narrate_tokens_warn_roles_and_function_params_do_not() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      default -> NARRATE "{actors} read {object} with {bject}."

functions:
  sus who -> NARRATE "the conductor's eyes linger on {who}, {actors} noticed."

triggers:
  dusk at hour 16 -> NARRATE "the lamps hum {louder}."
"#,
        ),
        &dict,
    )
    .unwrap();
    assert!(w.iter().any(|x| x.contains("read.default") && x.contains("{bject}")), "{w:?}");
    assert!(!w.iter().any(|x| x.contains("function sus") && x.contains("{louder}")), "{w:?}"); // params are known in bodies
    assert!(w.iter().any(|x| x.contains("trigger dusk") && x.contains("{louder}")), "{w:?}");
}

#[test]
fn a_deck_entry_that_is_not_stackable_trips_the_livelock_rule() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER, LETTER reset

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      default -> NARRATE "{actors} read {object}."
"#,
        ),
        &dict,
    )
    .unwrap();
    assert!(w.iter().any(|x| x.contains("deck POST: entry LETTER is not stackable")), "{w:?}");
}

// ---------------------------------------------------------------------------
// the deck livelock rule's once-only exemption — a non-stackable spec entry
// that can physically be served only once cannot collide with itself
// ---------------------------------------------------------------------------

/// One spec entry, no `reset`, no other deck serving the name: the deck is
/// exhausted before a second copy is ever asked for — clean.
#[test]
fn a_once_served_spec_entry_without_reset_is_exempt() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(w, Vec::<String>::new(), "{w:?}");
}

/// A repeated spec entry is served at two positions — the exemption never
/// applies, and each served position speaks.
#[test]
fn a_spec_entry_served_twice_is_reported_per_position() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER, LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    let deck: Vec<_> = w.iter().filter(|x| x.contains("entry LETTER is not stackable")).collect();
    assert_eq!(deck.len(), 2, "{w:?}");
}

/// `reset` wraps the spec: the entry is served again after the wrap, so the
/// once-only physics never holds.
#[test]
fn a_reset_deck_reports_its_once_entry() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER reset
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(
        w.iter().filter(|x| x.contains("entry LETTER is not stackable")).count(),
        1,
        "{w:?}"
    );
}

/// A defaultcard serves forever, but the SPEC period still deals each spec
/// entry exactly once — the spec entry stays exempt beside one.
#[test]
fn a_defaultcard_deck_still_exempts_its_once_spec_entry() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1
  COIN is resource stackable

decks:
  POST spec LETTER defaultcard COIN
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(w, Vec::<String>::new(), "{w:?}");
}

/// The defaultcard entry itself is never exempt: it is served on every
/// later draw.
#[test]
fn the_defaultcard_entry_itself_is_reported() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1
  COIN is resource stackable

decks:
  POST spec COIN defaultcard LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(
        w.iter().filter(|x| x.contains("entry LETTER is not stackable")).count(),
        1,
        "{w:?}"
    );
}

/// A second deck serving the same name hands the first deck a living copy
/// to collide with — both decks report.
#[test]
fn the_same_name_in_two_decks_reports_from_both() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER
  POST2 spec LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(
        w.iter().filter(|x| x.contains("entry LETTER is not stackable")).count(),
        2,
        "{w:?}"
    );
}

/// The census counts defaultcard positions too: deck A's spec serves the
/// name exactly once, but deck B's `defaultcard` deals the same name again
/// on every later draw — the exemption is cancelled and both decks report.
#[test]
fn a_defaultcard_in_another_deck_cancels_the_once_spec_entrys_exemption() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1
  COIN is resource stackable

decks:
  POST spec LETTER
  POST2 spec COIN defaultcard LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    let deck: Vec<_> = w.iter().filter(|x| x.contains("entry LETTER is not stackable")).collect();
    assert_eq!(deck.len(), 2, "{w:?}");
    assert!(deck.iter().any(|x| x.contains("deck POST:")), "{deck:?}");
    assert!(deck.iter().any(|x| x.contains("deck POST2:")), "{deck:?}");
}

// ---------------------------------------------------------------------------
// the §4.2 `unique` keyword (v0.9.6): a prototype-line keyword only, and the
// deck livelock advisory's exemption (the mint face skips while a copy
// lives, so the deck never pins)
// ---------------------------------------------------------------------------

#[test]
fn unique_and_stackable_on_one_prototype_line_is_a_load_error() {
    assert_err_msg(
        compile(&base("prototypes:\n  RELIC is item unique stackable")),
        r"prototype 'RELIC' cannot carry both `stackable` and `unique` — a unique prototype is never stackable",
    );
    assert_err_msg(
        compile(&base("prototypes:\n  RELIC is item stackable unique")),
        r"prototype 'RELIC' cannot carry both `stackable` and `unique` — a unique prototype is never stackable",
    );
}

#[test]
fn unique_on_an_entity_line_is_a_load_error() {
    assert_err_msg(
        compile(&base("  RELIC is item unique")),
        r"entity 'RELIC' cannot carry `unique` — a declared entity exists once by name already",
    );
    // the keyword position only: the word inside a desc string is prose
    assert!(compile(&base("  RELIC is item desc \"a unique relic\"")).is_ok());
}

#[test]
fn a_unique_deck_entry_is_exempt_from_the_livelock_rule() {
    // the mint face skips while a copy lives (§4.2): the deck never pins
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
prototypes:
  RELIC is item unique

decks:
  POST spec RELIC, RELIC reset
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(w, Vec::<String>::new(), "{w:?}");
}

/// `spec X defaultcard X` is one problem about one name: exactly one
/// finding (the spec position's), never the byte-identical pair.
#[test]
fn spec_and_defaultcard_naming_one_entry_report_once() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1

decks:
  POST spec LETTER defaultcard LETTER
"#,
        ),
        &dict,
    )
    .unwrap();
    let deck: Vec<_> = w.iter().filter(|x| x.contains("entry LETTER is not stackable")).collect();
    assert_eq!(deck.len(), 1, "{w:?}");
}

/// The kind-level check answers first: an entry whose kind is stackable is
/// clean without the exemption ever being consulted.
#[test]
fn a_stackable_kind_entry_is_clean_without_the_exemption() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
  LETTER is text with secret:1
  SCRAP is text stackable

decks:
  POST spec LETTER, LETTER reset
"#,
        ),
        &dict,
    )
    .unwrap();
    assert_eq!(w, Vec::<String>::new(), "{w:?}");
}

#[test]
fn an_aspect_queried_but_never_declared_warns_declared_ones_do_not() {
    let dict = core_dictionary();
    let w = lint_script(
        &base(
            r#"
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.ocult when object.aspect.ocult >= 2 -> NARRATE "the letters swim."
      default -> NARRATE "{actors} read {object}."
"#,
        ),
        &dict,
    )
    .unwrap();
    assert!(w.iter().any(|x| x.contains("aspect 'ocult' is queried but never declared")), "{w:?}");
}

#[test]
fn a_non_lexeme_entity_name_warns_legal_but_every_ref_needs_quotes() {
    let dict = core_dictionary();
    let w = lint_script(&base("  X-Y is item with weight:1"), &dict).unwrap();
    assert!(w.iter().any(|x| x.contains("entity 'X-Y': not a name lexeme")), "{w:?}");
}

#[test]
fn an_unknown_meta_key_is_a_load_error_naming_the_key() {
    // Q15 fixed: an unknown meta key names itself and lists the five keys.
    // The old wildcard arm treated anything un-listed as the clock key (so
    // the next key added to the regex without a match arm would have been
    // blamed on "bad clock"); unknown keys fell through to the section
    // error, which never mentioned meta at all. The fixture trips exactly
    // ONE error — the meta line fires before any section content parses.
    assert_err_msg(
        compile("script: Probe\nplayer: MARCH\ntitle: My Game\nentities:\n  MARCH is character\n"),
        r"line 3: unknown meta key 'title' \(the meta keys are: script, player, goal, start, seed\)",
    );
}

#[test]
fn a_bad_start_value_blames_the_clock_not_the_key() {
    // Q15's honest-attribution face: the KNOWN clock key keeps its clock
    // error (correct attribution) — only the un-listed-key path changed.
    assert_err_msg(
        compile("script: Probe\nplayer: MARCH\nstart: banana\nentities:\n  MARCH is character\n"),
        r#"bad clock 'banana' \(expected "day:tick"\)"#,
    );
}

// ---------------------------------------------------------------------------
// E11 — the never-matching-`when` detector (the WARNING class)
// ---------------------------------------------------------------------------

fn findings_for(extra: &str) -> Vec<LintFinding> {
    let dict = core_dictionary();
    lint_script_findings(&base(extra), &dict).unwrap()
}

fn warnings_of(findings: &[LintFinding]) -> Vec<&LintFinding> {
    findings.iter().filter(|f| f.severity == LintSeverity::Warning).collect()
}

/// The converter's dead-marker shape (E11): on an `actors` slot of 1..1,
/// `NOT (count(actors) >= 1)` is false for every §6.6 selection binding —
/// the inner atom holds in every window, so the negation never does. The
/// satisfiable negation beside it (`NOT (count(actors) = 1)` over 1..2:
/// satisfiable through the slot's other value) stays silent — the detector
/// complements exact verdicts only. (The fixtures use slot-TRUE inner atoms
/// throughout: a slot-dead inner atom is the count-gate ERROR's
/// territory, negation or not.)
#[test]
fn a_negated_count_gate_the_slot_forces_always_true_is_a_warning() {
    let findings = findings_for(
        "
verbs:
  TALK:
    actors: 1 kind:character
    duration: 30m
    recipes:
      talk.mark when NOT (count(actors) >= 1) -> NARRATE \"dead.\"
  WORK:
    actors: 1..2 kind:character
    duration: 1h
    recipes:
      work.sat when NOT (count(actors) = 1) -> NARRATE \"fine.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 1, "{findings:?}");
    assert!(
        warnings[0].message.contains("talk.mark")
            && warnings[0].message.contains("count(actors) < 1"),
        "{findings:?}"
    );
    assert!(findings.iter().all(|f| !f.message.contains("work.sat")), "{findings:?}");
    assert!(
        findings.iter().all(|f| f.severity == LintSeverity::Warning),
        "the marker fixtures must not trip the ERROR class: {findings:?}"
    );
}

/// All six op spellings the converter's marker convention emits (`NOT
/// (count(actors) <op> <k>)` with the inner atom slot-forced true) are the
/// same single warning.
#[test]
fn every_negation_spelling_of_a_slot_forced_count_is_dead() {
    let dict = core_dictionary();
    for (op, k) in [(">=", 0), (">=", 1), ("=", 1), ("<=", 1), ("<", 2), (">", 0)] {
        let text = base(&format!(
            "
verbs:
  WORK:
    actors: 1 kind:character
    duration: 1h
    recipes:
      work.mark when NOT (count(actors) {op} {k}) -> NARRATE \"dead.\"
"
        ));
        let findings = lint_script_findings(&text, &dict).unwrap();
        let warnings = warnings_of(&findings);
        assert_eq!(warnings.len(), 1, "NOT (count(actors) {op} {k}): {findings:?}");
        assert!(warnings[0].message.contains("work.mark"), "{findings:?}");
    }
}

/// A slot the verb does not declare binds nothing (§6.6 — validate_slots
/// refuses any ref to it), so its count is exactly 0: `>= 1` is dead, and
/// its negation is silently always-true.
#[test]
fn a_count_over_a_role_the_verb_never_binds_warns_and_its_negation_does_not() {
    let findings = findings_for(
        "
verbs:
  READ:
    actors: 1 kind:character
    duration: 1h
    recipes:
      read.obj when count(object) >= 1 -> NARRATE \"dead.\"
      read.noobj when NOT (count(object) >= 1) -> NARRATE \"fine.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 1, "{findings:?}");
    assert!(
        warnings[0].message.contains("read.obj")
            && warnings[0].message.contains("count(object) >= 1"),
        "{findings:?}"
    );
    assert!(findings.iter().all(|f| !f.message.contains("read.noobj")), "{findings:?}");
}

/// Same-variable contradictions inside one conjunction: disjoint pins on one
/// count, disjoint pins on one total — either side alone is satisfiable;
/// together they admit no value (§6.3).
#[test]
fn disjoint_pins_on_one_count_or_total_are_warnings() {
    let findings = findings_for(
        "
  LEDGER is text with secret:1

verbs:
  READ:
    actors: 1..2 kind:character
    object: 0..1 kind:text
    duration: 1h
    recipes:
      read.count when count(actors) <= 1 AND count(actors) >= 2 -> NARRATE \"dead.\"
      read.total when total(object, aspect:secret) >= 1 AND total(object, aspect:secret) < 1 -> NARRATE \"dead too.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 2, "{findings:?}");
    assert!(
        warnings
            .iter()
            .all(|w| w.message.contains("read.count") || w.message.contains("read.total")),
        "{findings:?}"
    );
}

/// The emptiness coupling: a conjunct that pins a role empty (`NOT
/// (count(with) >= 1)`, `count(object) = 0`) against one that needs the
/// role bound (`total(with, …) >= 1`, a positive `object.aspect.…` path) is
/// the `NOT (count(actors) >= 1) AND total(with, …) >= 1` family the E11
/// ruling names — dead even though neither atom is.
#[test]
fn a_when_requiring_a_role_both_empty_and_bound_is_a_warning() {
    let findings = findings_for(
        "
  LEDGER is text with secret:1

verbs:
  READ:
    actors: 1 kind:character
    object: 0..1 kind:text
    with: 0..1 kind:text
    duration: 1h
    recipes:
      read.total when NOT (count(with) >= 1) AND total(with, aspect:secret) >= 1 -> NARRATE \"dead.\"
      read.path when count(object) = 0 AND object.aspect.secret >= 1 -> NARRATE \"dead too.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 2, "{findings:?}");
    assert!(warnings.iter().all(|w| w.message.contains("both empty and bound")), "{findings:?}");
}

/// OR branches are independent worlds: dead only when EVERY branch is dead;
/// a tautology (either branch always true) is silent. (NOT-wrapped branches
/// with slot-true inner atoms are the count-gate scan's territory and must
/// stay silent here.)
#[test]
fn an_or_is_dead_only_when_every_branch_is_dead() {
    let findings = findings_for(
        "
verbs:
  TALK:
    actors: 1 kind:character
    duration: 30m
    recipes:
      talk.dead when NOT (count(actors) >= 1) OR NOT (count(actors) = 1) -> NARRATE \"dead.\"
      talk.taut when NOT (count(object) >= 1) OR NOT (count(actors) >= 1) -> NARRATE \"fine.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 1, "{findings:?}");
    assert!(warnings[0].message.contains("talk.dead"), "{findings:?}");
    assert!(findings.iter().all(|f| !f.message.contains("talk.taut")), "{findings:?}");
    assert!(
        findings.iter().all(|f| f.severity == LintSeverity::Warning),
        "the OR fixtures must not trip the ERROR class: {findings:?}"
    );
}

/// The documented soundness boundary: world reads have no model at load, so
/// even an immediate `exists(A) AND NOT exists(A)` stays silent, as does the
/// pool form `count(name:…)` — catching those needs a world, not a grammar.
#[test]
fn world_reads_stay_unknown_even_when_paternally_contradictory() {
    let findings = findings_for(
        "
verbs:
  TALK:
    actors: 1 kind:character
    duration: 30m
    recipes:
      talk.world when exists(GHOST) AND NOT exists(GHOST) -> NARRATE \"a.\"
      talk.pool when count(name:GHOST) = 5 AND count(name:GHOST) = 1 -> NARRATE \"b.\"
",
    );
    assert!(warnings_of(&findings).is_empty(), "{findings:?}");
    assert!(findings.is_empty(), "{findings:?}");
}

/// The two-class design (E11): today's finding set keeps its ERROR class
/// (the shadowed-when check here), the new detector is the WARNING class.
#[test]
fn the_classes_split_errors_keep_todays_findings_warnings_are_the_detector() {
    let findings = findings_for(
        "
verbs:
  TALK:
    actors: 1 kind:character
    duration: 30m
    recipes:
      talk.first when count(actors) = 1 -> NARRATE \"a.\"
      talk.second when count(actors) = 1 -> NARRATE \"b.\"
      talk.mark when NOT (count(actors) >= 1) -> NARRATE \"dead.\"
",
    );
    let shadowed = findings
        .iter()
        .find(|f| f.message.contains("talk.second"))
        .unwrap_or_else(|| panic!("the shadowed-when finding must exist: {findings:?}"));
    assert_eq!(shadowed.severity, LintSeverity::Error);
    let marked = findings
        .iter()
        .find(|f| f.message.contains("talk.mark"))
        .unwrap_or_else(|| panic!("the never-matching-when warning must exist: {findings:?}"));
    assert_eq!(marked.severity, LintSeverity::Warning);
}

// ---------------------------------------------------------------------------
// W1 — the always-true world-form gate (the WARNING class)
// ---------------------------------------------------------------------------

/// The W1 fixture base: MARCH plus a load-borne funds bearer.
fn lint_base(extra: &str) -> String {
    format!(
        "
script: Probe
player: MARCH
start: 0:0

entities:
  MARCH is character
  POT is resource with funds:3
{extra}
"
    )
}

/// The whole lint over the W1 fixture — the E1 checks reuse it too.
fn lint_findings(extra: &str) -> Vec<LintFinding> {
    let dict = core_dictionary();
    lint_script_findings(&lint_base(extra), &dict).unwrap()
}

#[test]
fn an_exists_gate_on_an_aspect_borne_from_load_warns_naming_the_aspect_and_bearer() {
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when exists(aspect:funds>=1) -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(
        w[0].message.contains(
            "recipe rite.gate (RITE): exists(aspect:funds>=1) is always true — aspect 'funds' is borne from load by POT, so the gate can never gate (§6.3)"
        ),
        "{w:?}"
    );
    assert_eq!(w[0].severity, LintSeverity::Warning);
}

#[test]
fn an_open_count_gate_already_satisfied_by_load_bearers_warns() {
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when count(aspect:funds>=2) >= 1 -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(
        w[0]
            .message
            .contains("count(aspect:funds>=2) >= 1 is always true — aspect 'funds' is borne from load by 1 card(s) from load"),
        "{w:?}"
    );
}

#[test]
fn the_never_gating_faces_stay_silent() {
    // a name-constrained form gates on the name half; a pure-name exists
    // gates on a card; a closed count comparison can be grown past; a NOT
    // form is the never-matching detector's territory; a threshold the
    // load-present pool does not meet can genuinely gate
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item
  PURSE is resource with funds:1

decks:
  D1 spec PURSE defaultcard PURSE

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.named when exists(name:POT aspect:funds>=1) -> SPAWN PILLAR
      rite.pure when exists(POT) -> SPAWN PILLAR
      rite.closed when count(aspect:funds>=1) <= 1 -> SPAWN PILLAR
      rite.negated when NOT exists(aspect:funds>=9) -> SPAWN PILLAR
      rite.unmet when count(aspect:funds>=1) >= 9 -> SPAWN PILLAR
      rite.unmet_exists when exists(aspect:funds>=9) -> SPAWN PILLAR
      default -> NARRATE \"done.\"
",
    );
    let w = warnings_of(&findings);
    assert!(w.is_empty(), "{w:?}");
}

#[test]
fn a_defaultcard_bearer_warns_with_the_once_served_face_naming_the_card() {
    // a defaultcard is NOT borne from load — before the deck's first serve
    // the gate reads false — so the warning carries its own face
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item
  COIN is resource with funds:5

decks:
  D1 spec PILLAR defaultcard COIN

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when exists(aspect:funds>=5) -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(
        w[0].message.contains(
            "exists(aspect:funds>=5) is always true once the deck serves its default card COIN"
        ),
        "{w:?}"
    );
    assert!(
        w[0].message.contains("so the gate can never gate after the deck's first serve"),
        "{w:?}"
    );
    assert!(!w[0].message.contains("borne from load"), "{w:?}");
}

#[test]
fn a_defaultcard_bearer_does_not_count_toward_the_load_present_count() {
    // POT alone meets `>= 1` from load — the warning stands; the
    // defaultcard COIN adds nothing to the load-present count, so `>= 2`
    // can genuinely gate before the deck's first serve — silent
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item
  COIN is resource with funds:5

decks:
  D1 spec PILLAR defaultcard COIN

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.met when count(aspect:funds>=1) >= 1 -> SPAWN PILLAR
      rite.unmet when count(aspect:funds>=1) >= 2 -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(w[0].message.contains("rite.met"), "{w:?}");
}

#[test]
fn a_kill_of_the_only_bearer_stands_the_always_true_warning_down() {
    // BANISH destroys POT: the aspect can genuinely leave the world, so
    // the always-true claim is false and the warning stands down
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when exists(aspect:funds>=1) -> SPAWN PILLAR
  BANISH:
    actors: 1 kind:character
    duration: 0m
    recipes:
      banish.pot -> DESTROY POT
",
    );
    assert!(warnings_of(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_surviving_bearer_keeps_the_warning_naming_only_the_survivors() {
    // PURSE (funds:2) is untouched by BANISH, so the gate remains always
    // true through it — the finding names PURSE, never the killable POT
    let script = "
script: Probe
player: MARCH
start: 0:0

entities:
  MARCH is character
  POT is resource with funds:3
  PURSE is resource with funds:2

prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when exists(aspect:funds>=1) -> SPAWN PILLAR
  BANISH:
    actors: 1 kind:character
    duration: 0m
    recipes:
      banish.pot -> DESTROY POT
";
    let dict = core_dictionary();
    let findings = lint_script_findings(script, &dict).unwrap();
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(w[0].message.contains("borne from load by PURSE"), "{w:?}");
    assert!(!w[0].message.contains("POT,"), "{w:?}");
}

#[test]
fn w1_atoms_inside_and_and_or_still_warn() {
    // an always-true atom stays always-true under AND and under OR — the
    // walk lifts through the boolean connectives
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item
  GRAVE is place with winter:1

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.and when count(actors) >= 1 AND exists(aspect:funds>=1) -> SPAWN PILLAR
      rite.or when exists(aspect:winter>=9) OR exists(aspect:funds>=1) -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 2, "{findings:?}");
    assert!(
        w.iter().all(|f| f.message.contains("exists(aspect:funds>=1) is always true")),
        "{w:?}"
    );
    assert!(w.iter().any(|f| f.message.contains("rite.and")), "{w:?}");
    assert!(w.iter().any(|f| f.message.contains("rite.or")), "{w:?}");
}

#[test]
fn a_flipped_operand_count_form_normalizes_before_the_op_check() {
    // `1 <= count(aspect:funds>=2)` ≡ `count(aspect:funds>=2) >= 1` — the
    // normalization mirrors the op, then the always-true check runs
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.flipped when 1 <= count(aspect:funds>=2) -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(w[0].message.contains("count(aspect:funds>=2) >= 1 is always true"), "{w:?}");
}

#[test]
fn a_multi_term_or_negated_query_and_a_dotted_read_stay_silent() {
    // a multi-term query can gate on its companions; a negated term can
    // gate on absence; the dotted read matches no load-present bearer here
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item
  PURSE is resource with funds:1

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.multi when exists(aspect:funds>=1 kind:resource) -> SPAWN PILLAR
      rite.negterm when exists(-aspect:funds) -> SPAWN PILLAR
      rite.alt when exists(aspect:funds>=1 | aspect:winter) -> SPAWN PILLAR
      rite.dotted when exists(aspect:funds.purse>=1) -> SPAWN PILLAR
      default -> NARRATE \"done.\"
",
    );
    let w = warnings_of(&findings);
    assert!(w.is_empty(), "{w:?}");
}

// ---------------------------------------------------------------------------
// E1 — SPAWN of a non-prototype (the no-kind refusal, ERROR class)
// ---------------------------------------------------------------------------

#[test]
fn a_spawn_of_a_declared_entity_errors_advertising_the_prototype_twin() {
    let findings = lint_findings(
        "
prototypes:
  PURSE is resource with funds:1

verbs:
  MAKE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      make.lamp -> SPAWN PURSE
      make.vial -> SPAWN POT
",
    );
    let hits: Vec<&LintFinding> =
        findings.iter().filter(|f| f.message.contains("SPAWN POT")).collect();
    assert_eq!(hits.len(), 1, "{findings:?}");
    assert_eq!(hits[0].severity, LintSeverity::Error);
    assert_eq!(
        hits[0].message,
        "recipe make.vial (MAKE): SPAWN POT names a declared entity, not a prototype — the spawn has no kind to mint (prototype or kind:NAME required) and refuses at play every time; declare a prototype (or a `_CARD` twin) and spawn that (§4.2)"
    );
    // the prototype spawn beside it stays clean
    assert!(findings.iter().all(|f| !f.message.contains("SPAWN PURSE")), "{findings:?}");
}

#[test]
fn a_spawn_of_an_aspect_less_entity_still_errors_the_refusal_is_identical() {
    // MARCH declares no aspects, but the SPAWN refuses at play all the
    // same — there is no bare-mint idiom to protect
    let findings = lint_findings(
        "
verbs:
  MAKE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      make.token -> SPAWN MARCH
",
    );
    let hits: Vec<&LintFinding> =
        findings.iter().filter(|f| f.message.contains("SPAWN MARCH")).collect();
    assert_eq!(hits.len(), 1, "{findings:?}");
    assert_eq!(hits[0].severity, LintSeverity::Error);
    assert!(
        hits[0].message.contains("has no kind to mint (prototype or kind:NAME required)"),
        "{findings:?}"
    );
}

#[test]
fn a_spawn_of_an_entity_inside_an_if_arm_or_a_trigger_is_still_found() {
    let findings = lint_findings(
        "
verbs:
  MAKE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      make.arm when count(actors) >= 1 -> IF count(actors) >= 1 -> (SPAWN POT)

triggers:
  t1 at tick 3 -> SPAWN POT
",
    );
    let hits: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.message.split(':').next())
        .filter(|o| o.starts_with("recipe ") || o.starts_with("trigger "))
        .collect();
    assert_eq!(hits.len(), 2, "{findings:?}");
    assert!(
        findings.iter().any(|f| f.message.starts_with("recipe make.arm (MAKE): SPAWN POT")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|f| f.message.starts_with("trigger t1: SPAWN POT")),
        "{findings:?}"
    );
}

#[test]
fn a_spawn_produced_by_an_op_or_function_expansion_is_still_found() {
    // op and function bodies expand into the recipe outcomes at compile
    // (§10), so the finding names the CALLING recipe, per owner and name
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

ops:
  MINT -> SPAWN POT

functions:
  forge -> SPAWN POT

verbs:
  MAKE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      make.via_op -> MINT()
      make.via_fn -> forge()
",
    );
    assert!(
        findings.iter().any(|f| f.message.starts_with("recipe make.via_op (MAKE): SPAWN POT")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|f| f.message.starts_with("recipe make.via_fn (MAKE): SPAWN POT")),
        "{findings:?}"
    );
    assert_eq!(
        findings.iter().filter(|f| f.message.contains("SPAWN POT")).count(),
        2,
        "{findings:?}"
    );
}

#[test]
fn a_repeated_entity_spawn_in_one_recipe_reports_once() {
    let findings = lint_findings(
        "
verbs:
  MAKE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      make.many -> SPAWN POT, SPAWN POT, SPAWN POT
",
    );
    let hits: Vec<&LintFinding> =
        findings.iter().filter(|f| f.message.contains("SPAWN POT")).collect();
    assert_eq!(hits.len(), 1, "{findings:?}");
}

// ---------------------------------------------------------------------------
// §6.7 — the dead-chain warning: a chain-op list whose every candidate's
// target `when` is provably dead can never pin
// ---------------------------------------------------------------------------

/// The dead-chain needles: the RUN list's own warning, distinct from the E11
/// never-matching-`when` warnings the dead TARGET recipes carry themselves.
fn dead_chain_warnings(findings: &[LintFinding]) -> Vec<&LintFinding> {
    findings
        .iter()
        .filter(|f| f.severity == LintSeverity::Warning && f.message.contains("can never pin"))
        .collect()
}

/// dead1/dead2: provably false for every §6.6 selection binding over the
/// 1..1 actors slot (NOT of a slot-always-true atom) — and neither atom is a
/// count-gate error, so the deadness surfaces only through the E11
/// never-matching warnings. The deadness RIDES THE BINDING COUNT (the
/// negation holds over an empty role), so it does not warn as a dead chain:
/// the gate's actual domain reads fewer bindings than the slot's selection
/// range (empty from a trigger, dead-ref-dropping from a parent overlay).
const DEAD_CANDIDATES: &str = "      dead1 when NOT (count(actors) >= 1) -> NARRATE \"dead1\"\n      dead2 when NOT (count(actors) = 1) -> NARRATE \"dead2\"";

/// starved: dead under EVERY binding count — over the 1..1 actors slot AND
/// over the empty-binding domain (count 0) — the deadness the chain warning
/// demands of a recipe-owned list, and all it demands of a trigger-owned one.
const COUNT_DEAD_CANDIDATES: &str = "      starved when count(actors) >= 2 -> NARRATE \"starved\"";

#[test]
fn a_recipe_chain_whose_deadness_rides_the_binding_count_stays_silent() {
    // the NOT-count face: dead at the §6.1 selection range, but the gate's
    // merged bindings can read fewer than the slot's selection count (dead
    // refs drop, §6.7) — the warning stands down
    let findings = findings_for(&format!(
        "
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent when count(actors) >= 1 -> RUN \"dead1\", \"dead2\"
{DEAD_CANDIDATES}
"
    ));
    assert!(dead_chain_warnings(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_recipe_chain_dead_under_every_binding_count_warns() {
    // the projected-overlay face: the deadness survives the empty-binding
    // domain too, so the merged gate can never satisfy it either
    let findings = findings_for(&format!(
        "
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent when count(actors) >= 1 -> RUN \"starved\"
{COUNT_DEAD_CANDIDATES}
"
    ));
    let hits = dead_chain_warnings(&findings);
    assert_eq!(hits.len(), 1, "{findings:?}");
    assert_eq!(
        hits[0].message,
        "recipe parent: the RUN list can never pin — every candidate's when is dead (§6.7)"
    );
}

#[test]
fn one_live_candidate_silences_the_dead_chain_warning() {
    // a mixed list stays silent — one live candidate is enough
    let findings = findings_for(&format!(
        "
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent when count(actors) >= 1 -> RUN \"dead1\", \"live\"
      live when count(actors) = 1 -> NARRATE \"live\"
{DEAD_CANDIDATES}
"
    ));
    assert!(dead_chain_warnings(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_world_gated_candidate_silences_the_dead_chain_warning() {
    // `exists` is a world read — Unknown at load, never provably dead
    let findings = findings_for(&format!(
        "
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent when count(actors) >= 1 -> RUN \"dead1\", \"maybe\"
      maybe when exists(name:HOUSE) -> NARRATE \"maybe\"
{DEAD_CANDIDATES}
"
    ));
    assert!(dead_chain_warnings(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_when_less_candidate_and_an_if_hidden_list_are_live() {
    // a when-less default candidate is always live; a chain op inside an IF
    // branch is still seen (the compiled walk)
    let findings = findings_for(&format!(
        "
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent when count(actors) >= 1 -> IF count(actors) >= 1 -> (RUN \"dead1\", \"fallback\")
      fallback -> NARRATE \"fallback\"
{DEAD_CANDIDATES}
"
    ));
    assert!(dead_chain_warnings(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_trigger_chain_list_dead_only_at_the_selection_bindings_stays_silent() {
    // the NOT-count face from a trigger: the gate reads the EMPTY bindings
    // (§6.6), count = 0, the negation holds — the candidate PINS at play, so
    // the warning stands down (the §6.1 selection range is not the gate's
    // domain here)
    let findings = findings_for(&format!(
        "
triggers:
  dusk at tick 3 -> FORK \"dead1\", \"dead2\"

verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent -> NARRATE \"p\"
{DEAD_CANDIDATES}
"
    ));
    assert!(dead_chain_warnings(&findings).is_empty(), "{findings:?}");
}

#[test]
fn a_trigger_chain_list_dead_over_the_empty_bindings_warns() {
    // the trigger-channel face: the empty-binding domain IS the gate's
    // domain there — deadness over it is exactly can-never-pin
    let findings = findings_for(&format!(
        "
triggers:
  dusk at tick 3 -> FORK \"starved\"

verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      parent -> NARRATE \"p\"
{COUNT_DEAD_CANDIDATES}
"
    ));
    let hits = dead_chain_warnings(&findings);
    assert_eq!(hits.len(), 1, "{findings:?}");
    assert_eq!(
        hits[0].message,
        "trigger dusk: the FORK list can never pin — every candidate's when is dead (§6.7)"
    );
}

#[test]
fn an_open_table_sum_gate_already_met_by_load_bearers_warns() {
    // POT bears funds 3 from load: the world sum is a floor already at 3 —
    // `total(table, aspect:funds) >= 3` (and the strict `> 2`) can never
    // gate. `>= 4` the load-present sum does not meet: silent.
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when total(table, aspect:funds) >= 3 -> SPAWN PILLAR
      rite.strict when total(table, aspect:funds) > 2 -> SPAWN PILLAR
      rite.unmet when total(table, aspect:funds) >= 4 -> SPAWN PILLAR
      rite.closed when total(table, aspect:funds) <= 3 -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 2, "{findings:?}");
    assert!(
        w.iter().all(|f| f.message.contains(
            "is always true — aspect 'funds' is borne from load totaling 3, so the gate can never gate (§6.3)"
        )),
        "{w:?}"
    );
    assert!(w.iter().any(|f| f.message.contains("rite.gate")), "{w:?}");
    assert!(w.iter().any(|f| f.message.contains("rite.strict")), "{w:?}");
}

#[test]
fn a_flipped_operand_table_sum_normalizes_before_the_op_check() {
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.flipped when 3 <= total(table, aspect:funds) -> SPAWN PILLAR
",
    );
    let w = warnings_of(&findings);
    assert_eq!(w.len(), 1, "{findings:?}");
    assert!(w[0].message.contains("total(table, aspect:funds) >= 3 is always true"), "{w:?}");
}

#[test]
fn a_killed_bearer_stands_a_table_sum_out_of_the_finding() {
    // BANISH destroys POT: the world sum can genuinely drop — the warning
    // stands down
    let script = "
script: Probe
player: MARCH
start: 0:0

entities:
  MARCH is character
  POT is resource with funds:3

prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when total(table, aspect:funds) >= 3 -> SPAWN PILLAR
  BANISH:
    actors: 1 kind:character
    duration: 0m
    recipes:
      banish.pot -> DESTROY POT
";
    let dict = core_dictionary();
    let findings = lint_script_findings(script, &dict).unwrap();
    assert!(warnings_of(&findings).is_empty(), "{findings:?}");
}

#[test]
fn the_bound_sum_scope_is_window_local_and_never_warns() {
    // total(bound, …) reads the step's bindings — the window can genuinely
    // gate — so the always-true walk stays silent on it
    let findings = lint_findings(
        "
prototypes:
  PILLAR is item

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when total(bound, aspect:funds) >= 1 -> SPAWN PILLAR
",
    );
    assert!(warnings_of(&findings).is_empty(), "{findings:?}");
}

// ---------------------------------------------------------------------------
// the held faces (v0.9.3, §6.1, §6.3, §6.4)
// ---------------------------------------------------------------------------

/// A recipe reading the held scope on a verb that declares no held slot is
/// dead content — the scope is ALWAYS empty there (no HOLD step can bind).
/// Every read face is caught: the count, the path, the total, the
/// quantifier, the rel pair.
#[test]
fn a_held_reading_recipe_without_a_held_slot_is_an_error() {
    let findings = findings_for(
        "
prototypes:
  QUESTY is item with quest:1

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.count when count(held) >= 1 -> NARRATE \"dead.\"
      rite.path when held.aspect.quest >= 1 -> NARRATE \"dead.\"
      rite.total when total(held, aspect:quest) >= 1 -> NARRATE \"dead.\"
      rite.any when any(held, aspect:quest >= 1) -> NARRATE \"dead.\"
      rite.rel when rel(neighbor, held) -> NARRATE \"dead.\"
      rite.relc when rel(neighbor, held.container, actors) -> NARRATE \"dead.\"
      default -> NARRATE \"live.\"
",
    );
    let held_errors: Vec<&LintFinding> = findings
        .iter()
        .filter(|f| f.severity == LintSeverity::Error && f.message.contains("reads the held scope"))
        .collect();
    assert_eq!(held_errors.len(), 6, "{findings:?}");
    for id in ["rite.count", "rite.path", "rite.total", "rite.any", "rite.rel", "rite.relc"] {
        let f = held_errors
            .iter()
            .find(|f| f.message.contains(id))
            .unwrap_or_else(|| panic!("{id} is missing from the findings: {findings:?}"));
        assert!(
            f.message.contains("reads the held scope")
                && f.message.contains("declares no held slot"),
            "{f:?}"
        );
    }
    // the default (held-free) recipe stays clean
    assert!(findings.iter().all(|f| !f.message.contains("default")), "{findings:?}");
    // the deadness errors stand E11 down — one root cause, one finding:
    // no never-matching warning re-reports what the held ERROR named
    assert!(warnings_of(&findings).is_empty(), "{findings:?}");
}

/// The advisory face: a verb declaring a held slot no recipe ever reads —
/// no hold can flip the deferred selection. A single held-reading when
/// silences it.
#[test]
fn a_held_slot_no_recipe_reads_is_an_advisory_warning() {
    let findings = findings_for(
        "
verbs:
  DEAD:
    actors: 1 kind:character
    held: 0..1 kind:item
    duration: 0m
    recipes:
      dead.a -> NARRATE \"one.\"
      dead.b when count(actors) >= 1 -> NARRATE \"two.\"
  LIVE:
    actors: 1 kind:character
    held: 0..1 kind:item
    duration: 0m
    recipes:
      live.flip when count(held) >= 1 -> NARRATE \"flipped.\"
      default -> NARRATE \"plain.\"
",
    );
    let warnings = warnings_of(&findings);
    assert_eq!(warnings.len(), 1, "{findings:?}");
    assert!(
        warnings[0].message.contains("verb DEAD")
            && warnings[0].message.contains("no recipe reads it"),
        "{findings:?}"
    );
    assert!(findings.iter().all(|f| !f.message.contains("LIVE")), "{findings:?}");
}

/// The held ERROR face extends into recipe BODIES: a held-reading IF
/// condition on a verb that declares no held slot is the same dead content —
/// the held-reading arm can never take. The when-face leaves a live `when`
/// alone, and the held-capable twin's IF stays clean.
#[test]
fn a_held_reading_if_condition_on_a_held_less_verb_is_an_error() {
    let findings = findings_for(
        "
prototypes:
  QUESTY is item with quest:1

verbs:
  RITE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      rite.gate when count(actors) >= 1 -> IF count(held) >= 1 -> (NARRATE \"dead arm.\") ELSE -> (NARRATE \"live arm.\")
      default -> NARRATE \"plain.\"

  RITE2:
    actors: 1 kind:character
    held: 0..1 kind:item
    duration: 0m
    recipes:
      rite2.gate when count(actors) >= 1 -> IF count(held) >= 1 -> (NARRATE \"arrangeable.\") ELSE -> (NARRATE \"without.\")
",
    );
    let errors: Vec<&LintFinding> =
        findings.iter().filter(|f| f.severity == LintSeverity::Error).collect();
    assert_eq!(errors.len(), 1, "{findings:?}");
    assert!(
        errors[0].message.contains("rite.gate")
            && errors[0].message.contains("an IF condition reads the held scope")
            && errors[0].message.contains("declares no held slot"),
        "{findings:?}"
    );
    // the held-capable twin's IF is arrangeable content, not dead
    assert!(findings.iter().all(|f| !f.message.contains("rite2")), "{findings:?}");
    // the when itself (`count(actors) >= 1` over 1..1) is live — the E11
    // detector stays silent (RITE2's advisory held-slot warning is the
    // separate when-face, expected here)
    assert!(
        findings.iter().all(
            |f| !(f.severity == LintSeverity::Warning && f.message.contains("can never match"))
        ),
        "{findings:?}"
    );
}

/// The count-cardinality family reads the held slot's 0..1 like any slot:
/// `count(held) >= 2` can never hold (the count-gate ERROR), `>= 1` can
/// (silent — the hold is arrangeable).
#[test]
fn the_held_count_gate_reads_the_0_1_cardinality() {
    let findings = findings_for(
        "
verbs:
  RITE:
    actors: 1 kind:character
    held: 0..1 kind:item
    duration: 0m
    recipes:
      rite.dead when count(held) >= 2 -> NARRATE \"impossible.\"
      rite.live when count(held) >= 1 -> NARRATE \"arrangeable.\"
      default -> NARRATE \"plain.\"
",
    );
    let errors: Vec<&LintFinding> =
        findings.iter().filter(|f| f.severity == LintSeverity::Error).collect();
    assert_eq!(errors.len(), 1, "{findings:?}");
    assert!(
        errors[0].message.contains("rite.dead")
            && errors[0].message.contains("count(held) >= 2 can never hold")
            && errors[0].message.contains("takes 0..1"),
        "{findings:?}"
    );
    assert!(findings.iter().all(|f| !f.message.contains("rite.live")), "{findings:?}");
}

/// The held slot takes exactly 0..1 (§6.1) — any other cardinality is a
/// load error naming the verb, never a slot the hold step could fill.
#[test]
fn the_held_slot_cardinality_is_exactly_0_1() {
    for line in ["held: 1", "held: 0..2", "held: 1..", "held: 2"] {
        assert_err_msg(
            compile(&base(&format!(
                "
verbs:
  RITE:
    actors: 1 kind:character
    {line} kind:item
    duration: 0m
    recipes:
      default -> NARRATE \"x.\"
"
            ))),
            r"the held slot takes exactly '0\.\.1'",
        );
    }
    // the legal spelling compiles clean
    assert!(compile(&base(
        "
verbs:
  RITE:
    actors: 1 kind:character
    held: 0..1 kind:item
    duration: 0m
    recipes:
      default -> NARRATE \"x.\"
"
    ))
    .is_ok());
}
