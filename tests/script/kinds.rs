//! Script-definable kinds (§4.1, v0.7.5): the `kinds:` section registers new
//! noun classes into the load-effective dictionary, and every vocabulary
//! check site — entity lines, prototype lines, slot queries, `kind:` terms
//! inside predicates — reads that dictionary. Declaration order is the
//! error-message/suggestion order; duplicates, built-in re-declarations, and
//! relation-type collisions are loud load errors.

use chorus::content::script::{compile_script, parse_script};
use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, TurnKind};

use crate::common::harness::assert_err_msg;
use crate::common::rig::auto_settle_line;

/// The custom-kind acceptance rig: one declared kind used everywhere a kind
/// word is read — entity lines, a stackable prototype, a slot query, a
/// count(kind:…) when — plus one card of the custom kind minted at play.
const OMEN_SCRIPT: &str = "
script: The Omen Ledger
player: ROWAN
start: 0:0

kinds:
  omen

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_TALLY is omen with marks:2 at HOUSE
  THE_PORTENT is omen with marks:3 at HOUSE

prototypes:
  BAD_OMEN is omen stackable with marks:3

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:omen
    recipes:
      study.portents hidden when object.aspect.marks >= 3 -> REFUSE(\"The marks swim like minnows.\")
      default -> NARRATE \"{actors} study {object}.\"
  TALK:
    actors: 1 kind:character
    recipes:
      talk.dread hidden when count(kind:omen) >= 1 -> SPAWN BAD_OMEN, NARRATE \"The dread spreads.\"

templates:
  STUDY says \"{actors} study {object}.\"
  TALK says \"{actors} feel the dread spread.\"
";

#[test]
fn a_kinds_section_registers_its_names_into_the_parsed_and_compiled_world() {
    let dict = core_dictionary();
    // the parsed record: section order, on the raw face
    let script = parse_script(OMEN_SCRIPT, &dict).unwrap();
    let kinds: Vec<&str> = script.content.kinds.iter().map(String::as_str).collect();
    assert_eq!(kinds, ["omen"]);
    // the compiled world accepts the kind at every check site: entity lines
    // and the stackable prototype declared above, the slot query
    // `object: 1 kind:omen`, and the count(kind:omen) when (all would have
    // been load errors against the bare core dictionary)
    let game = compile_script(OMEN_SCRIPT, &dict).unwrap();
    assert_eq!(game.state.by_name["THE_TALLY"].len(), 1);
    assert_eq!(game.state.by_name["THE_PORTENT"].len(), 1);
    // the load hands back the effective dictionary: built-ins first, then
    // the declaration — the suggestion order every unknown-kind error joins
    assert_eq!(
        game.dict.kinds_list(),
        "character, place, item, text, idea, state, rite, resource, omen"
    );
}

#[test]
fn a_custom_kind_card_plays_refuses_and_undoes() {
    let dict = core_dictionary();
    let mut game = Game::from_script(OMEN_SCRIPT, dict).unwrap();
    let base = game.log.records.len();
    // TALK's when reads count(kind:omen) — true (two omens declared) — and
    // mints the custom-kind prototype at the window's completion; the lenient
    // front door resolves the refs against the same world
    let t = auto_settle_line(&mut game, "talk").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert_eq!(game.state.by_name["BAD_OMEN"].len(), 1, "the omen prototype minted");
    // §7 turn-boundary undo: the round-trip restores the pristine log and
    // world exactly
    while game.log.records.len() > base {
        game.undo().unwrap();
    }
    assert_eq!(game.log.records.len(), base, "the round-trip reaches the pristine log");
    assert!(!game.state.by_name.contains_key("BAD_OMEN"), "undo removes the minted omen");
    // the custom-kind slot query binds: studying a marks:2 omen takes the
    // default recipe
    let t = auto_settle_line(&mut game, "study the tally").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("study")), "{:?}", t.lines);
    let studied = game.log.records.len();
    // and studying the marks:3 omen REFUSEs — the authored gate over the
    // custom kind fires at the completion (§6.5: the attempt logs, nothing
    // else moves)
    let t = auto_settle_line(&mut game, "study the portent").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("swim")), "the refusal prose: {:?}", t.lines);
    assert!(game.log.records.len() > studied, "the attempt logs");
    while game.log.records.len() > studied {
        game.undo().unwrap();
    }
    assert_eq!(game.log.records.len(), studied, "undo reopens the book unchanged");
}

