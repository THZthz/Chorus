//! Script-definable verbs (§4.3, v0.7.6): the `verbs:` block declares
//! brand-new UPPER_SNAKE lexemes (or configures existing ones) into the
//! load-effective dictionary — the same per-load-clone pattern the `kinds:`
//! section established. Declaration is not playability: a declared verb is
//! playable only through its authored config block, reserved words never
//! register, and a verb lexeme may not share a name with a declared entity
//! or prototype (registration grows `reserved`, so the overlap is checked
//! order-independently over the whole load).

use chorus::content::script::{compile_script, parse_script};
use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, TurnKind};

use crate::common::harness::assert_err_msg;
use crate::common::rig::auto_settle_line;

/// The acceptance rig: one brand-new verb (slots, duration, two recipes — a
/// duty-gated break and a default), one CONFIGURED core verb (re-declaring a
/// core lexeme keeps working), and a template for the new verb.
const INTERROGATION_SCRIPT: &str = "
script: The Interrogation Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_GUARD is character with duty:2 at HOUSE
  THE_COOK is character with duty:0 at HOUSE
  THE_LEDGER is text at HOUSE

verbs:
  INTERROGATE:
    actors: 1 kind:character
    object: 1 kind:character
    duration: 30m
    recipes:
      interrogate.break hidden when object.aspect.duty >= 2 -> SET(OBJECT, aspect:broken, 1), NARRATE \"{object} breaks.\"
      default -> NARRATE \"{actors} question {object}.\"
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      default -> NARRATE \"{actors} read {object}.\"

templates:
  INTERROGATE says \"{actors} press {object}.\"
";

#[test]
fn a_script_declared_verb_registers_plays_suggests_and_round_trips() {
    let dict = core_dictionary();
    // the raw face: the header lands in ContentRaw.verbs like any configured
    // verb, and the parse no longer demands a pre-registered lexeme
    let script = parse_script(INTERROGATION_SCRIPT, &dict).unwrap();
    assert!(script.content.verbs.contains_key("INTERROGATE"));
    // the load hands back the effective dictionary with the declaration in it
    let compiled = compile_script(INTERROGATION_SCRIPT, &dict).unwrap();
    assert!(compiled.dict.is_verb("INTERROGATE"));
    assert!(compiled.dict.reserved.contains("INTERROGATE"));
    assert!(compiled.content.verbs.contains_key("INTERROGATE"));
    // the template for the declared verb compiled through the templates gate
    assert_eq!(
        compiled.content.templates.get("INTERROGATE").map(String::as_str),
        Some("{actors} press {object}.")
    );

    let mut game = Game::from_script(INTERROGATION_SCRIPT, dict).unwrap();
    // the banner's configured-verbs row and the try row read the extended
    // vocabulary (content-driven, so nothing to build — only to observe)
    let banner = game.banner().unwrap();
    assert!(
        banner.iter().any(|l| l.contains("verbs: interrogate")),
        "the banner lists the declared verb: {banner:?}"
    );
    let suggestions = game.suggestions().unwrap();
    assert!(
        suggestions.iter().any(|s| s.contains("interrogate")),
        "the try row suggests the declared verb: {suggestions:?}"
    );
    // typed lowercase at the lenient front door: the verb resolves against
    // the effective dictionary, the duty-gated recipe fires on THE_GUARD, the
    // window processes (the rig waits it out), and the BEGIN row logs the
    // canonical sentence
    let base = game.log.records.len();
    let t = auto_settle_line(&mut game, "interrogate the guard").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("breaks")), "{:?}", t.lines);
    assert!(
        game.log.records[base..].iter().any(|r| r.sentence.contains("INTERROGATE THE_GUARD")),
        "{:?}",
        game.log.records
    );
    let guard_id = &game.state.by_name["THE_GUARD"][0];
    assert!(
        game.state.entities[guard_id].aspects.get("broken").is_some(),
        "the break recipe landed"
    );
    // §7 turn-boundary undo: truncate + re-fold from the base — the recorded
    // rows re-parse against the same effective dictionary (fold round-trips a
    // script-declared verb), and the world lands back pristine
    while game.log.records.len() > base {
        game.undo().unwrap();
    }
    assert_eq!(game.log.records.len(), base, "undo reaches the pristine log");
    let guard_id = &game.state.by_name["THE_GUARD"][0];
    assert!(game.state.entities[guard_id].aspects.get("broken").is_none());
}

#[test]
fn a_registered_lexeme_with_no_authored_content_still_rejects_as_not_a_content_verb() {
    // the dictionary carries the lexeme (script one declared it) but script
    // two never authored its verbs: block — registration is not playability
    // (§4.3); the exec gate stays the backstop (the engine_core pin, via the
    // script path)
    let shared = Game::from_script(INTERROGATION_SCRIPT, core_dictionary()).unwrap().dict;
    let script_two = "
script: The Silent Twin
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_GUARD is character at HOUSE
";
    let mut game = Game::from_script(script_two, shared).unwrap();
    let t = game.handle_line("interrogate the guard").unwrap();
    assert_eq!(t.kind, TurnKind::Error, "{:?}", t.lines);
    assert!(
        t.lines.iter().any(|l| l.contains("INTERROGATE is not a content verb")),
        "{:?}",
        t.lines
    );
    assert!(game.log.records.is_empty(), "a rejection logs nothing");
}

