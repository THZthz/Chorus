//! Smoke tests for the session layer (lenient/render/game). Anchors
//! exercised here, all pinned by the acceptance suites:
//! - lenient: bare-verb default player, multi-word names, role-phrase groups,
//!   canonical passthrough, suggestion surfaces, case-insensitive names;
//!   E9's multi-role binding (distinct §4.2 pool copies head-first, the
//!   too-small-pool refusal, same-role doubling unchanged) and E6's bare
//!   clock halfhour wrap (the canonical form stays strict);
//! - Game: from_script → handle_line → journal/story/look → undo → bogus
//!   error, visible/inspect projection, suggestions with `s[0]` pinned,
//!   and BOTH game-over texts through a
//!   minimal endings script.

use crate::common::rig::auto_settle_line;

use chorus::core::dict::core_dictionary;
use chorus::engine::ExecOptions;
use chorus::session::game::{Game, TurnKind};
use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};

const FLYER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/the-nightjar-flyer.txt"));

// ------------------------------- lenient -----------------------------------

#[test]
fn lenient_routes_the_pinned_phrases() {
    let dict = core_dictionary();
    let game = Game::from_script(FLYER, dict).unwrap();
    let ctx = LenientCtx {
        dict: &game.dict,
        content: &game.content,
        state: &game.state,
        player: Some("MARCH"),
    };

    // 'bare verb defaults to the player character'
    let ParsedInput::Ok { step, .. } = parse_player_input("wait", &ctx) else {
        panic!("'wait' must parse");
    };
    assert_eq!(step.start_min, Some(42 * 30)); // the default begin minute is NOW
    let c0 = &step.clauses[0];
    assert_eq!(c0.verb, "WAIT");
    assert_eq!(c0.actors.len(), 1);
    assert!(
        matches!(&c0.actors[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "MARCH")
    );

    // 'multi-actor and multi-word names resolve'
    let ParsedInput::Ok { step, .. } = parse_player_input("march read the vane ledger", &ctx)
    else {
        panic!("multi-word name must parse");
    };
    let c0 = &step.clauses[0];
    assert_eq!(c0.verb, "READ");
    assert!(
        matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_VANE_LEDGER")
    );

    // 'role phrases and objects do not bleed into each other'
    let ParsedInput::Ok { step, .. } =
        parse_player_input("use on the inquest with the manifest at guard van", &ctx)
    else {
        panic!("role phrases must parse");
    };
    let c0 = &step.clauses[0];
    assert_eq!(c0.verb, "USE_ON");
    assert!(
        matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_INQUEST")
    );
    assert!(
        matches!(&c0.with[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_MANIFEST")
    );
    assert!(
        matches!(&c0.at, Some(chorus::core::r#ref::Ref::Name { name, .. }) if name == "GUARD_VAN")
    );

    // 'canonical passthrough still works'
    let ParsedInput::Ok { step, .. } = parse_player_input("0:44 @ MARCH WAIT", &ctx) else {
        panic!("passthrough must parse");
    };
    assert_eq!(step.clock, Some(chorus::core::clock::Clock { day: 0, halfhour: 44 }));

    // 'unknown words get suggestions' — the failed word is 'vane',
    // so the suggestion rides the normalized-prefix path
    let ParsedInput::Fail { suggestions, .. } = parse_player_input("read the vane ledgr", &ctx)
    else {
        panic!("'ledgr' must fail");
    };
    assert!(suggestions.contains(&"the vane ledger".to_string()), "{suggestions:?}"); // typeable form

    // 'verbs get suggestions too' — the whole line with the verb corrected,
    // so the suggestion can be typed back as it stands
    let ParsedInput::Fail { suggestions, .. } = parse_player_input("xeplore the car", &ctx) else {
        panic!("'xeplore' must fail");
    };
    assert!(suggestions.contains(&"explore the car".to_string()), "{suggestions:?}");

    // 'names resolve case-insensitively (spec §11)'
    assert!(parse_player_input("read THE VANE LEDGER", &ctx).is_ok());
}

// ------------------------- lenient: E9 multi-role binding -------------------

/// A state with a pooled name: two living THE_PSALM copies (birth order
/// `psalm_a`, `psalm_b`) plus one THE_CANDLE, so single-mention resolution
/// has a second name to stay byte-identical on.
fn pool_world() -> chorus::core::types::State {
    use chorus::core::types::{add_entity, new_state, AttrValue, Attrs, Entity};
    let mut s = new_state(0);
    for (id, kind, name, container, stackable) in [
        ("house", "place", "HOUSE", None, false),
        ("ana", "character", "ANA", Some("house"), false),
        ("psalm_a", "idea", "THE_PSALM", Some("house"), true),
        ("psalm_b", "idea", "THE_PSALM", Some("house"), true),
        ("candle", "item", "THE_CANDLE", Some("house"), false),
    ] {
        let mut attrs = Attrs::new();
        if let Some(c) = container {
            attrs.insert("container".to_string(), AttrValue::Str(c.to_string()));
        }
        add_entity(
            &mut s,
            Entity {
                id: id.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                aspects: Default::default(),
                attrs,
                born_action: None,
                dead_action: None,
                stackable,
            },
        )
        .unwrap();
    }
    s
}

fn pool_ctx<'a>(
    dict: &'a chorus::core::dict::Dictionary,
    content: &'a chorus::content::CompiledContent,
    state: &'a chorus::core::types::State,
) -> LenientCtx<'a> {
    LenientCtx { dict, content, state, player: Some("ANA") }
}

#[test]
fn one_name_in_two_roles_binds_distinct_pool_copies() {
    // E9 (spec §11): `read the psalm with the psalm` names the pool in two
    // roles — the object takes the §4.2 head as a name ref exactly as ever,
    // the with-role takes the NEXT living copy as an #id ref
    let dict = core_dictionary();
    let content =
        chorus::content::compile_content(&chorus::content::ContentRaw::default(), &dict, &[])
            .unwrap();
    let world = pool_world();
    let ParsedInput::Ok { step, .. } =
        parse_player_input("ana read the psalm with the psalm", &pool_ctx(&dict, &content, &world))
    else {
        panic!("two copies alive: the self-pair must parse");
    };
    let c0 = &step.clauses[0];
    assert!(matches!(&c0.actors[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "ANA"));
    assert!(
        matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_PSALM"),
        "the first role keeps the head name ref"
    );
    assert!(
        matches!(&c0.with[0], chorus::core::r#ref::Ref::Id { id } if id == "psalm_b"),
        "the second role binds the next living copy by id"
    );

    // authored role order is the hand-out order: the WITH phrase first still
    // leaves the head to the object (it comes first in the sentence)
    let ParsedInput::Ok { step, .. } = parse_player_input(
        "ana read the psalm about the candle with the psalm",
        &pool_ctx(&dict, &content, &world),
    ) else {
        panic!("distinct names must parse");
    };
    let c0 = &step.clauses[0];
    assert!(matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { .. }), "object is the head");
    assert!(
        matches!(&c0.with[0], chorus::core::r#ref::Ref::Id { id } if id == "psalm_b"),
        "the later role still takes the second copy"
    );
}

#[test]
fn a_pool_too_small_for_its_roles_refuses_by_name() {
    // E9: the refusal — one copy alive, two roles naming it (spec §11)
    let dict = core_dictionary();
    let content =
        chorus::content::compile_content(&chorus::content::ContentRaw::default(), &dict, &[])
            .unwrap();
    let mut world = pool_world();
    let dead = world.entities.get_mut("psalm_b").unwrap();
    dead.dead_action = Some(1);
    let ParsedInput::Fail { error, suggestions, .. } =
        parse_player_input("ana read the psalm with the psalm", &pool_ctx(&dict, &content, &world))
    else {
        panic!("one copy cannot fill two roles");
    };
    assert_eq!(error, "only one 'psalm' — you need two", "{error}");
    assert!(suggestions.is_empty(), "refusals carry no suggestions");

    // three roles naming a two-copy pool: the third comes up short
    let world = pool_world();
    let ParsedInput::Fail { error, .. } = parse_player_input(
        "ana read the psalm with the psalm about the psalm",
        &pool_ctx(&dict, &content, &world),
    ) else {
        panic!("two copies cannot fill three roles");
    };
    assert_eq!(error, "only two 'psalm' — you need three", "{error}");
}

#[test]
fn single_mentions_and_same_role_doubles_are_untouched() {
    // E9's zero-regression surface: a name in one role is the head name ref
    // (byte-identical), and a name doubled INSIDE one role still binds the
    // head twice — the engine's same-role placement refusal speaks, not the
    // lenient binder (spec §11)
    let dict = core_dictionary();
    let content =
        chorus::content::compile_content(&chorus::content::ContentRaw::default(), &dict, &[])
            .unwrap();
    let world = pool_world();
    let ParsedInput::Ok { step, .. } = parse_player_input(
        "ana read the psalm with the candle",
        &pool_ctx(&dict, &content, &world),
    ) else {
        panic!("single mentions must parse");
    };
    let c0 = &step.clauses[0];
    assert!(matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { .. }));
    assert!(matches!(&c0.with[0], chorus::core::r#ref::Ref::Name { .. }));

    let ParsedInput::Ok { step, .. } =
        parse_player_input("ana read the psalm the psalm", &pool_ctx(&dict, &content, &world))
    else {
        panic!("a doubled name still parses (the engine refuses it)");
    };
    let c0 = &step.clauses[0];
    assert_eq!(c0.objects.len(), 2);
    assert!(
        matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_PSALM")
    );
    assert!(
        matches!(&c0.objects[1], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_PSALM"),
        "same-role doubling re-uses the slot's head claim"
    );
}

// ------------------------- lenient: E6 bare-clock wrap ----------------------

#[test]
fn the_bare_lenient_clock_wraps_halfhour_overflow() {
    // E6 (spec §11): the lenient front door WRAPS an out-of-range halfhour
    // field the way durations compose — `0:118` is 118 halfhours = day 2,
    // halfhour 22 — where the CANONICAL `d:h @` passthrough keeps its frozen
    // loud `bad halfhour` bound. Per form: lenient wraps, canonical refuses.
    let dict = core_dictionary();
    let game = Game::from_script(FLYER, dict).unwrap();
    let ctx = LenientCtx {
        dict: &game.dict,
        content: &game.content,
        state: &game.state,
        player: Some("MARCH"),
    };

    let ParsedInput::Ok { step, .. } = parse_player_input("0:118", &ctx) else {
        panic!("0:118 must wrap");
    };
    assert_eq!(step.clock, Some(chorus::core::clock::Clock { day: 2, halfhour: 22 }));
    assert_eq!(step.start_min, Some((2 * 48 + 22) * 30));

    // 1:49 crosses the day too (1*48 + 49 = 97 halfhours = day 2, halfhour 1)
    let ParsedInput::Ok { step, .. } = parse_player_input("1:49", &ctx) else {
        panic!("1:49 must wrap");
    };
    assert_eq!(step.clock, Some(chorus::core::clock::Clock { day: 2, halfhour: 1 }));

    // the boundary itself folds exactly one day; the legal top of a day is
    // untouched
    let ParsedInput::Ok { step, .. } = parse_player_input("0:48", &ctx) else {
        panic!("0:48 must wrap");
    };
    assert_eq!(step.clock, Some(chorus::core::clock::Clock { day: 1, halfhour: 0 }));
    let ParsedInput::Ok { step, .. } = parse_player_input("0:47", &ctx) else {
        panic!("0:47 is legal");
    };
    assert_eq!(step.clock, Some(chorus::core::clock::Clock { day: 0, halfhour: 47 }));

    // the canonical passthrough keeps the strict bound — the frozen §5
    // discipline governs that form
    let ParsedInput::Fail { error, .. } = parse_player_input("0:118 @ MARCH WAIT", &ctx) else {
        panic!("the canonical form must refuse");
    };
    assert!(error.contains("bad halfhour"), "{error}");
}

// --------------------- lenient through the engine: the pair plays -----------

/// A two-verb rig for E9 at engine scale: EXPLORE mints two THE_PSALM copies
/// (a stackable pool), STUDY carries the 2+2 pair recipe that consumes
/// object and with and mints THE_GREAT_PSALM once.
const POOL_SCRIPT: &str = "
script: Pair Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  THE_PSALM is idea stackable with radiance:2
  THE_GREAT_PSALM is idea with radiance:6

verbs:
  EXPLORE:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> SPAWN THE_PSALM, SPAWN THE_PSALM, NARRATE \"The psalm comes apart into light, twice over.\"
  STUDY:
    actors: 1 kind:character
    object: 1 kind:idea
    with: 0..1 kind:idea
    duration: 1h
    recipes:
      study.pair when count(object) = 1 AND count(with) = 1 AND object.aspect.radiance = 2 AND total(with, aspect:radiance) = 2 -> DESTROY OBJECT, DESTROY WITH, SPAWN THE_GREAT_PSALM, NARRATE \"The pair reads as one.\"
      study.default -> NARRATE \"{actors} study {object}.\"

templates:
  EXPLORE says \"{actors} explore.\"
  STUDY says \"{actors} study {object}.\"
";

#[test]
fn the_self_pair_binds_both_copies_and_mints_once() {
    // E9 at engine scale (spec §11): the lenient self-pair hands the engine
    // DISTINCT ids — both fragments are consumed, the 6 mints once, and no
    // stray survives. Under the old head-only binding the pair fired on one
    // card and left the second copy alive.
    let dict = core_dictionary();
    let mut game = Game::from_script(POOL_SCRIPT, dict).unwrap();
    auto_settle_line(&mut game, "explore").unwrap();

    // two copies alive in birth order before the pair
    let psalms = &game.state.by_name["THE_PSALM"];
    assert_eq!(psalms.len(), 2, "the mint deals the pair");
    let head = psalms[0].clone();

    auto_settle_line(&mut game, "study the psalm with the psalm").unwrap();

    // THE decisive pin: NO living psalm remains — the engine saw two ids
    let alive = |game: &Game, name: &str| -> usize {
        game.state
            .by_name
            .get(name)
            .map(|ids| {
                ids.iter()
                    .filter(|id| {
                        game.state
                            .entities
                            .get(id.as_str())
                            .is_some_and(|e| e.dead_action.is_none())
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(alive(&game, "THE_PSALM"), 0, "both copies were consumed");
    assert_eq!(alive(&game, "THE_GREAT_PSALM"), 1, "minted exactly once");

    // the log carries the distinct binding: the begin row's with-role is an
    // #id ref (not the head's name ref), and it is not the object's id
    let begin = game
        .log
        .records
        .iter()
        .find(|r| r.kind == "action" && r.verb == "STUDY")
        .expect("the pair window began");
    assert!(
        matches!(&begin.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_PSALM"),
        "the first role keeps the head name ref"
    );
    match &begin.instruments[0] {
        chorus::core::r#ref::Ref::Id { id } => {
            assert_ne!(id, &head, "the second role is NOT the head copy");
        }
        other => panic!("the with-role must carry an id ref, got {other:?}"),
    }
    // and the canonical sentence shows it (format_ref renders #id)
    assert!(begin.sentence.contains('#'), "{}", begin.sentence);

    // the mechanic repeats: re-stock and pair the fresh copies off too
    auto_settle_line(&mut game, "explore").unwrap();
    auto_settle_line(&mut game, "study the psalm with the psalm").unwrap();
    assert_eq!(alive(&game, "THE_PSALM"), 0, "the fresh pair binds again");
    assert_eq!(alive(&game, "THE_GREAT_PSALM"), 2, "one mint per pair");
}

// --------------------------------- Game ------------------------------------

#[test]
fn a_script_becomes_a_playable_game() {
    let dict = core_dictionary();
    let mut game = Game::from_script(FLYER, dict).unwrap();
    let turn = auto_settle_line(&mut game, "read the manifest").unwrap();
    assert_eq!(turn.kind, TurnKind::Action);
    assert!(turn.lines.join(" ").contains("passenger manifest"), "{}", turn.lines.join(" "));

    // the timetable speaks: the body is found on the 22:00 crossing
    auto_settle_line(&mut game, "wait").unwrap();
    assert!(game
        .log
        .records
        .iter()
        .any(|r| r.kind == "event" && r.trigger.as_deref() == Some("body_found")));
    assert!(game.journal().join("\n").contains("EVENT body_found"));
    assert!(game.story().unwrap().join("\n").contains("entirely dead"));
    assert!(game.look().join("\n").contains("You are at"));

    // undo takes back the wait, leaving the log consistent
    let n_before = game.log.records.len();
    assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
    assert!(game.log.records.len() < n_before);
    assert_eq!(game.handle_line("bogus").unwrap().kind, TurnKind::Error);
}

#[test]
fn the_table_only_shows_what_the_inspector_has_met() {
    let dict = core_dictionary();
    let mut game = Game::from_script(FLYER, dict).unwrap();
    let names =
        |game: &Game| -> Vec<String> { game.visible().iter().map(|e| e.name.clone()).collect() };
    let n = names(&game);
    assert!(n.contains(&"THE_GUARD".to_string())); // the archetype you can see
    assert!(!n.contains(&"BRACK".to_string())); // the name is learned by talking
    assert!(!n.contains(&"THE_WIDOW".to_string())); // another car: not yet
    assert!(!n.contains(&"THE_VANE_LEDGER".to_string())); // the sealed cabin

    auto_settle_line(&mut game, "explore the dining car").unwrap();
    let n = names(&game);
    assert!(n.contains(&"THE_WIDOW".to_string())); // seen, but not known
    assert!(!n.contains(&"VERITY".to_string()));

    auto_settle_line(&mut game, "talk with the widow").unwrap();
    let n = names(&game);
    assert!(n.contains(&"VERITY".to_string())); // met: the name is known
    assert!(!n.contains(&"THE_WIDOW".to_string()));
}

#[test]
fn inspect_reads_a_visible_cards_face() {
    let dict = core_dictionary();
    let game = Game::from_script(FLYER, dict).unwrap();
    let face = game.inspect("the manifest");
    assert_eq!(face[0], "THE MANIFEST  [text]");
    assert!(face.join("\n").contains("manifest 1"));
    assert!(face.join("\n").contains("at: MARCH"));
    // the sealed cabin's contents are not on the table yet
    assert!(game.inspect("the vane ledger")[0].contains("nothing on the table"));
    assert!(game.inspect("the loch ness monster")[0].contains("nothing on the table"));
}

#[test]
fn suggestions_are_concrete_executable_actions() {
    // The pinned s[0]
    let dict = core_dictionary();
    let game = Game::from_script(FLYER, dict).unwrap();
    let s = game.suggestions().unwrap();
    assert!(!s.is_empty());
    for line in &s {
        let parsed = parse_player_input(
            line,
            &LenientCtx {
                dict: &game.dict,
                content: &game.content,
                state: &game.state,
                player: Some("MARCH"),
            },
        );
        assert!(parsed.is_ok(), "suggestion must parse: {line}");
    }
    assert_eq!(s[0], "read the manifest");
}

#[test]
fn processing_lists_open_windows_soonest_first_begin_id_on_ties() {
    // the processing view (§11): open windows sort by end minute, ties
    // broken by begin_id (commit order); each line reads
    // "<verb display> until <hh:mm>" with the verb lowercased, underscores
    // spaced, the end in the player's clock.
    let dict = core_dictionary();
    let mut game = Game::from_script(PROCESSING_SCRIPT, dict).unwrap();
    assert!(game.processing().is_empty(), "nothing committed, nothing processing");

    for line in ["study the frob", "dream", "use on the glyph"] {
        let t = game.handle_line(line).unwrap();
        assert_eq!(t.kind, TurnKind::Action, "`{line}` must commit: {:?}", t.lines);
    }
    // dream and use_on both end at 0:2 (one hour = two halfhours from 0:0) —
    // the earlier begin (dream, committed first) comes first; study runs an
    // hour longer and closes the list.
    assert_eq!(
        game.processing(),
        vec![
            "dream until 01:00".to_string(),
            "use on until 01:00".to_string(),
            "study until 02:00".to_string(),
        ]
    );
}

#[test]
fn an_absurd_clock_jump_refuses_instead_of_walking_halfhours_forever() {
    // §6.6: the advance pass crosses every halfhour in its span (recurring
    // triggers fire on each) — a giant day field must be a loud refusal, not
    // an unbounded walk
    let dict = core_dictionary();
    let mut game = Game::from_script(FLYER, dict).unwrap();
    for line in ["9999999999:1", "100000:1", "9999999999:1 @ MARCH WAIT"] {
        let t = game.handle_line(line).unwrap();
        assert_eq!(t.kind, TurnKind::Error, "{line}");
        assert!(t.lines[0].contains("more than 7 days ahead"), "{line}: {:?}", t.lines);
        assert_eq!(game.state.now, 1260, "the clock never moved");
    }
}

#[test]
fn game_over_speaks_both_texts_and_undo_reopens() {
    let dict = core_dictionary();
    let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert!(game.state.ended.is_some());
    // engine version (the short text) — rejected, no flavour
    let again = chorus::engine::execute_step(
        &game.content,
        &game.dict,
        chorus::core::types::clone_state(&game.state),
        chorus::engine::Step {
            start_min: None,
            clock: Some(chorus::core::clock::Clock { day: 0, halfhour: 44 }),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 99, id_gen: None },
    )
    .unwrap();
    assert_eq!(again.rejected.as_deref(), Some("the story has ended — No More"));
    // UI version (the full notice) — the info turn, not execution
    let turn = game.handle_line("wait").unwrap();
    assert_eq!(turn.kind, TurnKind::Info);
    assert_eq!(turn.lines, vec!["the story has ended — No More (melancholy)."]);

    // undo across the ending step reopens the game — ended derives from the fold
    assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
    assert_eq!(game.undo().unwrap().kind, TurnKind::Info); // …and its begin
    assert!(game.state.ended.is_none());
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
}

// ------------------------------ fixtures -----------------------------------

const GAME_SCRIPT: &str = "
script: The Long Fall
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

endings:
  DESPAIR_DEATH: \"No More\"
  LANTERN_VICTORY: \"The Light in the Ruins\" (grand)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"The candle gutters.\", ENDING DESPAIR_DEATH

templates:
  WAIT says \"{actors} wait.\"
";

/// A three-station rig for the processing view: two one-hour stations
/// committed in sequence (their windows share an end minute — the begin_id
/// tiebreak speaks) and one two-hour station (a later end).
const PROCESSING_SCRIPT: &str = "
script: Processing Rig
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE
  FROB is item at HOUSE
  GLYPH is item at HOUSE

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 2h
    recipes:
      default -> NARRATE \"study.\"
  DREAM:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"dream.\"
  USE_ON:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 1h
    recipes:
      default -> NARRATE \"use.\"

templates:
  STUDY says \"{actors} study {object}.\"
  DREAM says \"{actors} dream.\"
  USE_ON says \"{actors} use {object}.\"
";