#[test]
fn duplicate_kind_declarations_are_load_errors() {
    let dict = core_dictionary();
    assert_err_msg(
        parse_script("kinds:\n  omen\n  omen\n", &dict),
        r"line 3: kind 'omen' is already registered",
    );
    // a second kinds: section re-declaring across sections fails the same way
    assert_err_msg(
        parse_script("kinds:\n  omen\nkinds:\n  omen\n", &dict),
        r"line 4: kind 'omen' is already registered",
    );
}

#[test]
fn redeclaring_a_builtin_kind_is_a_load_error() {
    // uniform with every §10 duplicate rule: the built-ins are dictionary
    // state, and a re-declaration would silently reorder nothing but blur
    // authorship — fail loud instead
    let dict = core_dictionary();
    assert_err_msg(
        parse_script("kinds:\n  character\n", &dict),
        r"line 2: kind 'character' is already registered",
    );
}

#[test]
fn a_kind_colliding_with_a_relation_type_is_a_load_error() {
    // the entity line dispatches relation types BEFORE kinds (decls.rs), so
    // a kind named like a registered relation type could never be declared
    // on an entity line — reject it at the kinds: line instead
    let dict = core_dictionary();
    assert_err_msg(
        parse_script("kinds:\n  south\n", &dict),
        r"line 2: kind 'south' collides with a relation type",
    );
    assert_err_msg(
        parse_script("kinds:\n  neighbor\n", &dict),
        r"line 2: kind 'neighbor' collides with a relation type",
    );
}

#[test]
fn bad_kind_names_are_load_errors() {
    // the naming rule is `^[a-z][a-z0-9_]*$` — UPPER_SNAKE is the verb/name
    // namespace (§5.1), a leading digit is no identifier at all
    let dict = core_dictionary();
    assert_err_msg(
        parse_script("kinds:\n  Omen\n", &dict),
        r"line 2: kind 'Omen' must be lower_snake",
    );
    assert_err_msg(
        parse_script("kinds:\n  3rd_kind\n", &dict),
        r"line 2: kind '3rd_kind' must be lower_snake",
    );
    assert_err_msg(
        parse_script("kinds:\n  has space\n", &dict),
        r"line 2: kind 'has space' must be lower_snake",
    );
}

#[test]
fn the_unknown_kind_error_lists_the_declared_vocabulary() {
    let dict = core_dictionary();
    // declared kinds join the suggestion list in declaration order — the
    // byte-exact dynamic-vocabulary pin
    assert_err_msg(
        parse_script("kinds:\n  omen\nentities:\n  X is dragon\n", &dict),
        "line 4: unknown kind 'dragon' \\(expected one of character, place, item, text, idea, state, rite, resource, omen\\)",
    );
    // use before declaration is the ordinary unknown-kind load error (the
    // same declare-then-use order containers already follow, §10): the
    // kinds: section below has not been read when the entity line parses
    assert_err_msg(
        parse_script("entities:\n  X is omen\nkinds:\n  omen\n", &dict),
        "line 2: unknown kind 'omen' \\(expected one of character, place, item, text, idea, state, rite, resource\\)",
    );
    // the prototype form keeps its terse shape
    assert_err_msg(
        parse_script("kinds:\n  omen\nprototypes:\n  X is dragon\n", &dict),
        r"line 4: unknown kind 'dragon'",
    );
}

#[test]
fn the_kinds_section_grammar_is_loud() {
    let dict = core_dictionary();
    // the body is indent-2
    assert_err_msg(
        parse_script("kinds:\nomen\n", &dict),
        r"line 2: kinds expect an indent-2 kind name, got 'omen'",
    );
    assert_err_msg(
        parse_script("kinds:\n   omen\n", &dict),
        r"line 2: kinds expect an indent-2 kind name, got 'omen'",
    );
    // one name per line
    assert_err_msg(
        parse_script("kinds:\n  omen crow\n", &dict),
        r"line 2: kind 'omen crow' must be lower_snake",
    );
    // and the section is not a meta key: a bare `kinds:` header with no
    // body parses to nothing declared (an empty section is legal, like
    // every other section)
    let script = parse_script("kinds:\nentities:\n  X is item\n", &dict).unwrap();
    assert!(script.content.kinds.is_empty());
}
