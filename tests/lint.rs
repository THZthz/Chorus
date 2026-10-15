//! Lint (14 tests) — the compiler's load-time
//! discipline: error upgrades (things that used to compile and detonate
//! later) and the lint layer (warnings that never throw).

use chorus::content::script::{compile_script, lint_script};
use chorus::core::dict::core_dictionary;

mod common;
use common::harness::assert_err_msg;

/// The bundled showcase, read as text (script text = data).
const FLYER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/the-nightjar-flyer.txt"
));

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

fn compile(text: &str) -> Result<chorus::content::script::CompiledScript, chorus::error::ChorusError> {
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
    assert_err_msg(
        compile(&base("  WITH is character at HOUSE")),
        r"'WITH' is a reserved word",
    );
    assert_err_msg(
        compile(&base("  MARCH is parent_child of TO")),
        r"'TO' is a reserved word",
    );
    assert_err_msg(
        compile(&base("prototypes:\n  IF is idea")),
        r"'IF' is a reserved word",
    );
    // non-lexeme names are legal — §9.42's unsafe-name corner, quote every ref
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
    // Q4 fixed (spec §4.3's six-role list + §10's fail-loudly rule): the slot
    // key is validated against the role vocabulary at load; previously the
    // role was cast unvalidated and the slot compiled only to never match.
    // (The fixture trips exactly ONE error — the count and query are legal.)
    assert_err_msg(
        compile(&base(
            "verbs:\n  READ:\n    actrs: 1 kind:character\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"read.\"\n",
        )),
        r"line \d+: bad slot role 'actrs' \(expected one of actors, object, with, at, about, to\)",
    );
}

#[test]
fn duplicate_prototype_declarations_are_a_load_error_not_last_write_wins() {
    // Q5 fixed: prototypes used to silently overwrite (the later insert won);
    // they now error like entities/endings. The fixture trips exactly ONE
    // error — both lines are individually legal (no slot typo to double-fire
    // into Q4's check).
    assert_err_msg(
        compile(&base(
            "prototypes:\n  SPARK is idea\n  SPARK is idea with secret:1",
        )),
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
    // a repeated header used to silently RESET the block (a fresh empty
    // VerbContent replaced the configured one)
    assert_err_msg(
        compile(&base(
            "verbs:\n  READ:\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"one.\"\n  READ:\n    object: 1 kind:text\n    recipes:\n      default -> NARRATE \"two.\"\n",
        )),
        r"line \d+: duplicate verb declaration 'READ'",
    );
}

#[test]
fn case_colliding_entity_names_are_a_minted_id_collision_load_error() {
    // Q5's fourth half: names are case-exact (§9.14), so 'ana'/'ANA' pass
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
    // Q2 fixed (§6.3's grammar has no `@`): a `@3`-class typo used to be
    // silently swallowed by the predicate tokenizer — `secret >= @2` read
    // as `secret >= 2` and selected. The tokenizer now fails at load naming
    // the character and its offset. (The fixture trips exactly ONE error.)
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
    assert!(
        w.iter().any(|x| x.contains("read.late") && x.contains("unreachable")),
        "{w:?}"
    );
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
        w.iter()
            .any(|x| x.contains("read.second") && x.contains("shadowed by read.first")),
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
    assert!(
        w.iter()
            .any(|x| x.contains("read.default") && x.contains("{bject}")),
        "{w:?}"
    );
    assert!(
        !w.iter()
            .any(|x| x.contains("function sus") && x.contains("{louder}")),
        "{w:?}"
    ); // params are known in bodies
    assert!(
        w.iter()
            .any(|x| x.contains("trigger dusk") && x.contains("{louder}")),
        "{w:?}"
    );
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
    assert!(
        w.iter()
            .any(|x| x.contains("deck POST: entry LETTER is not stackable")),
        "{w:?}"
    );
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
    assert!(
        w.iter()
            .any(|x| x.contains("aspect 'ocult' is queried but never declared")),
        "{w:?}"
    );
}

#[test]
fn a_non_lexeme_entity_name_warns_legal_but_every_ref_needs_quotes() {
    let dict = core_dictionary();
    let w = lint_script(&base("  X-Y is item with weight:1"), &dict).unwrap();
    assert!(
        w.iter()
            .any(|x| x.contains("entity 'X-Y': not a name lexeme")),
        "{w:?}"
    );
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
        r#"bad clock 'banana' \(expected "day:halfhour"\)"#,
    );
}