#[test]
fn a_reserved_word_as_a_verb_header_is_a_load_error() {
    let dict = core_dictionary();
    // §5.1 (v0.7.6): the registration the load performs re-validates the
    // whole nameable grammar — a reserved word fails loud, naming the lexeme
    assert_err_msg(
        compile_script("verbs:\n  EVENT:\n", &dict),
        r"verb 'EVENT' is a reserved word \(§5\.1\)",
    );
    assert_err_msg(
        compile_script("verbs:\n  TO:\n", &dict),
        r"verb 'TO' is a reserved word \(§5\.1\)",
    );
    // the same load error through the plain parse-then-compile split the
    // lint uses
    assert_err_msg(
        compile_script("verbs:\n  COMPLETE:\n    duration: 5m\n", &dict),
        r"verb 'COMPLETE' is a reserved word \(§5\.1\)",
    );
}

#[test]
fn an_entity_named_like_a_declared_verb_is_a_load_error_in_either_section_order() {
    let dict = core_dictionary();
    // registration grows `reserved`, so the overlap would silently change how
    // the name formats and parses depending on which section came first —
    // the cross-check is order-independent, one message either way
    let entity_first = "
entities:
  SHOUT is item
verbs:
  SHOUT:
    recipes:
      default -> NARRATE \"ho.\"
";
    assert_err_msg(
        compile_script(entity_first, &dict),
        r"'SHOUT' is declared both as an entity and as a verb — a verb lexeme is a reserved word \(§5\.1\)",
    );
    let verb_first = "
verbs:
  SHOUT:
    recipes:
      default -> NARRATE \"ho.\"
entities:
  SHOUT is item
";
    assert_err_msg(
        compile_script(verb_first, &dict),
        r"'SHOUT' is declared both as an entity and as a verb — a verb lexeme is a reserved word \(§5\.1\)",
    );
    // the prototype face of the same rule
    assert_err_msg(
        compile_script(
            "prototypes:\n  SHOUT is item\nverbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\n",
            &dict,
        ),
        r"'SHOUT' is declared both as a prototype and as a verb — a verb lexeme is a reserved word \(§5\.1\)",
    );
}

#[test]
fn a_duplicate_verb_header_is_a_load_error_for_new_lexemes_too() {
    let dict = core_dictionary();
    // the parse-side uniform duplicate rule (§10) — unchanged for a lexeme
    // the dictionary has never seen
    assert_err_msg(
        compile_script(
            "verbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\n  SHOUT:\n",
            &dict,
        ),
        "line 5: duplicate verb declaration 'SHOUT'",
    );
}

#[test]
fn redeclaring_a_core_verb_configures_it_and_still_rejects_duplicates() {
    let dict = core_dictionary();
    // configuring READ rides the same register-if-unknown skip as before —
    // the lexeme is already registered, the block just configures it
    let game = compile_script(INTERROGATION_SCRIPT, &dict).unwrap();
    let read = game.content.verbs.get("READ").unwrap();
    assert!(read.slot_by_role.contains_key("object"), "the authored slot compiled in");
    assert_eq!(read.duration, 60, "the authored duration compiled in");
    // ... and the duplicate rule keeps covering the core lexemes
    assert_err_msg(
        compile_script("verbs:\n  READ:\n  READ:\n", &dict),
        "line 3: duplicate verb declaration 'READ'",
    );
}

#[test]
fn a_deck_id_named_like_a_declared_verb_is_a_load_error() {
    // registration grows `reserved`, and the deck-id check reads it — so the
    // id-vs-verb face of the same rule holds: a deck may not take a declared
    // verb lexeme's name (deck.rs's reserved check sees the extended set)
    let dict = core_dictionary();
    assert_err_msg(
        compile_script(
            "verbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\ndecks:\n  SHOUT spec ANYTHING\n",
            &dict,
        ),
        r"'SHOUT' is reserved and cannot be a deck id",
    );
    // ... in either section order (the deck table compiles after the
    // registration, whichever way the file lists them)
    assert_err_msg(
        compile_script(
            "decks:\n  SHOUT spec ANYTHING\nverbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\n",
            &dict,
        ),
        r"'SHOUT' is reserved and cannot be a deck id",
    );
}

#[test]
fn compiling_the_same_text_twice_against_one_dictionary_is_idempotent() {
    // the per-load clone: the caller's dictionary is never mutated, so two
    // loads against one base both succeed (and a load against a previous
    // load's effective output skips the known lexemes — register-if-unknown)
    let dict = core_dictionary();
    let first = compile_script(INTERROGATION_SCRIPT, &dict).unwrap();
    let second = compile_script(INTERROGATION_SCRIPT, &dict).unwrap();
    assert!(second.dict.is_verb("INTERROGATE"));
    assert!(!dict.is_verb("INTERROGATE"), "the base dictionary stays untouched");
    let third = compile_script(INTERROGATION_SCRIPT, &first.dict).unwrap();
    assert!(third.dict.is_verb("INTERROGATE"));
    // and the lint path — parse + compile against the same base — stays clean
    // for a script that declares its verbs (the findings walk reads the
    // effective dictionary from the compile)
    let findings =
        chorus::content::script::lint_script_findings(INTERROGATION_SCRIPT, &dict).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}
