//! The `hold <card>` REPL face (§11, §6.7): the command's error faces (no
//! held slot, no open station, no fit, ambiguity), the canonical echo, the
//! processing line's held marker, `explain`'s held slot rendering, the
//! try-row's hold-flip suggestion, and undo across a hold returning the card.

use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, TurnKind};

const SCRIPT: &str = "\
script: Held Table
player: ANA
start: 0:0

entities:
  CARREL is place
  ANA is character at CARREL
  BOOK is text at CARREL
  CHALICE is item with quest:1 at CARREL
  CANDLE is item at CARREL

prototypes:
  GLYPH is idea

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..1 aspect:quest
    duration: 30m
    recipes:
      study.quested when count(held) >= 1 -> SPAWN GLYPH, NARRATE \"the {held} hums.\"
      default -> NARRATE \"plain.\"

  WISH:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> NARRATE \"wished.\"

templates:
  STUDY says \"ANA studies {object}.\"
";

fn game() -> Game {
    Game::from_script(SCRIPT, core_dictionary()).expect("the script compiles")
}

/// the one-line bodies the hold faces read
fn last_line(t: &chorus::session::game::Turn) -> String {
    t.lines.last().cloned().unwrap_or_default()
}

#[test]
fn hold_on_a_non_held_verb_and_bare_hold_error_with_usage() {
    let mut g = game();
    let t = g.hold("chalice").unwrap();
    assert_eq!(t.kind, TurnKind::Error);
    assert!(last_line(&t).contains("no open station can take the hold"), "{}", last_line(&t));
    let t = g.hold("").unwrap();
    assert_eq!(t.kind, TurnKind::Error);
    assert!(last_line(&t).starts_with("usage: hold <card>"), "{}", last_line(&t));
    // nothing logged, nothing processing
    assert!(g.state.windows.is_empty());
    assert_eq!(g.log.records.len(), 0);
}

#[test]
fn hold_binds_the_open_station_and_echoes_the_canonical_row() {
    let mut g = game();
    let t = g.handle_line("study the book").unwrap();
    assert_eq!(t.kind, TurnKind::Action);
    // the try row's processing marker names the station
    let t = g.hold("the chalice").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    // the canonical echo: `HOLD STUDY CHALICE` (the §5.1 hold row)
    let echo = t.lines.iter().find(|l| l.starts_with("« ")).expect("the hold echoes");
    assert!(echo.contains("HOLD STUDY CHALICE"), "{echo}");
    // the log carries the hold row and the window holds the card
    assert_eq!(g.log.records.last().unwrap().kind, "hold");
    let w = g.state.windows.values().next().unwrap();
    assert!(w.locked.iter().any(|id| id == "s_chalice"));
}

#[test]
fn hold_rejects_a_card_that_does_not_fit_and_names_what_is_cooking() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    // CANDLE carries no quest aspect
    let t = g.hold("candle").unwrap();
    assert_eq!(t.kind, TurnKind::Error);
    assert!(last_line(&t).contains("does not fit slot 'held' of STUDY"), "{}", last_line(&t));
}

#[test]
fn the_processing_line_names_what_a_station_holds() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    let before = g.processing();
    assert_eq!(before, ["study until 00:30"]);
    g.hold("the chalice").unwrap();
    let after = g.processing();
    assert_eq!(after, ["study until 00:30 (holding CHALICE)"]);
}

#[test]
fn undo_across_the_hold_returns_the_card() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    g.hold("the chalice").unwrap();
    // the hold was its own turn — undo takes exactly it back
    let t = g.undo().unwrap();
    assert_eq!(t.kind, TurnKind::Info);
    assert!(
        t.lines.iter().any(|l| l.contains("HOLD STUDY CHALICE")),
        "undo names what it took back: {:?}",
        t.lines
    );
    // the window stands, unheld; the card unlocked
    let w = g.state.windows.values().next().unwrap();
    assert!(!w.locked.iter().any(|id| id == "s_chalice"));
    assert_eq!(g.log.records.len(), 1, "the hold row left the log");
    // and the completion re-selects with the empty scope: the default lands
    g.handle_line("0:3").unwrap();
    let journal = g.journal().remove(0);
    assert!(journal.contains("[study.default]"), "{journal}");
    assert!(g.state.by_name.contains_key("CHALICE"), "no hold, no consumption");
}

