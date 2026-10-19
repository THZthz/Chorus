//! The `desc "…"` card-description attribute (§4.2): grammar on the entity
//! and prototype lines (escapes per the one quoted-string discipline, §5.1),
//! the compiled description table, the mint-time copy onto the card face,
//! and the `inspect` render. Display text only — no engine semantics,
//! nothing logged, the lint stays silent on brace-looking prose.

use chorus::content::script::{compile_script, lint_script_findings};
use chorus::core::dict::core_dictionary;
use chorus::core::types::clone_state;
use chorus::engine::fold::fold;
use chorus::session::game::Game;

use crate::common::harness::assert_err_msg;
use crate::common::rig::{auto_settle_line, settle_line};
use crate::common::snap::snap;

/// The shared base script. Byte-shape preserved (leading newline, section
/// order) because line numbers flow into load-error messages.
fn base(extra: &str) -> String {
    format!(
        "
script: Desc Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE
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

// ------------------------------- grammar -----------------------------------

#[test]
fn desc_parses_on_entity_and_prototype_lines() {
    let c = compile(&base(
        "  THE_CHALICE is item with quest:1 desc \"a dented cup\"\n\nprototypes:\n  CHALICE is item stackable desc \"a dented cup\" with quest:1",
    ))
    .unwrap();
    let chalice = c.state.entities.get("s_the_chalice").unwrap();
    assert_eq!(chalice.desc.as_deref(), Some("a dented cup"));
    assert_eq!(chalice.aspects.get("quest"), Some(&chorus::core::types::AspectVal::Num(1)));
    let proto = c.content.prototypes.get("CHALICE").unwrap();
    assert_eq!(proto.desc.as_deref(), Some("a dented cup"));
    assert!(proto.stackable);
    // the compiled description table carries both faces
    assert_eq!(c.content.mint_desc("THE_CHALICE"), Some("a dented cup"));
    assert_eq!(c.content.mint_desc("CHALICE"), Some("a dented cup"));
    // the with-clause may trail the desc attribute too
    let c2 = compile(&base("  CUP is item desc \"small\" with quest:2")).unwrap();
    let cup = c2.state.entities.get("s_cup").unwrap();
    assert_eq!(cup.desc.as_deref(), Some("small"));
    assert_eq!(cup.aspects.get("quest"), Some(&chorus::core::types::AspectVal::Num(2)));
}

#[test]
fn desc_absent_is_none_everywhere() {
    let c = compile(&base("  PLAIN is item\n\nprototypes:\n  PLAINER is item")).unwrap();
    assert_eq!(c.state.entities.get("s_plain").unwrap().desc, None);
    assert_eq!(c.content.prototypes.get("PLAINER").unwrap().desc, None);
    assert_eq!(c.content.mint_desc("PLAIN"), None);
    assert_eq!(c.content.mint_desc("PLAINER"), None);
}

#[test]
fn desc_escapes_decode_like_narrate_strings() {
    // the one quoted-string discipline (§5.1): \" is a quote, \\ a backslash,
    // an unknown escape keeps the literal next unit
    let c = compile(&base("  X is item desc \"a \\\"b\\\" c\\\\d\\e\"")).unwrap();
    assert_eq!(c.state.entities.get("s_x").unwrap().desc.as_deref(), Some("a \"b\" c\\de"));
}

#[test]
fn desc_rejects_empty_double_and_unterminated_strings() {
    // blank-after-trim is the endings' blank-title loudness — a load error
    assert_err_msg(
        compile(&base("  X is item desc \"\"")),
        "desc expects one non-blank quoted string",
    );
    assert_err_msg(compile(&base("  X is item desc \" \"")), "non-blank quoted string");
    // one desc per line: the quoted capture runs to the line's LAST quote,
    // so a second desc clause lands inside the first one's capture — loud
    assert_err_msg(
        compile(&base("  X is item desc \"a\" desc \"b\"")),
        "desc expects one non-blank quoted string",
    );
    assert_err_msg(
        compile(&base("  X is item desc \"a\" with quest:1 desc \"b\"")),
        "desc expects one non-blank quoted string",
    );
    assert_err_msg(compile(&base("  X is item desc \"a")), "could not parse entity line");
}

#[test]
fn desc_before_the_clauses_parses_even_with_keyword_text() {
    // §10 promises the trailing clauses in either order — the desc clause is
    // spliced out before the scanners EVERY iteration, so keyword-looking
    // text inside the string (`found with care`, `went in here`) never feeds
    // the with/at-in scanners, tail position or not
    let c = compile(&base("  CUP is item desc \"found with care\" with quest:1")).unwrap();
    let cup = c.state.entities.get("s_cup").unwrap();
    assert_eq!(cup.desc.as_deref(), Some("found with care"));
    assert_eq!(cup.aspects.get("quest"), Some(&chorus::core::types::AspectVal::Num(1)));
    // the at-in scanner, same discipline — either order
    let c2 = compile(&base("  LAMP is item desc \"went in here\" at HOUSE")).unwrap();
    assert_eq!(c2.state.entities.get("s_lamp").unwrap().desc.as_deref(), Some("went in here"));
    let c3 = compile(&base("  DISH is item at HOUSE desc \"went in here\"")).unwrap();
    assert_eq!(c3.state.entities.get("s_dish").unwrap().desc.as_deref(), Some("went in here"));
}

#[test]
fn desc_before_deriving_parses_on_prototype_lines() {
    let c = compile(&base(
        "\nprototypes:\n  CUP is item desc \"a deriving tale\" deriving name from object",
    ))
    .unwrap();
    let proto = c.content.prototypes.get("CUP").unwrap();
    assert_eq!(proto.desc.as_deref(), Some("a deriving tale"));
    assert_eq!(proto.derive.as_ref().and_then(|d| d.name_from.as_deref()), Some("object"));
    // either order
    let c2 = compile(&base(
        "\nprototypes:\n  DISH is item deriving name from object desc \"a deriving tale\"",
    ))
    .unwrap();
    let dish = c2.content.prototypes.get("DISH").unwrap();
    assert_eq!(dish.desc.as_deref(), Some("a deriving tale"));
    assert_eq!(dish.derive.as_ref().and_then(|d| d.name_from.as_deref()), Some("object"));
}

#[test]
fn keyword_bearing_desc_in_tail_position_still_parses() {
    // the tail case the desc-first check always covered — kept working
    let c = compile(&base("  CUP is item with quest:1 desc \"found with care\"")).unwrap();
    let cup = c.state.entities.get("s_cup").unwrap();
    assert_eq!(cup.desc.as_deref(), Some("found with care"));
    assert_eq!(cup.aspects.get("quest"), Some(&chorus::core::types::AspectVal::Num(1)));
}

// ------------------------------ mint + face --------------------------------

const MINT_SCRIPT: &str = "
script: Mint Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE
  THE_DENTED_CUP is item desc \"a dented cup\" at HOUSE

prototypes:
  CHALICE is item stackable desc \"a dented cup\" with quest:1

verbs:
  SUMMON:
    actors: 1..1 kind:character
    recipes:
      default -> SPAWN CHALICE, NARRATE \"it appears\"
";

#[test]
fn inspect_renders_the_description_under_the_face() {
    let dict = core_dictionary();
    let game = Game::from_script(MINT_SCRIPT, dict).unwrap();
    assert_eq!(
        game.inspect("the dented cup"),
        vec![
            "THE DENTED CUP  [item]".to_string(),
            "  a dented cup".to_string(),
            "  at: HOUSE".to_string(),
        ]
    );
}

#[test]
fn a_card_without_desc_renders_exactly_the_pre_desc_lines() {
    let dict = core_dictionary();
    let game = Game::from_script(MINT_SCRIPT, dict).unwrap();
    // the whole face, pinned line for line — no desc, no desc line
    assert_eq!(game.inspect("house"), vec!["HOUSE  [place]".to_string()],);
}

#[test]
fn a_minted_card_carries_the_prototypes_desc() {
    let dict = core_dictionary();
    let mut game = Game::from_script(MINT_SCRIPT, dict).unwrap();
    // the outcomes fire at the window's completion (§6.7) — settle it
    let turn = auto_settle_line(&mut game, "summon").unwrap();
    assert_eq!(turn.kind, chorus::session::game::TurnKind::Action);
    let mint =
        game.state.entities.values().find(|e| e.name == "CHALICE").expect("the summon minted");
    assert_eq!(mint.desc.as_deref(), Some("a dented cup"));
    // and the mint's face shows it under the header
    let face = game.inspect("chalice");
    assert_eq!(face[0], "CHALICE  [item]");
    assert_eq!(face[1], "  a dented cup");
}

#[test]
fn a_drawn_card_carries_the_prototypes_desc() {
    let dict = core_dictionary();
    let script = "
script: Draw Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  CHALICE is item stackable desc \"a drawn chalice\"

decks:
  RELICS spec CHALICE

verbs:
  WAIT:
    actors: 1..1 kind:character
    recipes:
      default -> DRAW RELICS
";
    let mut game = Game::from_script(script, dict).unwrap();
    let turn = auto_settle_line(&mut game, "wait").unwrap();
    assert_eq!(turn.kind, chorus::session::game::TurnKind::Action);
    let drawn =
        game.state.entities.values().find(|e| e.name == "CHALICE").expect("the draw minted");
    assert_eq!(drawn.desc.as_deref(), Some("a drawn chalice"));
    let face = game.inspect("chalice");
    assert_eq!(face[0], "CHALICE  [item]");
    assert_eq!(face[1], "  a drawn chalice");
}

#[test]
fn a_replaced_card_carries_the_successors_desc() {
    let dict = core_dictionary();
    let script = "
script: Replace Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE
  OLD_CUP is item at HOUSE

prototypes:
  NEW_CUP is item desc \"a fresh cup\"

verbs:
  SUMMON:
    actors: 1..1 kind:character
    recipes:
      default -> REPLACE OLD_CUP WITH NEW_CUP
";
    let mut game = Game::from_script(script, dict).unwrap();
    let turn = auto_settle_line(&mut game, "summon").unwrap();
    assert_eq!(turn.kind, chorus::session::game::TurnKind::Action);
    assert!(
        !game.state.entities.values().any(|e| e.name == "OLD_CUP" && e.dead_action.is_none()),
        "the replaced card left play"
    );
    let mint =
        game.state.entities.values().find(|e| e.name == "NEW_CUP").expect("the replace minted");
    assert_eq!(mint.desc.as_deref(), Some("a fresh cup"));
    let face = game.inspect("new cup");
    assert_eq!(face[0], "NEW CUP  [item]");
    assert_eq!(face[1], "  a fresh cup");
}

#[test]
fn play_and_fold_agree_on_the_desc() {
    let c = compile(MINT_SCRIPT).unwrap();
    // the sentence form carries the SPAWN outcome; the fold replays the
    // concrete row and must recompute the same desc from the same name
    let r = settle_line(
        &c.content,
        &c.dict,
        clone_state(&c.state),
        "0:0 @ ANA SUMMON -> SPAWN CHALICE",
        1,
    )
    .unwrap();
    assert_eq!(r.rejected, None, "rejected: {:?}", r.rejected);
    let replayed = fold(&r.records, &clone_state(&c.state), &c.dict, &c.content).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state), "fold must land the same desc");
    let mint = r.state.entities.values().find(|e| e.name == "CHALICE").unwrap();
    assert_eq!(mint.desc.as_deref(), Some("a dented cup"));
}

#[test]
fn double_compile_lands_the_same_descriptions() {
    let a = compile(MINT_SCRIPT).unwrap();
    let b = compile(MINT_SCRIPT).unwrap();
    assert_eq!(a.content.descs, b.content.descs);
    let fa = Game::from_script(MINT_SCRIPT, core_dictionary()).unwrap();
    let fb = Game::from_script(MINT_SCRIPT, core_dictionary()).unwrap();
    assert_eq!(fa.inspect("the dented cup"), fb.inspect("the dented cup"));
}

// -------------------------------- lint -------------------------------------

#[test]
fn the_lint_stays_silent_on_brace_looking_desc_text() {
    // desc is literal display text, not a template — the {…} token scan
    // never sees it (§13.2)
    let findings = lint_script_findings(
        &base("  X is item desc \"see {actors} and {object} now\""),
        &core_dictionary(),
    )
    .unwrap();
    assert_eq!(findings, vec![], "no findings: {:?}", findings);
}
