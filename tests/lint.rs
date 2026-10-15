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

fn compile(text: &str) -> Result<chorus::content::script::CompiledScript, chorus::js::ChorusError> {
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