#[test]
fn the_try_row_suggests_the_hold_that_flips_selection() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    // the station is processing; the play suggestions are gone (the verb is
    // busy), but the hold flip is live: quested fires only under the hold
    let lines = g.after_turn().unwrap();
    let try_row = lines.iter().find(|l| l.starts_with("try: ")).expect("the try row");
    assert!(try_row.contains("hold chalice"), "the flip suggests the hold: {try_row}");
    // a verb without an open station never suggests holds
    let mut g2 = game();
    let lines2 = g2.after_turn().unwrap();
    for line in &lines2 {
        assert!(!line.contains("hold "), "{line}");
    }
}

#[test]
fn the_hold_suggestion_is_typeable_verbatim() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    let lines = g.after_turn().unwrap();
    let try_row = lines.iter().find(|l| l.starts_with("try: ")).unwrap().clone();
    let items: Vec<String> =
        try_row.trim_start_matches("try: ").split(" · ").map(str::to_string).collect();
    let item = items.iter().find(|s| s.starts_with("hold ")).expect("the hold suggestion").clone();
    let t = g.hold(item.trim_start_matches("hold ")).unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
}

#[test]
fn the_held_recipe_completes_through_the_session_and_prose_reads_the_role() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    g.hold("the chalice").unwrap();
    g.handle_line("0:3").unwrap();
    // the story reads the held card through the {held} template token
    let story = g.story().unwrap();
    let prose = story.join("\n");
    assert!(prose.contains("the CHALICE hums."), "{prose}");
    let journal = g.journal().remove(0);
    assert!(journal.contains("[study.quested]"), "{journal}");
}

#[test]
fn explain_renders_the_held_slot_and_its_scope() {
    let mut g = game();
    let lines = g.explain("study").unwrap();
    let slots = lines.iter().find(|l| l.starts_with("slots:")).expect("the slots line");
    assert!(slots.contains("held 0..1 aspect:quest"), "{slots}");
    let held = lines.iter().find(|l| l.starts_with("held:")).expect("the held line");
    assert!(held.contains("hold <card>"), "{held}");
    // with a hold bound, the held line names the card
    g.handle_line("study the book").unwrap();
    g.hold("the chalice").unwrap();
    let lines = g.explain("study").unwrap();
    let held = lines.iter().find(|l| l.starts_with("held:")).expect("the held line");
    assert!(held.contains("CHALICE"), "{held}");
    // a verb without a held slot gains no held line
    let lines = g.explain("wish").unwrap();
    assert!(lines.iter().all(|l| !l.starts_with("held:")), "{lines:?}");
}

#[test]
fn hold_for_verb_disambiguates_and_the_bin_routes_the_word() {
    let mut g = game();
    g.handle_line("study the book").unwrap();
    // the `for <verb>` tail names the station (one station here — accepted)
    let t = g.hold("the chalice for study").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    // the for-tail also types through a verb the card does NOT fit
    let mut g2 = game();
    g2.handle_line("study the book").unwrap();
    let t2 = g2.hold("the chalice for wish").unwrap();
    assert_eq!(t2.kind, TurnKind::Error);
    assert!(last_line(&t2).contains("wish cannot take the hold"), "{}", last_line(&t2));
}

/// The slot class (v0.9.9) at the REPL: a `held: 0..2` station takes a
/// second hold, the processing line names both, the third hold meets the
/// engine's at-max refusal, and `explain` names the bindings.
#[test]
fn the_slot_class_holds_to_its_fill_at_the_repl() {
    let mut g = Game::from_script(
        "\
script: Held Range
player: ANA
start: 0:0

entities:
  CARREL is place
  ANA is character at CARREL
  BOOK is text at CARREL
  CHALICE is item with quest:1 at CARREL
  RELIC is item with quest:2 at CARREL
  CROWN is item with quest:3 at CARREL

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..2 aspect:quest
    duration: 30m
    recipes:
      study.twin when count(held) >= 2 -> NARRATE \"twins.\"
      default -> NARRATE \"plain.\"
",
        core_dictionary(),
    )
    .expect("the script compiles");
    g.handle_line("study the book").unwrap();
    let t = g.hold("the chalice").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{}", last_line(&t));
    let t = g.hold("the relic").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{}", last_line(&t));
    // the processing line names both bindings
    let after = g.after_turn().unwrap();
    assert!(after.iter().any(|l| l.contains("(holding CHALICE, RELIC)")), "{after:?}");
    // the third fitting card meets the at-max refusal
    let t = g.hold("the crown").unwrap();
    assert_eq!(t.kind, TurnKind::Error);
    assert!(
        last_line(&t).contains("STUDY already holds its fill — the held slot is full"),
        "{}",
        last_line(&t)
    );
    // undo takes exactly the last hold back: one card rides out
    g.undo().unwrap();
    let after = g.after_turn().unwrap();
    assert!(after.iter().any(|l| l.contains("(holding CHALICE)")), "{after:?}");
}
