//! Smoke tests for the session layer (lenient/render/game). Anchors
//! exercised here, all pinned by the acceptance suites:
//! - lenient: bare-verb default player, multi-word names, role-phrase groups,
//!   canonical passthrough, suggestion surfaces, case-insensitive names;
//! - render: the ending banner/notice texts, the prose closing line;
//! - Game: from_script → handle_line → journal/story/look → undo → bogus
//!   error, visible/inspect projection, suggestions with `s[0]` pinned,
//!   and BOTH game-over texts through a
//!   minimal endings script.

use chorus::content::content::{compile_content, ContentRaw, EndingDef, RecipeDef, SlotDef, VerbContent};
use chorus::core::dict::core_dictionary;
use chorus::engine::engine::ExecOptions;
use chorus::error::Result;
use chorus::session::game::{Game, TurnKind};
use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};
use chorus::session::render::{ending_banner, ending_notice, prose_lines};

mod common;

const FLYER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/the-nightjar-flyer.txt"
));

/// The v0.5 test adapter: settle each
/// committed action's window right away, so loop tests keep their
/// pre-parallel commit-completes-at-once shape. (The game-loop suites
/// mount this same helper.)
fn auto_settle_line(game: &mut Game, line: &str) -> Result<chorus::session::game::Turn> {
    let t = game.handle_line(line)?;
    let t = if t.kind == TurnKind::Action && !game.state.windows.is_empty() {
        let settle = game.wait()?;
        chorus::session::game::Turn {
            kind: TurnKind::Action,
            lines: [t.lines, settle.lines].concat(),
        }
    } else {
        t
    };
    Ok(t)
}

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
    assert!(matches!(&c0.actors[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "MARCH"));

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
    assert!(matches!(&c0.objects[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_INQUEST"));
    assert!(matches!(&c0.with[0], chorus::core::r#ref::Ref::Name { name, .. } if name == "THE_MANIFEST"));
    assert!(matches!(&c0.at, Some(chorus::core::r#ref::Ref::Name { name, .. }) if name == "GUARD_VAN"));

    // 'canonical passthrough still works'
    let ParsedInput::Ok { step, .. } = parse_player_input("0:44 @ MARCH WAIT", &ctx) else {
        panic!("passthrough must parse");
    };
    assert_eq!(step.clock, Some(chorus::core::types::Clock { day: 0, halfhour: 44 }));

    // 'unknown words get suggestions' — the failed word is 'vane',
    // so the suggestion rides the normalized-prefix path
    let ParsedInput::Fail { suggestions, .. } = parse_player_input("read the vane ledgr", &ctx)
    else {
        panic!("'ledgr' must fail");
    };
    assert!(suggestions.contains(&"THE_VANE_LEDGER".to_string()), "{suggestions:?}");

    // 'verbs get suggestions too'
    let ParsedInput::Fail { suggestions, .. } = parse_player_input("xeplore the car", &ctx) else {
        panic!("'xeplore' must fail");
    };
    assert!(suggestions.contains(&"explore".to_string()), "{suggestions:?}");

    // 'names resolve case-insensitively (spec §11)'
    assert!(parse_player_input("read THE VANE LEDGER", &ctx).is_ok());
}

// -------------------------------- render -----------------------------------

#[test]
fn render_banner_notice_and_the_prose_closing_line() {
    let e = chorus::core::types::Ending {
        id: "DESPAIR_DEATH".into(),
        title: "No More".into(),
        flavour: "melancholy".into(),
    };
    assert_eq!(ending_banner(&e), "— NO MORE · melancholy —");
    assert_eq!(ending_notice(&e), "the story has ended — No More (melancholy).");

    // the prose of the ending step carries the closing line; others do not
    // — a WAIT recipe whose outcomes end the story
    let dict = core_dictionary();
    let raw = ending_rig();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let world = seed_world_with_ana();
    let s = parse_playerish(&dict, &content, "0:28 @ ANA WAIT", &world);
    let r = chorus::engine::engine::execute_step(
        &content,
        &dict,
        world,
        s,
        ExecOptions { first_record_id: 1, id_gen: None },
    )
    .unwrap();
    // the step begins a window; complete it with a pure advance to its end
    let end = r.state.windows.values().map(|w| w.end).max().unwrap_or(0);
    let r = chorus::engine::engine::execute_step(
        &content,
        &dict,
        r.state,
        chorus::engine::engine::Step {
            start_min: Some(end),
            clock: None,
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 2, id_gen: None },
    )
    .unwrap();
    let complete = r
        .records
        .iter()
        .find(|x| x.kind == "complete")
        .expect("the window completes");
    let lines = prose_lines(complete, &content, &dict).unwrap();
    assert_eq!(lines[lines.len() - 1], "— No More —");
}

// --------------------------------- Game ------------------------------------

#[test]
fn a_script_becomes_a_playable_game() {
    let dict = core_dictionary();
    let mut game = Game::from_script(FLYER, dict).unwrap();
    let turn = auto_settle_line(&mut game, "read the manifest").unwrap();
    assert_eq!(turn.kind, TurnKind::Action);
    assert!(
        turn.lines.join(" ").contains("passenger manifest"),
        "{}",
        turn.lines.join(" ")
    );

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
    let names = |game: &Game| -> Vec<String> { game.visible().iter().map(|e| e.name.clone()).collect() };
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
    assert_eq!(face[0], "THE_MANIFEST  [text]");
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
    assert_eq!(s[0], "read THE MANIFEST");
}

#[test]
fn game_over_speaks_both_texts_and_undo_reopens() {
    let dict = core_dictionary();
    let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert!(game.state.ended.is_some());
    // engine version (the short text) — rejected, no flavour
    let again = chorus::engine::engine::execute_step(
        &game.content,
        &game.dict,
        chorus::core::types::clone_state(&game.state),
        chorus::engine::engine::Step {
            start_min: None,
            clock: Some(chorus::core::types::Clock { day: 0, halfhour: 44 }),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 99, id_gen: None },
    )
    .unwrap();
    assert_eq!(
        again.rejected.as_deref(),
        Some("the story has ended — No More")
    );
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

/// A WAIT
/// verb whose recipe ends the story, plus the §10 endings table.
fn ending_rig() -> ContentRaw {
    let mut endings = indexmap::IndexMap::new();
    endings.insert(
        "DESPAIR_DEATH".to_string(),
        EndingDef {
            title: "No More".into(),
            flavour: None,
        },
    );
    endings.insert(
        "LANTERN_VICTORY".to_string(),
        EndingDef {
            title: "The Light in the Ruins".into(),
            flavour: Some("grand".into()),
        },
    );
    ContentRaw {
        verbs: [(
            "WAIT".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                }],
                duration: 60,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    out: vec![
                        "NARRATE \"last light.\"".to_string(),
                        "ENDING DESPAIR_DEATH".to_string(),
                    ],
                }],
            },
        )]
        .into_iter()
        .collect(),
        endings: Some(endings),
        ..ContentRaw::default()
    }
}

/// (the ANA-in-HOUSE slice the endings rig plays on)
fn seed_world_with_ana() -> chorus::core::types::State {
    use chorus::core::types::{add_entity, new_state, AttrValue, Attrs, Entity};
    let mut s = new_state(0);
    for (id, kind, name, container) in [
        ("house", "place", "HOUSE", None),
        ("ana", "character", "ANA", Some("house")),
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
                stackable: false,
            },
        )
        .unwrap();
    }
    s
}

fn parse_playerish(
    dict: &chorus::core::dict::Dictionary,
    content: &chorus::content::content::CompiledContent,
    src: &str,
    state: &chorus::core::types::State,
) -> chorus::engine::engine::Step {
    let parsed = parse_player_input(
        src,
        &LenientCtx {
            dict,
            content,
            state,
            player: Some("ANA"),
        },
    );
    let ParsedInput::Ok { step, .. } = parsed else {
        panic!("'{src}' must parse");
    };
    step
}
