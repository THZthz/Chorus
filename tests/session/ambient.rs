//! The ambient verb's player-surface faces (§6.9/§11): the try row never
//! suggests one, the verb lists mark it (`runs on its own`), `explain`
//! carries the ambient line, a typed commit refuses, and undo across an
//! ambient completion returns the minted card — play == fold == undo, and
//! the saved log re-folds to the same world (`reach --from-state`'s
//! replay path agrees).

use chorus::content::script::compile_script;
use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, TurnKind};

const SCRIPT: &str = "
script: Ambient Table
player: ANA
start: 0:0

entities:
  ANA is character at HOUSE
  HOUSE is place

prototypes:
  TICK is idea stackable

verbs:
  TIME:
    ambient
    duration: 1h
    recipes:
      time.pulse dur 10m -> SPAWN TICK, NARRATE \"the hour turns.\"
  WORK:
    actors: 1 kind:character
    duration: 30m
    recipes:
      default -> NARRATE \"worked.\"
";

fn game() -> Game {
    Game::from_script(SCRIPT, core_dictionary()).unwrap()
}

#[test]
fn the_banner_marks_the_ambient_verb_and_the_try_row_skips_it() {
    let g = game();
    let banner = g.banner().unwrap();
    let verbs_row = banner.iter().find(|l| l.starts_with("verbs: ")).unwrap();
    assert!(
        verbs_row.contains("time (runs on its own)"),
        "the ambient verb carries the mark: {verbs_row}"
    );
    let try_row = banner.iter().find(|l| l.starts_with("try: ")).unwrap();
    assert!(!try_row.contains("time"), "the try row never suggests the ambient verb: {try_row}");
    assert!(try_row.contains("work"), "the plain verb still suggests: {try_row}");
}

#[test]
fn a_typed_commit_refuses_with_the_ambient_line() {
    let mut g = game();
    let turn = g.handle_line("time").unwrap();
    assert_eq!(turn.kind, TurnKind::Error);
    assert!(turn.lines[0].starts_with("TIME runs on its own"), "{}", turn.lines[0]);
    // nothing logged — the rejection is a no-op turn
    assert!(g.log.records.is_empty());
}

#[test]
fn explain_marks_the_ambient_verb() {
    let g = game();
    let lines = g.explain("time").unwrap();
    assert!(lines.iter().any(|l| l.starts_with("ambient: runs on its own")), "{lines:?}");
    // the usage face's verb list carries the mark too
    let usage = g.explain("").unwrap();
    assert!(usage.iter().any(|l| l.contains("time (runs on its own)")), "{usage:?}");
}

#[test]
fn the_journal_and_prose_speak_the_pulse() {
    let mut g = game();
    // wait half an hour: the sweep opens at the first crossing (minute 10)
    // and the pulse completes at minute 20
    let _ = g.handle_line("0:3").unwrap();
    assert!(g.state.entities.values().any(|e| e.dead_action.is_none() && e.name == "TICK"));
    // the journal names the ambient open with its recipe id
    let journal = g.journal();
    assert!(journal[0].contains("@ TIME [time.pulse]"), "{journal:?}");
    // the prose narrates the pulse's NARRATE
    let story = g.story().unwrap().join("\n");
    assert!(story.contains("the hour turns."), "{story}");
}

#[test]
fn undo_across_an_ambient_completion_returns_the_minted_card() {
    let mut g = game();
    let before: usize = g.state.entities.values().filter(|e| e.dead_action.is_none()).count();
    let _ = g.handle_line("0:2").unwrap();
    let after: usize = g.state.entities.values().filter(|e| e.dead_action.is_none()).count();
    assert_eq!(after, before + 1, "the pulse minted one TICK");
    let turn = g.undo().unwrap();
    assert_eq!(turn.kind, TurnKind::Info);
    assert!(
        g.state.entities.values().filter(|e| e.dead_action.is_none()).count() == before,
        "undo returned the minted card"
    );
    assert!(g.state.windows.is_empty(), "undo re-folds: the ambient window is gone");
    // and the clock is back where the turn began
    assert_eq!(g.state.now, 0);
}

#[test]
fn a_saved_midpulse_log_refolds_to_the_same_world() {
    let mut g = game();
    // stop MID-PULSE: the sweep's window is open, nothing completed yet
    let _ = g.handle_line("0:1").unwrap();
    let dir = std::env::temp_dir().join(format!("chorus-ambient-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("midpulse.jsonl");
    let saved = g.save(path.to_str().unwrap()).unwrap();
    assert!(saved.starts_with("saved:"), "{saved}");
    // the `reach --from-state` replay path (load_log + fold) rebuilds the
    // same world — open ambient window included
    let compiled = compile_script(SCRIPT, &core_dictionary()).unwrap();
    let replayed = chorus::reach::midgame::replay_state(
        &path,
        &compiled.state,
        &compiled.dict,
        &compiled.content,
    )
    .unwrap();
    assert_eq!(
        replayed.windows.len(),
        g.state.windows.len(),
        "the ambient window replays through the ONE registration path"
    );
    assert_eq!(replayed.now, g.state.now);
    let _ = std::fs::remove_dir_all(&dir);
}
